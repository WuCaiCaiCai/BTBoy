//! TMDB 封面：搜索番名 → 选海报 URL → 下载图片字节
use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct TmdbHit {
    pub title: String,
    pub year: Option<String>,
    pub poster: Option<String>,
}

/// 搜索电视剧，返回命中列表（zh-CN 优先，fallback en-US）
pub async fn search(
    http: &reqwest::Client,
    api_key: &str,
    title: &str,
) -> Result<Vec<TmdbHit>> {
    let q = url::form_urlencoded::byte_serialize(title.as_bytes()).collect::<String>();
    for lang in ["zh-CN", "en-US"] {
        let url = format!(
            "https://api.themoviedb.org/3/search/tv?api_key={api_key}&language={lang}&query={q}"
        );
        let resp = http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("TMDB 请求失败: {url}"))?
            .error_for_status()?;
        let json: serde_json::Value = resp.json().await.context("TMDB 响应解析失败")?;
        let hits: Vec<TmdbHit> = json["results"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|r| TmdbHit {
                title: r["name"].as_str().unwrap_or("").to_string(),
                year: r["first_air_date"]
                    .as_str()
                    .map(|s| s.chars().take(4).collect()),
                poster: r["poster_path"]
                    .as_str()
                    .map(|p| format!("https://image.tmdb.org/t/p/w500{p}")),
            })
            .collect();
        if !hits.is_empty() {
            return Ok(hits);
        }
    }
    Ok(Vec::new())
}

/// 下载海报图片字节
pub async fn download_poster(http: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
    let resp = http.get(url).send().await.ok()?;
    resp.bytes().await.ok().map(|b| b.to_vec())
}
