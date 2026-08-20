use crate::models::{Candidate, SubRow, fmt_episode};

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
