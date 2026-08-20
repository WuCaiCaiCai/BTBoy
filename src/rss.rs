use anyhow::{Context, Result};
use regex::Regex;
use rss::Channel;

use crate::models::RssItem;

static RE_MAGNET: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

fn magnet_re() -> &'static Regex {
    RE_MAGNET.get_or_init(|| {
        Regex::new(r#"magnet:\?xt=urn:btih:[a-zA-Z0-9]+[^\s"'<>]*"#).unwrap()
    })
}

fn extract_magnet_from_html(s: &str) -> Option<String> {
    magnet_re()
        .find(s)
        .map(|m| m.as_str().trim_end_matches('"').trim_end_matches('\'').to_string())
}

/// 拉取并解析 RSS。磁力可能来自 enclosure（Mikan），也可能是 .torrent 链接（bangumi.moe）
pub async fn fetch_rss(http: &reqwest::Client, url: &str) -> Result<Vec<RssItem>> {
    let resp = http
        .get(url)
        .send()
        .await
        .with_context(|| format!("请求 RSS 失败: {url}"))?
        .error_for_status()
        .with_context(|| format!("RSS 状态异常: {url}"))?;
    let body = resp.text().await.context("读取 RSS 响应失败")?;
    parse_rss_bytes(body.as_bytes())
}

/// 从 XML 字节解析 RSS（供测试/离线使用）
pub fn parse_rss_bytes(body: &[u8]) -> Result<Vec<RssItem>> {
    let channel = Channel::read_from(body).context("解析 RSS XML 失败")?;

    let items = channel
        .items()
        .iter()
        .map(|it| {
            let title = it.title().unwrap_or_default().to_string();
            let link = it.link().unwrap_or_default().to_string();
            let guid = it
                .guid()
                .map(|g| g.value().to_string())
                .unwrap_or_default();
            let enclosure_url = it.enclosure().map(|e| e.url().to_string());
            // 磁力直接取；否则保留 enclosure/description 里的 magnet，或留作 .torrent 解析
            let magnet = enclosure_url
                .as_ref()
                .filter(|u| u.starts_with("magnet:"))
                .cloned()
                .or_else(|| it.description().and_then(extract_magnet_from_html));
            RssItem {
                title,
                link,
                magnet,
                enclosure_url,
                guid,
            }
        })
        .collect();

    Ok(items)
}

/// 解析磁力（通用，不依赖任何站点）：
/// 1) RSS 里已带的 magnet（enclosure 或 description）
/// 2) enclosure 是 .torrent 链接 → 按原链接下载，解析 info 字典算 infohash 生成 magnet
pub async fn resolve_magnet(http: &reqwest::Client, item: &RssItem) -> Option<String> {
    if let Some(m) = &item.magnet {
        return Some(m.clone());
    }
    let url = item.enclosure_url.as_ref()?;
    if !looks_like_torrent(url) {
        return None;
    }
    download_torrent_magnet(http, url).await
}

async fn download_torrent_magnet(http: &reqwest::Client, url: &str) -> Option<String> {
    let resp = match http.get(url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("下载 .torrent 失败 {url}: {e}");
            return None;
        }
    };
    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("下载 .torrent 失败 {url}: {e}");
            return None;
        }
    };
    match torrent_to_magnet(&bytes) {
        Some(m) => {
            tracing::info!("从 .torrent 解析到磁力: {m}");
            Some(m)
        }
        None => {
            tracing::warn!("解析 .torrent 失败: {url}");
            None
        }
    }
}

pub(crate) fn looks_like_torrent(url: &str) -> bool {
    url.to_ascii_lowercase().ends_with(".torrent") || url.contains("/torrent/")
}

/// .torrent 字节 → magnet:?xt=urn:btih:<infohash>&tr=<tracker>...
/// 保留 announce / announce-list 的 tracker，保证磁力可正常被客户端解析下载
pub fn torrent_to_magnet(data: &[u8]) -> Option<String> {
    use sha1::Digest;
    let info = crate::bencode::dict_get(data, b"info")?;
    let hash = sha1::Sha1::digest(info);
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();

    let mut magnet = format!("magnet:?xt=urn:btih:{hex}");
    let mut trackers: Vec<String> = Vec::new();
    if let Some(a) = crate::bencode::dict_get_str(data, b"announce") {
        trackers.push(a.to_string());
    }
    if let Some(v) = crate::bencode::dict_get(data, b"announce-list") {
        if let Some(list) = crate::bencode::list_of_lists_strings(v, 0) {
            for t in list {
                if !trackers.contains(&t) {
                    trackers.push(t);
                }
            }
        }
    }
    for t in trackers {
        magnet.push_str("&tr=");
        magnet.push_str(&percent_encode(&t));
    }
    Some(magnet)
}

/// RFC3986 查询串百分号编码（保留 -._~ 与字母数字）
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn torrent_to_magnet_ok() {
        // 最小合法 .torrent：{"info":{"name":"a","length":5}}
        let data = b"d4:infod4:name1:a6:lengthi5eeee";
        let m = torrent_to_magnet(data).unwrap();
        assert!(m.starts_with("magnet:?xt=urn:btih:"));
        assert_eq!(m.len(), "magnet:?xt=urn:btih:".len() + 40);
    }

    #[test]
    fn invalid_torrent() {
        assert!(torrent_to_magnet(b"garbage").is_none());
    }

    #[test]
    fn torrent_url_detected() {
        assert!(looks_like_torrent("https://x.com/download/torrent/abc123/foo.torrent"));
        assert!(looks_like_torrent("http://x.com/torrent/abc123"));
        assert!(!looks_like_torrent("https://x.com/episode/123"));
    }

    #[test]
    fn magnet_includes_trackers() {
        // 含 announce + announce-list 的 .torrent
        let data = b"d8:announce21:http://tr.example.com13:announce-listll21:http://tr.example.comel26:udp://tr2.example.com:8080ee4:infod4:name1:a6:lengthi5eee";
        let m = torrent_to_magnet(data).unwrap();
        assert_eq!(
            m,
            format!(
                "magnet:?xt=urn:btih:{hash}&tr=http%3A%2F%2Ftr.example.com&tr=udp%3A%2F%2Ftr2.example.com%3A8080",
                hash = sha1_hex(b"d4:name1:a6:lengthi5ee")
            )
        );
    }

    #[test]
    fn magnet_without_trackers() {
        let data = b"d4:infod4:name1:a6:lengthi5ee";
        let m = torrent_to_magnet(data).unwrap();
        assert!(m.starts_with("magnet:?xt=urn:btih:"));
        assert!(!m.contains("&tr="));
    }

    fn sha1_hex(bytes: &[u8]) -> String {
        use sha1::Digest;
        sha1::Sha1::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}
