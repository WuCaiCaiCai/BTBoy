use std::time::Duration;

use crate::models::{Candidate, SubRow, fmt_episode};

/// 最近一次 Telegram 发送的时间戳（毫秒），用于全局节流
static LAST_SEND_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 保证两次发送之间至少间隔 1.2s，避免批量推送触发 Telegram 429
pub async fn throttle() {
    const MIN_MS: u64 = 1200;
    loop {
        let now = now_millis();
        let last = LAST_SEND_MS.load(std::sync::atomic::Ordering::Relaxed);
        let elapsed = now.saturating_sub(last);
        if elapsed >= MIN_MS {
            LAST_SEND_MS.store(now, std::sync::atomic::Ordering::Relaxed);
            return;
        }
        tokio::time::sleep(Duration::from_millis(MIN_MS - elapsed)).await;
    }
}

/// 发送前节流；遇到 `RetryAfter` 限流时等待后重试一次
pub async fn send_retry<F, Fut, T>(mut f: F) -> Result<T, teloxide::RequestError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, teloxide::RequestError>>,
{
    throttle().await;
    match f().await {
        Err(teloxide::RequestError::RetryAfter(secs)) => {
            tracing::warn!("Telegram 限流，等待 {}s 后重试", secs.seconds());
            tokio::time::sleep(secs.duration() + Duration::from_secs(1)).await;
            throttle().await;
            f().await
        }
        other => other,
    }
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// 频道固定推送格式：
/// 【番名 第07话】
/// [字幕组 · 1080P · HEVC · 简中 · v2]
/// magnet:?xt=...
pub fn format_push(sub: &SubRow, c: &Candidate) -> String {
    let ep = fmt_episode(c.episode);
    let mut parts: Vec<String> = Vec::new();
    if let Some(f) = &c.fansub {
        parts.push(f.clone());
    }
    if let Some(s) = &c.source {
        parts.push(s.clone());
    }
    if let Some(q) = &c.quality {
        parts.push(q.clone());
    }
    if let Some(cd) = &c.codec {
        parts.push(cd.clone());
    }
    if c.lang != "未知" {
        parts.push(c.lang.clone());
    }
    if c.version > 1 {
        parts.push(format!("v{}", c.version));
    }
    let attrs = if parts.is_empty() {
        String::new()
    } else {
        format!("<i>[{}]</i>", parts.join(" · "))
    };

    format!(
        "<b>{title} 第{ep}话</b>\n{attrs}\n\n{magnet}",
        title = html_escape(&sub.title),
        ep = ep,
        attrs = attrs,
        magnet = c.magnet,
    )
}

/// 冲突询问消息里的单条候选展示
pub fn candidate_line(c: &Candidate, idx: usize) -> String {
    let mut parts: Vec<String> = Vec::new();
    if c.version > 1 {
        parts.push(format!("v{}", c.version));
    } else {
        parts.push("v1".into());
    }
    if let Some(s) = &c.source {
        parts.push(s.clone());
    }
    if c.lang != "未知" {
        parts.push(c.lang.clone());
    }
    if let Some(q) = &c.quality {
        parts.push(q.clone());
    }
    if let Some(cd) = &c.codec {
        parts.push(cd.clone());
    }
    let hash: String = c
        .magnet
        .find("btih:")
        .and_then(|i| c.magnet[i + 5..].get(..8))
        .unwrap_or("????")
        .to_string();
    format!("{} · {} · …{}", idx + 1, parts.join(" · "), hash)
}
