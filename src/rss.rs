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

/// 拉取并解析 RSS，返回条目列表（Mikan 的磁力在 enclosure url 或 description 里）
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
            let magnet = it
                .enclosure()
                .map(|e| e.url().to_string())
                .filter(|u| u.starts_with("magnet:"))
                .or_else(|| {
                    it.description()
                        .and_then(extract_magnet_from_html)
                });
            RssItem {
                title,
                link,
                magnet,
                guid,
            }
        })
        .collect();

    Ok(items)
}
