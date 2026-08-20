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

    let channel = Channel::read_from(body.as_bytes()).context("解析 RSS XML 失败")?;

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

/// 解析磁力。优先用 RSS 里的 magnet；否则若 enclosure 是 .torrent 链接则下载并算 infohash。
pub async fn resolve_magnet(http: &reqwest::Client, item: &RssItem) -> Option<String> {
    if let Some(m) = &item.magnet {
        return Some(m.clone());
    }
    let url = item.enclosure_url.as_ref()?;
    if !looks_like_torrent(url) {
        return None;
    }
    match http.get(url).send().await {
        Ok(resp) => match resp.bytes().await {
            Ok(bytes) => match torrent_to_magnet(&bytes) {
                Some(magnet) => Some(magnet),
                None => {
                    tracing::warn!("解析 .torrent 失败: {url}");
                    None
                }
            },
            Err(e) => {
                tracing::warn!("下载 .torrent 失败 {url}: {e}");
                None
            }
        },
        Err(e) => {
            tracing::warn!("下载 .torrent 失败 {url}: {e}");
            None
        }
    }
}

fn looks_like_torrent(url: &str) -> bool {
    url.to_ascii_lowercase().ends_with(".torrent") || url.contains("/torrent/")
}

/// .torrent 字节 → magnet:?xt=urn:btih:<infohash>
pub fn torrent_to_magnet(data: &[u8]) -> Option<String> {
    use sha1::Digest;
    let info = crate::bencode::dict_get(data, b"info")?;
    let hash = sha1::Sha1::digest(info);
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    Some(format!("magnet:?xt=urn:btih:{hex}"))
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
}
