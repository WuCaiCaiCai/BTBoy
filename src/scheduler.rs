use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use teloxide::prelude::*;
use teloxide::types::{ChatId, InlineKeyboardButton, InlineKeyboardMarkup, ParseMode};
use tokio::time::sleep;

use crate::db;
use crate::filter::filter_by_lang;
use crate::models::{Candidate, SubRow, fmt_episode};
use crate::notifier;
use crate::parser::parse_title;
use crate::rss::fetch_rss;
use crate::AppState;

#[derive(Debug, Default)]
pub struct ProcessReport {
    pub new: usize,
    pub pushed: usize,
    pub asked: usize,
}

#[derive(Debug)]
pub(crate) enum Decision {
    Push(Vec<Candidate>),
    Ask,
    Skip,
}

pub async fn run(state: Arc<AppState>) {
    let mut bgm_last_check = String::new();
    loop {
        let t0 = std::time::Instant::now();
        if let Err(e) = process_all(&state).await {
            tracing::error!("调度循环错误: {e}");
        }
        if let Err(e) = refresh_bgm_if_due(&state, &mut bgm_last_check).await {
            tracing::error!("BGM 总集数刷新失败: {e}");
        }
        let interval_min =
            db::meta_int(&state.db, "fetch_interval_min", state.config.fetch_interval_min as i64)
                .max(1) as u64;
        let wait = (interval_min * 60).saturating_sub(t0.elapsed().as_secs());
        sleep(Duration::from_secs(wait.max(30))).await;
    }
}

/// 每 12 小时用 Bangumi 刷新一次已绑定 bgm_id 订阅的总集数
async fn refresh_bgm_if_due(state: &AppState, last: &mut String) -> Result<()> {
    let now_ts = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    if !last.is_empty() && now_ts == *last {
        return Ok(());
    }
    let bgm_subs = db::list_subs_with_bgm(&state.db)?;
    if bgm_subs.is_empty() {
        *last = now_ts;
        return Ok(());
    }
    for (id, bgm_id) in bgm_subs {
        match fetch_bgm_total(&state.http, bgm_id).await {
            Ok(Some(total)) => {
                let old = db::get_subscription(&state.db, id)?
                    .and_then(|s| s.total_episodes)
                    .unwrap_or(0);
                if total > 0 && total != old {
                    db::update_total_episodes(&state.db, id, total)?;
                    let sub = db::get_subscription(&state.db, id)?;
                    if let Some(sub) = sub {
                        inform(
                            state,
                            &format!(
                                "📚 <b>{}</b> BGM 总集数更新: {} → {}",
                                notifier::html_escape(&sub.title),
                                old,
                                total
                            ),
                        )
                        .await?;
                    }
                }
            }
            Ok(None) => {}
            Err(e) => tracing::warn!("sub#{id} BGM 拉取失败: {e}"),
        }
    }
    *last = now_ts;
    Ok(())
}

pub(crate) async fn fetch_bgm_total(http: &reqwest::Client, bgm_id: i64) -> Result<Option<i64>> {
    let url = format!("https://api.bgm.tv/v0/subjects/{bgm_id}");
    let resp = http.get(&url).send().await?.error_for_status()?;
    let json: serde_json::Value = resp.json().await?;
    Ok(json
        .get("total_episodes")
        .and_then(|v| v.as_i64())
        .filter(|n| *n > 0))
}

pub async fn process_all(state: &Arc<AppState>) -> Result<()> {
    if !db::meta_bool(&state.db, "rss_enabled", true) {
        return Ok(());
    }
    let subs = db::list_subscriptions(&state.db)?;
    for sub in subs {
        if !sub.enabled {
            continue;
        }
        match process_subscription(state, &sub).await {
            Ok(r) => {
                if r.new > 0 || r.pushed > 0 || r.asked > 0 {
                    tracing::info!(
                        "sub#{} 新候选 {} · 推送 {} · 询问 {}",
                        sub.id,
                        r.new,
                        r.pushed,
                        r.asked
                    );
                }
                if let Err(e) = post_checks(state, &sub).await {
                    tracing::error!("sub#{} 后置检查失败: {e}", sub.id);
                }
            }
            Err(e) => {
                tracing::error!("sub#{} 处理失败: {e}", sub.id);
                notify_error(state, &sub.title, &e.to_string()).await;
            }
        }
    }
    Ok(())
}

/// 遗漏检测 / 摸鱼检测 / 自动禁用
async fn post_checks(state: &Arc<AppState>, sub: &SubRow) -> Result<()> {
    // 自动禁用：总集数已知且已全部推送
    if db::meta_bool(&state.db, "autodisable", false) {
        if let Some(total) = sub.total_episodes {
            let pushed = db::count_pushed_for_sub(&state.db, sub.id)?;
            if total > 0 && pushed >= total {
                db::set_sub_enabled(&state.db, sub.id, false)?;
                inform(
                    state,
                    &format!(
                        "🏁 <b>{}</b> 全部 {} 集已推送，订阅已自动停用",
                        notifier::html_escape(&sub.title),
                        total
                    ),
                )
                .await?;
                return Ok(());
            }
        }
    }

    // 遗漏检测：范围内缺集通知（>10 视为误判不通知）
    if db::meta_bool(&state.db, "gap_detect", false) {
        let pushed = db::pushed_episodes(&state.db, sub.id)?;
        let start = sub.start_episode.max(1);
        let mut missing: Vec<i64> = Vec::new();
        if let Some(&max) = pushed.iter().max() {
            let have: std::collections::HashSet<i64> =
                pushed.iter().copied().filter(|e| *e >= start).collect();
            for e in start..=max {
                if !have.contains(&e) {
                    missing.push(e);
                }
            }
        }
        if (1..=10).contains(&missing.len()) {
            let sig = missing
                .iter()
                .map(|e| crate::models::fmt_episode(*e as u32))
                .collect::<Vec<_>>()
                .join(",");
            if sub.gap_notified.as_deref() != Some(sig.as_str()) {
                let missing_s = missing
                    .iter()
                    .map(|e| crate::models::fmt_episode(*e as u32))
                    .collect::<Vec<_>>()
                    .join(" ");
                let have_max = pushed.iter().max().unwrap_or(&0);
                inform(
                    state,
                    &format!(
                        "🔍 <b>{}</b> 遗漏检测: 第 {} 话尚未推送 (已推到 {})",
                        notifier::html_escape(&sub.title),
                        missing_s,
                        crate::models::fmt_episode(*have_max as u32)
                    ),
                )
                .await?;
                db::set_sub_gap_notified(&state.db, sub.id, &sig)?;
            }
        }
    }

    // 摸鱼检测：超过 N 天无新推送 → 通知
    let slack_days = db::meta_int(&state.db, "slack_days", 0);
    if slack_days > 0 {
        if let Some(last_push) = &sub.last_push_at {
            let since = days_since(last_push);
            let already_notified = sub
                .last_slack_notified
                .as_ref()
                .map(|t| days_since(t) < 1)
                .unwrap_or(false);
            if since >= slack_days && !already_notified {
                inform(
                    state,
                    &format!(
                        "🥱 <b>{}</b> 已 {since} 天没有更新，字幕组可能摸鱼了（建议检查 RSS / 开启自动停用）",
                        notifier::html_escape(&sub.title)
                    ),
                )
                .await?;
                db::set_sub_last_slack_notified(&state.db, sub.id)?;
            }
        }
    }

    Ok(())
}

fn days_since(ts: &str) -> i64 {
    chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S")
        .map(|t| {
            let now = chrono::Local::now().naive_local();
            (now - t).num_days()
        })
        .unwrap_or(0)
}

/// 拉取 → 解析 → 过滤 → 查重 → 推送或询问
pub async fn process_subscription(state: &Arc<AppState>, sub: &SubRow) -> Result<ProcessReport> {
    let skip_half = db::meta_bool(&state.db, "skip_half", false);

    let main_items = match fetch_rss(&state.http, &sub.rss_url).await {
        Ok(items) => items,
        Err(e) => {
            tracing::warn!("sub#{} 主 RSS 拉取失败: {e}", sub.id);
            Vec::new()
        }
    };
    let mut candidates = collect_candidates(state, sub, &main_items, skip_half).await?;

    // 备用 RSS：主源没产出任何新候选时兜底
    if candidates.is_empty() && !sub.backup_rss_url.is_empty() {
        match fetch_rss(&state.http, &sub.backup_rss_url).await {
            Ok(items) => {
                candidates = collect_candidates(state, sub, &items, skip_half).await?;
                if !candidates.is_empty() {
                    tracing::info!("sub#{} 主源无更新，使用备用 RSS", sub.id);
                }
            }
            Err(e) => tracing::warn!("sub#{} 备用 RSS 拉取失败: {e}", sub.id),
        }
    }

    let _ = db::touch_fetch(&state.db, sub.id);

    let report = ProcessReport {
        new: candidates.len(),
        ..Default::default()
    };

    let mut groups: BTreeMap<u32, Vec<Candidate>> = BTreeMap::new();
    for c in candidates {
        groups.entry(c.episode).or_default().push(c);
    }

    let mut report = report;
    for (ep, cands) in groups {
        if db::pending_exists(&state.db, sub.id, ep as i64)? {
            continue;
        }

        let already = db::pushed_for_episode(&state.db, sub.id, ep as i64)?;
        if !already.is_empty() {
            // 已推送过的集数出现新版本/新语言 → 询问追加
            // （若该集已做过决定：忽略/已追加，则不再打扰）
            if db::get_episode_pref(&state.db, sub.id, ep as i64)?.is_some() {
                continue;
            }
            ask_update(state, sub, ep, &cands).await?;
            report.asked += 1;
            continue;
        }

        match decide_fresh(state, sub, ep, &cands)? {
            Decision::Push(list) => {
                for c in list {
                    push_candidate(state, sub, &c).await?;
                    report.pushed += 1;
                }
            }
            Decision::Ask => {
                ask_fresh(state, sub, ep, &cands).await?;
                report.asked += 1;
            }
            Decision::Skip => {}
        }
    }

    Ok(report)
}

pub(crate) async fn collect_candidates(
    state: &AppState,
    sub: &SubRow,
    items: &[crate::models::RssItem],
    skip_half: bool,
) -> Result<Vec<Candidate>> {
    let mut candidates: Vec<Candidate> = Vec::new();
    for item in items {
        let p = parse_title(&item.title);
        let Some(ep) = p.episode else { continue };
        if p.is_collection || p.is_special {
            continue;
        }
        if p.is_half && skip_half {
            continue;
        }
        if (ep as i64) < sub.start_episode {
            continue;
        }
        if !crate::filter::matches_keywords(&item.title, &sub.include_kw, &sub.exclude_kw) {
            continue;
        }
        // 已按条目标识推过（避免重复解析 .torrent）→ 跳过
        if db::is_pushed_link(&state.db, &item.link)? {
            continue;
        }
        // 该集正在等待用户选择 → 不重复解析/打扰
        if db::pending_exists(&state.db, sub.id, ep as i64)? {
            continue;
        }
        let Some(magnet) = crate::rss::resolve_magnet(&state.http, item).await else {
            tracing::debug!("sub#{} 无法解析磁力: {}", sub.id, item.title);
            continue;
        };
        if db::is_pushed(&state.db, &magnet)? {
            continue;
        }
        let pushed = db::pushed_for_episode(&state.db, sub.id, ep as i64)?;
        if pushed
            .iter()
            .any(|(v, l)| *v == p.version as i64 && l == p.lang.label())
        {
            continue;
        }
        candidates.push(Candidate {
            title: item.title.clone(),
            magnet,
            fansub: p.fansub,
            episode: ep,
            version: p.version,
            lang: p.lang.label().to_string(),
            quality: p.quality,
            codec: p.codec,
            source: p.source,
            link: item.link.clone(),
        });
    }
    Ok(candidates)
}

pub(crate) fn decide_fresh(state: &AppState, sub: &SubRow, ep: u32, cands: &[Candidate]) -> Result<Decision> {
    // 1) 该集已记住的偏好
    if let Some((lang, ver)) = db::get_episode_pref(&state.db, sub.id, ep as i64)? {
        if let Some(v) = ver {
            if let Some(c) = cands.iter().find(|c| c.version as i64 == v) {
                return Ok(Decision::Push(vec![c.clone()]));
            }
            // 偏好版本已下架 → 同语言里取最高版本
            let idxs = filter_by_lang(cands, &lang);
            if let Some(&i) = idxs.iter().max_by_key(|&&i| cands[i].version) {
                return Ok(Decision::Push(vec![cands[i].clone()]));
            }
            return Ok(Decision::Skip);
        }
        // 该集用户选择过"忽略"
        return Ok(Decision::Skip);
    }

    // 2) 订阅级简繁规则
    if matches!(sub.lang_pref.as_str(), "简中" | "繁中" | "简繁") {
        let idxs = filter_by_lang(cands, &sub.lang_pref);
        if idxs.is_empty() {
            return Ok(Decision::Skip);
        }
        if idxs.len() == 1 {
            return Ok(Decision::Push(vec![cands[idxs[0]].clone()]));
        }
        // 同语言多个版本 → 问
        return Ok(Decision::Ask);
    }

    // 3) 单候选直接推；多候选问
    if cands.len() == 1 {
        return Ok(Decision::Push(vec![cands[0].clone()]));
    }
    Ok(Decision::Ask)
}

async fn push_candidate(state: &AppState, sub: &SubRow, c: &Candidate) -> Result<()> {
    let msg = notifier::format_push(sub, c);
    match crate::resolve_channel(state) {
        Some(ch) => {
            state
                .bot
                .send_message(ChatId(ch), msg)
                .parse_mode(ParseMode::Html)
                .await?;
            tracing::info!("推送 sub#{} 第{}话 -> 频道 {ch}", sub.id, fmt_episode(c.episode));
        }
        None => match crate::resolve_admin(state) {
            Some(a) => {
                let warn = format!("⚠️ 未绑定频道，磁力发到管理员会话\n\n{msg}");
                state
                    .bot
                    .send_message(ChatId(a), warn)
                    .parse_mode(ParseMode::Html)
                    .await?;
            }
            None => tracing::warn!("无管理员无频道，丢弃磁力: {}", c.magnet),
        },
    }
    let _ = db::insert_pushed(
        &state.db,
        sub.id,
        c.episode as i64,
        c.version as i64,
        &c.lang,
        &c.magnet,
        &c.title,
        &c.link,
    );
    let _ = db::set_sub_last_push(&state.db, sub.id);
    Ok(())
}

async fn ask_fresh(state: &AppState, sub: &SubRow, ep: u32, cands: &[Candidate]) -> Result<()> {
    let json = serde_json::to_string(cands)?;
    let pid = db::save_pending(&state.db, sub.id, ep as i64, "fresh", &json)?;

    let lines: Vec<String> = cands
        .iter()
        .enumerate()
        .map(|(i, c)| notifier::candidate_line(c, i))
        .collect();
    let text = format!(
        "🧐 <b>{}</b> 第{}话 发现多个来源/版本\n{}\n\n要推送哪个？",
        notifier::html_escape(&sub.title),
        fmt_episode(ep),
        lines.join("\n"),
    );

    let mut rows = vec![(0..cands.len().min(6))
        .map(|i| InlineKeyboardButton::callback((i + 1).to_string(), format!("pick:{pid}:{i}")))
        .collect::<Vec<_>>()];
    rows.push(vec![
        InlineKeyboardButton::callback("全部", format!("pickall:{pid}")),
        InlineKeyboardButton::callback("跳过", format!("skip:{pid}")),
        InlineKeyboardButton::callback("稍后", format!("later:{pid}")),
    ]);

    send_to_admin(state, text, InlineKeyboardMarkup::new(rows)).await
}

async fn ask_update(state: &AppState, sub: &SubRow, ep: u32, cands: &[Candidate]) -> Result<()> {
    let json = serde_json::to_string(cands)?;
    let pid = db::save_pending(&state.db, sub.id, ep as i64, "update", &json)?;

    let lines: Vec<String> = cands
        .iter()
        .enumerate()
        .map(|(i, c)| notifier::candidate_line(c, i))
        .collect();
    let text = format!(
        "♻️ <b>{}</b> 第{}话 发现新版本\n{}\n\n追加推送？",
        notifier::html_escape(&sub.title),
        fmt_episode(ep),
        lines.join("\n"),
    );

    let mut rows = vec![(0..cands.len().min(6))
        .map(|i| InlineKeyboardButton::callback((i + 1).to_string(), format!("add:{pid}:{i}")))
        .collect::<Vec<_>>()];
    rows.push(vec![
        InlineKeyboardButton::callback("忽略", format!("noadd:{pid}")),
        InlineKeyboardButton::callback("稍后", format!("later:{pid}")),
    ]);

    send_to_admin(state, text, InlineKeyboardMarkup::new(rows)).await
}

/// 处理冲突询问按钮回调（pick / pickall / skip / later / add / noadd）
pub async fn handle_decision(_bot: &Bot, state: &Arc<AppState>, data: &str) -> Result<()> {
    let parts: Vec<&str> = data.splitn(3, ':').collect();
    let cmd = parts[0];
    let pid: i64 = parts
        .get(1)
        .and_then(|s| s.parse().ok())
        .context("pid 无效")?;
    let pending = db::get_pending(&state.db, pid)?.context("决策不存在")?;
    let cands: Vec<Candidate> =
        serde_json::from_str(&pending.candidates_json).context("候选数据损坏")?;
    let sub = db::get_subscription(&state.db, pending.subscription_id)?.context("订阅不存在")?;

    match cmd {
        "pick" => {
            let idx: usize = parts.get(2).and_then(|s| s.parse().ok()).context("idx 无效")?;
            let c = cands.get(idx).context("idx 越界")?.clone();
            db::save_episode_pref(&state.db, sub.id, pending.episode, &c.lang, Some(c.version as i64))?;
            learn_lang(state, &sub, &c.lang);
            db::delete_pending(&state.db, pid)?;
            push_candidate(state, &sub, &c).await?;
            confirm_pushed(state, &sub, &c, "已推送").await?;
        }
        "pickall" => {
            db::delete_pending(&state.db, pid)?;
            for c in &cands {
                push_candidate(state, &sub, c).await?;
            }
            inform(
                state,
                &format!("✅ 已全部推送 {} 第{}话 ({} 个)", sub.title, fmt_episode(pending.episode as u32), cands.len()),
            )
            .await?;
        }
        "skip" => {
            db::save_episode_pref(&state.db, sub.id, pending.episode, "", None)?;
            db::delete_pending(&state.db, pid)?;
            inform(
                state,
                &format!("⏭️ 已跳过 {} 第{}话", sub.title, fmt_episode(pending.episode as u32)),
            )
            .await?;
        }
        "later" => {
            db::delete_pending(&state.db, pid)?;
            inform(state, "⏳ 下次拉取会再问").await?;
        }
        "add" => {
            let idx: usize = parts.get(2).and_then(|s| s.parse().ok()).context("idx 无效")?;
            let c = cands.get(idx).context("idx 越界")?.clone();
            db::save_episode_pref(&state.db, sub.id, pending.episode, &c.lang, Some(c.version as i64))?;
            db::delete_pending(&state.db, pid)?;
            push_candidate(state, &sub, &c).await?;
            confirm_pushed(state, &sub, &c, "已追加").await?;
        }
        "noadd" => {
            db::save_episode_pref(&state.db, sub.id, pending.episode, "", None)?;
            db::delete_pending(&state.db, pid)?;
            inform(
                state,
                &format!("🚫 已忽略 {} 第{}话的新版本", sub.title, fmt_episode(pending.episode as u32)),
            )
            .await?;
        }
        _ => {}
    }
    Ok(())
}

fn learn_lang(state: &AppState, sub: &SubRow, lang: &str) {
    if lang.is_empty() || lang == "未知" {
        return;
    }
    if sub.lang_pref.is_empty() || sub.lang_pref == "ask" {
        let _ = db::set_sub_lang(&state.db, sub.id, lang);
    }
}

fn candidate_attrs(c: &Candidate) -> String {
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
    parts.join(" · ")
}

async fn confirm_pushed(state: &AppState, sub: &SubRow, c: &Candidate, verb: &str) -> Result<()> {
    inform(
        state,
        &format!(
            "✅ {verb} {} 第{}话 ({})",
            notifier::html_escape(&sub.title),
            fmt_episode(c.episode),
            candidate_attrs(c)
        ),
    )
    .await
}

async fn inform(state: &AppState, text: &str) -> Result<()> {
    if let Some(a) = crate::resolve_admin(state) {
        state
            .bot
            .send_message(ChatId(a), text.to_string())
            .parse_mode(ParseMode::Html)
            .await?;
    }
    Ok(())
}

async fn send_to_admin(state: &AppState, text: String, kb: InlineKeyboardMarkup) -> Result<()> {
    let admin = crate::resolve_admin(state).context("未绑定管理员")?;
    state
        .bot
        .send_message(ChatId(admin), text)
        .parse_mode(ParseMode::Html)
        .reply_markup(kb)
        .await?;
    Ok(())
}

async fn notify_error(state: &AppState, title: &str, err: &str) {
    let msg = format!(
        "⚠️ <b>{}</b> 拉取失败: {}",
        notifier::html_escape(title),
        notifier::html_escape(err)
    );
    let _ = inform(state, &msg).await;
}
