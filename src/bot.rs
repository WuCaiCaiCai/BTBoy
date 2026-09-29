use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::json;
use teloxide::net::Download;
use teloxide::prelude::*;
use teloxide::types::{
    CallbackQuery, ChatId, InlineKeyboardButton, InlineKeyboardMarkup, InputFile, Message,
    ParseMode,
};

use crate::db;
use crate::models::SubRow;
use crate::notifier::html_escape;
use crate::AppState;

pub async fn handle_message(bot: Bot, msg: Message, state: Arc<AppState>) -> ResponseResult<()> {
    let chat_id = msg.chat.id.0;
    let admin = crate::resolve_admin(&state);

    if let Some(text) = msg.text() {
        if let Some((cmd, args)) = parse_command(text) {
            let allowed = admin.map(|a| a == chat_id).unwrap_or(false);
            if !allowed && cmd != "start" && cmd != "help" && cmd != "admin" {
                return Ok(());
            }
            let _ = dispatch_command(&bot, &state, &msg, chat_id, admin, &cmd, &args).await;
            return Ok(());
        }
        let _ = handle_conversation_text(&bot, &state, &msg, chat_id, admin).await;
        return Ok(());
    }

    let _ = handle_conversation_text(&bot, &state, &msg, chat_id, admin).await;
    Ok(())
}

fn parse_command(text: &str) -> Option<(String, Vec<String>)> {
    let t = text.trim().strip_prefix('/')?;
    let mut it = t.split_whitespace();
    let mut cmd = it.next()?.to_lowercase();
    if let Some(i) = cmd.find('@') {
        cmd.truncate(i);
    }
    let args: Vec<String> = it.map(|s| s.to_string()).collect();
    Some((cmd, args))
}

fn bool_str(b: bool) -> &'static str {
    if b {
        "开"
    } else {
        "关"
    }
}

/// 用户输入的"序号"→ 真实订阅id（列表按 id 升序，1-based，删了会重排）
fn resolve_idx(state: &Arc<AppState>, idx: i64) -> Result<Option<i64>> {
    db::resolve_sub_by_index(&state.db, idx)
}

/// /start 的按钮主菜单
fn menu_kb() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("📺 订阅列表", "menu:list"),
            InlineKeyboardButton::callback("⚡ 立即拉取", "menu:push"),
        ],
        vec![
            InlineKeyboardButton::callback("📊 状态", "menu:status"),
            InlineKeyboardButton::callback("🕘 推送历史", "menu:history"),
        ],
        vec![
            InlineKeyboardButton::callback("⏳ 待选择项", "menu:pending"),
            InlineKeyboardButton::callback("❓ 帮助", "menu:help"),
        ],
    ])
}

async fn handle_menu(bot: &Bot, state: &Arc<AppState>, uid: i64, action: &str) -> Result<()> {
    match action {
        "list" => send_sub_list(bot, state, uid).await?,
        "push" => show_push_dialog(bot, state, uid).await?,
        "status" => send_status(bot, state, uid, Some(uid)).await?,
        "history" => send_history(bot, state, uid, None, 15).await?,
        "pending" => send_pending(bot, state, uid).await?,
        "help" => send(bot, uid, help_text(), None).await?,
        _ => send(bot, uid, "未知操作，发送 /help 查看帮助", None).await?,
    }
    Ok(())
}

/// 推送历史（/history 与菜单共用）
async fn send_history(
    bot: &Bot,
    state: &Arc<AppState>,
    chat_id: i64,
    idx: Option<i64>,
    limit: i64,
) -> Result<()> {
    let sid = match idx {
        Some(i) => match resolve_idx(state, i)? {
            Some(id) => Some(id),
            None => {
                send(bot, chat_id, format!("未找到订阅 #{i}"), None).await?;
                return Ok(());
            }
        },
        None => None,
    };
    let rows = db::list_pushed(&state.db, sid, limit)?;
    if rows.is_empty() {
        send(bot, chat_id, "📭 暂无推送记录", None).await?;
        return Ok(());
    }
    let lines: Vec<String> = rows
        .iter()
        .map(|r| {
            let ver = if r.version > 1 {
                format!(" · v{}", r.version)
            } else {
                String::new()
            };
            let lang = if r.lang.is_empty() || r.lang == "未知" {
                String::new()
            } else {
                format!(" · {}", r.lang)
            };
            let title: String = r.title.chars().take(30).collect();
            format!(
                "{} 第{}{}{} · {}",
                html_escape(&title),
                fmt_episode_i(r.episode),
                ver,
                lang,
                r.pushed_at
            )
        })
        .collect();
    send(
        bot,
        chat_id,
        format!(
            "🕘 <b>推送历史</b>（最近 {} 条）\n{}",
            rows.len(),
            lines.join("\n")
        ),
        None,
    )
    .await?;
    Ok(())
}

/// 待选择项列表（/pending 与菜单共用）
async fn send_pending(bot: &Bot, state: &Arc<AppState>, chat_id: i64) -> Result<()> {
    let rows = db::list_pending_with_title(&state.db)?;
    if rows.is_empty() {
        send(bot, chat_id, "✅ 没有待选择项", None).await?;
        return Ok(());
    }
    let mut kb_rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    for (i, (pid, ep, kind, title)) in rows.iter().enumerate() {
        let kind_s = if kind == "update" { "新版本" } else { "多来源" };
        lines.push(format!(
            "{}. <b>{}</b> 第{}话 · {}",
            i + 1,
            html_escape(title),
            fmt_episode_i(*ep),
            kind_s
        ));
        kb_rows.push(vec![InlineKeyboardButton::callback(
            format!("重发 {}", i + 1),
            format!("resend:{pid}"),
        )]);
    }
    send(
        bot,
        chat_id,
        format!(
            "⏳ 待选择项（{} 个）：\n{}\n点对应按钮重发询问",
            rows.len(),
            lines.join("\n")
        ),
        Some(InlineKeyboardMarkup::new(kb_rows)),
    )
    .await?;
    Ok(())
}

/// 订阅列表（/list 与菜单共用）
async fn send_sub_list(bot: &Bot, state: &Arc<AppState>, chat_id: i64) -> Result<()> {
    let subs = db::list_subscriptions(&state.db)?;
    if subs.is_empty() {
        send(bot, chat_id, "📭 还没有订阅，用 /sub <RSS链接> 添加", None).await?;
        return Ok(());
    }
    let mut lines = Vec::new();
    for (i, s) in subs.into_iter().enumerate() {
        let flag = if s.enabled { "🟢" } else { "⏸️" };
        let ep = fmt_episode_i(s.start_episode);
        let lang = if s.lang_pref.is_empty() || s.lang_pref == "ask" {
            "ask".to_string()
        } else {
            s.lang_pref.clone()
        };
        let extra = if !s.backup_rss_url.is_empty() {
            " 📦备用"
        } else {
            ""
        };
        let pushed = db::count_pushed_for_sub(&state.db, s.id).unwrap_or(0);
        let pending = db::pending_count_for_sub(&state.db, s.id).unwrap_or(0);
        let pend = if pending > 0 {
            format!(" · ⏳{pending}")
        } else {
            String::new()
        };
        lines.push(format!(
            "{flag} <b>#{}</b> {} · 从{ep}话起 · {lang} · 已推{pushed}集{pend}{extra}\n    {}",
            i + 1,
            html_escape(&s.title),
            html_escape(&s.rss_url)
        ));
    }
    send(bot, chat_id, lines.join("\n\n"), None).await?;
    Ok(())
}

/// 状态（/status 与菜单共用）
async fn send_status(
    bot: &Bot,
    state: &Arc<AppState>,
    chat_id: i64,
    admin: Option<i64>,
) -> Result<()> {
    let nsub = db::count_subscriptions(&state.db)?;
    let npush = db::count_pushed(&state.db)?;
    let ch = crate::resolve_channel(state)
        .map(|c| c.to_string())
        .unwrap_or_else(|| "未绑定".into());
    let admin_id = admin.map(|a| a.to_string()).unwrap_or_else(|| "未设置".into());
    let uptime = state.started.elapsed().as_secs();
    let rss_on = db::meta_bool(&state.db, "rss_enabled", true);
    let interval = db::meta_int(
        &state.db,
        "fetch_interval_min",
        state.config.fetch_interval_min as i64,
    );
    let skip_half = db::meta_bool(&state.db, "skip_half", false);
    let gap = db::meta_bool(&state.db, "gap_detect", false);
    let slack = db::meta_int(&state.db, "slack_days", 0);
    let autodisable = db::meta_bool(&state.db, "autodisable", false);

    // 轮询节奏：上次轮询时间 → 下次预计
    let poll_line = match db::meta_get(&state.db, "last_cycle_at") {
        Some(last) if !last.is_empty() => {
            let next = chrono::NaiveDateTime::parse_from_str(&last, "%Y-%m-%d %H:%M:%S")
                .map(|t| {
                    let next = t + chrono::Duration::minutes(interval);
                    let now = chrono::Local::now().naive_local();
                    let mins = (next - now).num_minutes();
                    if mins > 0 {
                        format!("下次约 {mins} 分钟后")
                    } else {
                        "即将轮询".to_string()
                    }
                })
                .unwrap_or_default();
            format!("上次轮询: {last} · {next}")
        }
        _ => "尚未轮询".to_string(),
    };

    let text = format!(
        "📊 <b>BTBoy 状态</b>\n\
         管理员: {admin_id}\n推送频道: {ch}\n\
         订阅数: {nsub} · 已推送: {npush}\n\
         运行时长: {uptime}s\n\
         轮询: {poll_line}\n\
         ── 设置 ──\n\
         RSS 总开关: {} · 轮询间隔: {interval} 分钟\n\
         跳过 .5 集: {} · 遗漏检测: {} · 摸鱼检测: {} 天 · 自动停用: {}",
        bool_str(rss_on),
        bool_str(skip_half),
        bool_str(gap),
        slack,
        bool_str(autodisable)
    );
    send(bot, chat_id, text, None).await?;
    Ok(())
}

async fn dispatch_command(
    bot: &Bot,
    state: &Arc<AppState>,
    msg: &Message,
    chat_id: i64,
    admin: Option<i64>,
    cmd: &str,
    args: &[String],
) -> Result<()> {
    let _ = msg;
    match cmd {
        "start" => {
            send(
                bot,
                chat_id,
                "🤖 <b>BTBoy 自动追番</b>\n选择要进行的操作（详细命令见 /help）：",
                Some(menu_kb()),
            )
            .await?
        }
        "help" => send(bot, chat_id, help_text(), None).await?,

        "admin" => {
            if admin.is_none() {
                db::meta_set(&state.db, "admin_id", &chat_id.to_string())?;
                send(bot, chat_id, format!("✅ 你已成为管理员 (id={chat_id})"), None).await?;
            } else if admin == Some(chat_id) {
                if let Some(id) = args.first().and_then(|s| s.parse::<i64>().ok()) {
                    db::meta_set(&state.db, "admin_id", &id.to_string())?;
                    send(bot, chat_id, format!("✅ 管理员已设为 {id}"), None).await?;
                } else {
                    send(bot, chat_id, format!("管理员已是 {chat_id}"), None).await?;
                }
            }
        }

        "bind" => {
            if let Some(id) = args.first().and_then(|s| s.parse::<i64>().ok()) {
                db::meta_set(&state.db, "channel_id", &id.to_string())?;
                send(bot, chat_id, format!("✅ 已绑定频道 {id}"), None).await?;
            } else {
                db::conv_set(&state.db, chat_id, r#"{"step":"await_channel"}"#)?;
                send(
                    bot,
                    chat_id,
                    "📣 绑定推送频道（机器人需已是该频道管理员）：\n\
                     ① <b>转发一条</b>来自该频道的消息给我\n\
                     ② 或直接<b>输入频道ID</b>\n\
                     /cancel 取消",
                    None,
                )
                .await?;
            }
        }

        "sub" => {
            if let Some(url) = args.first() {
                if url.starts_with("http") {
                    start_sub_flow(bot, state, chat_id, url).await?;
                } else {
                    send(
                        bot,
                        chat_id,
                        "❌ 链接格式不对，请输入以 http 开头的 RSS 链接",
                        None,
                    )
                    .await?;
                }
            } else {
                db::conv_set(&state.db, chat_id, r#"{"step":"await_sub_url"}"#)?;
                send(
                    bot,
                    chat_id,
                    "🔗 请发送你想订阅的 RSS 链接（/cancel 取消）",
                    None,
                )
                .await?;
            }
        }

        "list" => send_sub_list(bot, state, chat_id).await?,

        "show" => {
            if let Some(idx) = args.first().and_then(|s| s.parse::<i64>().ok()) {
                match resolve_idx(state, idx)? {
                    Some(id) => {
                        if let Some(s) = db::get_subscription(&state.db, id)? {
                            send(bot, chat_id, sub_detail(state, &s), None).await?;
                        }
                    }
                    None => send(bot, chat_id, format!("未找到订阅 #{idx}"), None).await?,
                }
            } else {
                show_sub_picker(bot, state, chat_id, "show").await?;
            }
        }

        "edit" => {
            if let Some(idx) = args.first().and_then(|s| s.parse::<i64>().ok()) {
                match resolve_idx(state, idx)? {
                    Some(id) => {
                        let kb = edit_kb(id);
                        send(bot, chat_id, format!("编辑订阅 <b>#{idx}</b>"), Some(kb)).await?;
                    }
                    None => send(bot, chat_id, format!("未找到订阅 #{idx}"), None).await?,
                }
            } else {
                show_sub_picker(bot, state, chat_id, "edit").await?;
            }
        }

        "del" => {
            if let Some(idx) = args.first().and_then(|s| s.parse::<i64>().ok()) {
                match resolve_idx(state, idx)? {
                    Some(id) => {
                        if let Some(s) = db::get_subscription(&state.db, id)? {
                            let kb = InlineKeyboardMarkup::new(vec![vec![
                                InlineKeyboardButton::callback("确认删除", format!("delc:{id}:yes")),
                                InlineKeyboardButton::callback("取消", format!("delc:{id}:no")),
                            ]]);
                            send(
                                bot,
                                chat_id,
                                format!("确认删除订阅 <b>{}</b> (#{idx})?", html_escape(&s.title)),
                                Some(kb),
                            )
                            .await?;
                        }
                    }
                    None => send(bot, chat_id, format!("未找到订阅 #{idx}"), None).await?,
                }
            } else {
                show_sub_picker(bot, state, chat_id, "del").await?;
            }
        }

        "push" => {
            if let Some(idx) = args.first().and_then(|s| s.parse::<i64>().ok()) {
                match resolve_idx(state, idx)? {
                    Some(id) => {
                        if let Some(s) = db::get_subscription(&state.db, id)? {
                            match crate::scheduler::process_subscription(state, &s, false).await {
                                Ok(r) => send(bot, chat_id, push_result_text(idx, &r), None).await?,
                                Err(e) => {
                                    send(bot, chat_id, format!("❌ #{idx} 拉取失败: {e}"), None)
                                        .await?
                                }
                            }
                        }
                    }
                    None => send(bot, chat_id, format!("未找到订阅 #{idx}"), None).await?,
                }
            } else {
                show_push_dialog(bot, state, chat_id).await?;
            }
        }

        "history" => {
            let idx = args.first().and_then(|s| s.parse::<i64>().ok());
            let limit = args
                .get(1)
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(15);
            send_history(bot, state, chat_id, idx, limit).await?
        }

        "pending" => send_pending(bot, state, chat_id).await?,

        "export" => {
            let subs = db::list_subscriptions(&state.db)?;
            if subs.is_empty() {
                send(bot, chat_id, "📭 还没有订阅，无法导出", None).await?;
            } else {
                let arr: Vec<serde_json::Value> = subs
                    .iter()
                    .map(|s| {
                        json!({
                            "rss_url": s.rss_url,
                            "title": s.title,
                            "start_episode": s.start_episode,
                            "lang_pref": s.lang_pref,
                            "include_kw": s.include_kw,
                            "exclude_kw": s.exclude_kw,
                            "backup_rss_url": s.backup_rss_url,
                            "total_episodes": s.total_episodes,
                            "bgm_id": s.bgm_id,
                            "enabled": s.enabled,
                            "poster_url": s.poster_url,
                        })
                    })
                    .collect();
                let content = serde_json::to_string_pretty(&arr)?;
                let file = InputFile::memory(content.into_bytes())
                    .file_name("btboy_subscriptions.json");
                bot.send_document(ChatId(chat_id), file)
                    .caption(format!("📦 订阅备份（{} 个），换机后用 /import 恢复", arr.len()))
                    .await?;
            }
        }

        "import" => {
            db::conv_set(&state.db, chat_id, r#"{"step":"await_import"}"#)?;
            send(
                bot,
                chat_id,
                "📥 把 /export 的 JSON 内容粘贴给我，或直接发送 .json 文件（/cancel 取消）",
                None,
            )
            .await?;
        }

        "test" => {
            match crate::resolve_channel(state) {
                Some(ch) => {
                    let sub = db::list_subscriptions(&state.db)?.into_iter().next();
                    match sub {
                        Some(s) => {
                            let c = crate::models::Candidate {
                                title: format!("{} 测试", s.title),
                                magnet: "magnet:?xt=urn:btih:TESTMAGNET".into(),
                                fansub: None,
                                episode: 1,
                                version: 1,
                                lang: "简中".into(),
                                quality: Some("1080P".into()),
                                codec: None,
                                source: None,
                                link: String::new(),
                            };
                            state
                                .bot
                                .send_message(ChatId(ch), crate::notifier::format_push(&s, &c))
                                .parse_mode(ParseMode::Html)
                                .await?;
                            send(bot, chat_id, "✅ 测试消息已发送到频道", None).await?;
                        }
                        None => send(
                            bot,
                            chat_id,
                            "没有订阅可做测试，先 /sub 添加一个",
                            None,
                        )
                        .await?,
                    }
                }
                None => send(bot, chat_id, "⚠️ 尚未绑定频道，用 /bind 绑定", None).await?,
            }
        }

        "status" => send_status(bot, state, chat_id, admin).await?,

        "logs" => {
            let n = args.first()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(30)
                .min(200);
            let text = crate::logging::tail_logs(n);
            send(
                bot,
                chat_id,
                format!(
                    "<pre>{}</pre>",
                    html_escape(&text).chars().take(4000).collect::<String>()
                ),
                None,
            )
            .await?;
        }

        "cancel" => {
            db::conv_clear(&state.db, chat_id)?;
            send(bot, chat_id, "已取消当前操作", None).await?;
        }

        // ---- 全局开关 / 参数 ----
        "rss" => {
            handle_switch(
                bot,
                state,
                chat_id,
                "rss_enabled",
                "RSS 轮询总开关",
                args.first().map(|s| s.as_str()),
                "/rss on|off",
            )
            .await?;
        }

        "interval" => {
            if let Some(n) = args
                .first()
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|n| (1..=1440).contains(n))
            {
                db::meta_set(&state.db, "fetch_interval_min", &n.to_string())?;
                send(bot, chat_id, format!("✅ 轮询间隔设为 {n} 分钟"), None).await?;
            } else {
                start_flow(bot, state, chat_id, "interval").await?;
            }
        }

        "skiphalf" => {
            handle_switch(
                bot,
                state,
                chat_id,
                "skip_half",
                "跳过 .5 特殊集",
                args.first().map(|s| s.as_str()),
                "/skiphalf on|off",
            )
            .await?;
        }

        "gap" => {
            handle_switch(
                bot,
                state,
                chat_id,
                "gap_detect",
                "遗漏检测通知",
                args.first().map(|s| s.as_str()),
                "/gap on|off",
            )
            .await?;
        }

        "slack" => {
            if let Some(raw) = args.first() {
                let days = match raw.as_str() {
                    "off" | "0" => 0,
                    s => s.parse::<i64>().unwrap_or(0),
                };
                db::meta_set(&state.db, "slack_days", &days.to_string())?;
                if days > 0 {
                    send(bot, chat_id, format!("✅ 摸鱼检测: {days} 天没更新会通知"), None).await?;
                } else {
                    send(bot, chat_id, "✅ 摸鱼检测已关闭", None).await?;
                }
            } else {
                start_flow(bot, state, chat_id, "slack").await?;
            }
        }

        "autodisable" => {
            handle_switch(
                bot,
                state,
                chat_id,
                "autodisable",
                "全部集数推送后自动停用订阅",
                args.first().map(|s| s.as_str()),
                "/autodisable on|off",
            )
            .await?;
        }

        // ---- 订阅级 ----
        "total" => {
            let idx = args.first().and_then(|s| s.parse::<i64>().ok());
            let n = args.get(1).and_then(|s| s.parse::<i64>().ok());
            match (idx, n) {
                (Some(idx), Some(n)) => match resolve_idx(state, idx)? {
                    Some(id) => {
                        db::set_sub_total(&state.db, id, Some(n))?;
                        send(bot, chat_id, format!("✅ #{idx} 总集数设为 {n}"), None).await?;
                    }
                    None => send(bot, chat_id, format!("未找到订阅 #{idx}"), None).await?,
                },
                _ => show_sub_picker(bot, state, chat_id, "total").await?,
            }
        }

        "bgm" => {
            let idx = args.first().and_then(|s| s.parse::<i64>().ok());
            let bgm = args.get(1).and_then(|s| s.parse::<i64>().ok());
            match (idx, bgm) {
                (Some(idx), Some(bgm)) => match resolve_idx(state, idx)? {
                    Some(id) => {
                        db::set_sub_bgm(&state.db, id, Some(bgm))?;
                        match crate::scheduler::fetch_bgm_total(&state.http, bgm).await {
                            Ok(Some(total)) => {
                                db::set_sub_total(&state.db, id, Some(total))?;
                                send(bot, chat_id, format!("✅ #{idx} 绑定 BGM {bgm}，总集数 {total}"), None).await?;
                            }
                            _ => send(bot, chat_id, format!("✅ #{idx} 已绑定 BGM {bgm}（暂时取不到总集数，稍后会自动刷新）"), None).await?,
                        }
                    }
                    None => send(bot, chat_id, format!("未找到订阅 #{idx}"), None).await?,
                },
                _ => show_sub_picker(bot, state, chat_id, "bgm").await?,
            }
        }

        "backup" => {
            let idx = args.first().and_then(|s| s.parse::<i64>().ok());
            let url = args.get(1).map(|s| s.as_str());
            match (idx, url) {
                (Some(idx), Some(url)) if url.starts_with("http") => match resolve_idx(state, idx)? {
                    Some(id) => {
                        db::set_sub_backup(&state.db, id, Some(url))?;
                        send(bot, chat_id, format!("✅ #{idx} 备用 RSS 已设置"), None).await?;
                    }
                    None => send(bot, chat_id, format!("未找到订阅 #{idx}"), None).await?,
                },
                _ => show_sub_picker(bot, state, chat_id, "backup").await?,
            }
        }

        "rmbackup" => {
            if let Some(idx) = args.first().and_then(|s| s.parse::<i64>().ok()) {
                match resolve_idx(state, idx)? {
                    Some(id) => {
                        db::set_sub_backup(&state.db, id, None)?;
                        send(bot, chat_id, format!("✅ #{idx} 备用 RSS 已移除"), None).await?;
                    }
                    None => send(bot, chat_id, format!("未找到订阅 #{idx}"), None).await?,
                }
            } else {
                show_sub_picker(bot, state, chat_id, "rmbackup").await?;
            }
        }

        _ => send(bot, chat_id, "未知命令，发送 /help 查看帮助", None).await?,
    }
    Ok(())
}

/// on/off 解析；无法识别返回 None（不再默认开启，避免误操作）
fn parse_on_off(s: &str) -> Option<bool> {
    match s {
        "on" | "开" | "1" => Some(true),
        "off" | "关" | "0" => Some(false),
        _ => None,
    }
}

/// 处理 on/off 开关：识别则写 meta 并回复，无法识别则回用法提示
async fn handle_switch(
    bot: &Bot,
    state: &Arc<AppState>,
    chat_id: i64,
    key: &str,
    label: &str,
    arg: Option<&str>,
    usage: &str,
) -> Result<()> {
    match parse_on_off(arg.unwrap_or("")) {
        Some(on) => {
            db::meta_set(&state.db, key, if on { "1" } else { "0" })?;
            send(bot, chat_id, format!("✅ {label}: {}", bool_str(on)), None).await?;
        }
        None => send(bot, chat_id, format!("用法: {usage}"), None).await?,
    }
    Ok(())
}

fn help_text() -> String {
    [
        "🤖 <b>BTBoy 自动追番</b>",
        "",
        "<b>订阅管理</b>",
        "/sub <rss> — 添加订阅（蜜柑RSS）",
        "/list — 列出所有订阅",
        "/show <id> — 订阅详情",
        "/edit <id> — 编辑（起始集/简繁/关键词/启停/删除）",
        "/del <id> — 删除",
        "/push <id> — 立即拉取推送一次",
        "",
        "<b>全局设置</b>",
        "/bind [频道ID] — 绑定推送频道（转发一条频道消息即可）",
        "/rss on|off — RSS 轮询总开关",
        "/interval <分钟> — 轮询间隔",
        "/skiphalf on|off — 跳过 07.5 这类特殊集",
        "/gap on|off — 遗漏检测（缺集通知）",
        "/slack <天数>|off — 摸鱼检测（N 天无更新通知）",
        "/autodisable on|off — 全部集数推完后自动停用",
        "",
        "<b>订阅级</b>",
        "/total <id> <n> — 手动设总集数",
        "/bgm <id> <bgmid> — 绑定 Bangumi 自动获取总集数",
        "/backup <id> <rss> — 设置备用 RSS（主源无更新时兜底）",
        "/rmbackup <id> — 移除备用 RSS",
        "",
        "<b>历史 / 待办</b>",
        "/history [id] [n] — 查看推送历史",
        "/pending — 查看待选择项并重发",
        "",
        "<b>备份</b>",
        "/export — 导出订阅备份（JSON）",
        "/import — 导入订阅备份",
        "",
        "<b>其他</b>",
        "/test — 发测试消息到频道",
        "/status — 状态",
        "/logs [行数] — 查看日志",
        "/cancel — 取消当前对话",
        "/admin [id] — 首个使用者成为管理员",
    ]
    .join("\n")
}

fn sub_detail(state: &Arc<AppState>, s: &SubRow) -> String {
    let lang = if s.lang_pref.is_empty() || s.lang_pref == "ask" {
        "ask".to_string()
    } else {
        s.lang_pref.clone()
    };
    let pushed = db::count_pushed_for_sub(&state.db, s.id).unwrap_or(0);
    let pending = db::pending_count_for_sub(&state.db, s.id).unwrap_or(0);
    let latest = db::pushed_episodes(&state.db, s.id)
        .ok()
        .and_then(|v| v.into_iter().max())
        .map(|e| format!("第{}话", fmt_episode_i(e)))
        .unwrap_or_else(|| "-".into());
    format!(
        "<b>#{id} {title}</b>\n\
         状态: {st}\nRSS: {rss}\n\
         进度: 已推 {pushed} 集 · 最新 {latest} · 待决策 {pending}\n\
         起始集: {start} · 简繁: {lang}\n\
         包含词: {inc} · 排除词: {exc}\n\
         备用 RSS: {backup}\n\
         总集数: {total}\n\
         BGM ID: {bgm}\n\
         上次拉取: {fetch}\n\
         上次推送: {push}\n\
         创建: {created}",
        id = s.id,
        title = html_escape(&s.title),
        st = if s.enabled { "🟢 启用" } else { "⏸️ 停用" },
        pushed = pushed,
        latest = latest,
        pending = pending,
        rss = html_escape(&s.rss_url),
        start = fmt_episode_i(s.start_episode),
        lang = lang,
        inc = if s.include_kw.is_empty() { "-" } else { &s.include_kw },
        exc = if s.exclude_kw.is_empty() { "-" } else { &s.exclude_kw },
        backup = if s.backup_rss_url.is_empty() { "-" } else { &s.backup_rss_url },
        total = s
            .total_episodes
            .map(|t| t.to_string())
            .unwrap_or_else(|| "-".into()),
        bgm = s.bgm_id.map(|b| b.to_string()).unwrap_or_else(|| "-".into()),
        fetch = s.last_fetch_at.as_deref().unwrap_or("-"),
        push = s.last_push_at.as_deref().unwrap_or("-"),
        created = s.created_at,
    )
}

fn edit_kb(sub_id: i64) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("番名", format!("edit:{sub_id}:title")),
            InlineKeyboardButton::callback("起始集", format!("edit:{sub_id}:start_episode")),
        ],
        vec![
            InlineKeyboardButton::callback("简繁", format!("editlangsel:{sub_id}")),
            InlineKeyboardButton::callback("包含词", format!("edit:{sub_id}:include_kw")),
            InlineKeyboardButton::callback("排除词", format!("edit:{sub_id}:exclude_kw")),
        ],
        vec![
            InlineKeyboardButton::callback("启/停", format!("toggle:{sub_id}")),
            InlineKeyboardButton::callback("删除", format!("del:{sub_id}")),
        ],
    ])
}

fn fmt_episode_i(e: i64) -> String {
    if e < 100 {
        format!("{e:02}")
    } else {
        e.to_string()
    }
}

async fn send(
    bot: &Bot,
    chat_id: i64,
    text: impl Into<String>,
    kb: Option<InlineKeyboardMarkup>,
) -> Result<()> {
    let text = text.into();
    crate::notifier::send_retry(|| {
        let mut req = bot.send_message(ChatId(chat_id), text.clone());
        req = req.parse_mode(ParseMode::Html);
        if let Some(kb) = &kb {
            req = req.reply_markup(kb.clone());
        }
        async move { req.await }
    })
    .await?;
    Ok(())
}

async fn start_sub_flow(bot: &Bot, state: &Arc<AppState>, chat_id: i64, url: &str) -> Result<()> {
    let mut preview = None;
    let mut dup_lines: Vec<String> = Vec::new();
    let mut sources: Vec<String> = Vec::new();
    if let Ok(items) = crate::rss::fetch_rss(&state.http, url).await {
        if let Some(item) = items.first() {
            let p = crate::parser::parse_title(&item.title);
            if !p.anime.is_empty() {
                preview = Some(p.anime);
            }
        }
        // 收集所有出现的片源
        for it in &items {
            let p = crate::parser::parse_title(&it.title);
            if let Some(s) = p.source {
                if !sources.contains(&s) {
                    sources.push(s);
                }
            }
        }
        sources.sort();
        // 重复集数预览：同集不同来源/版本/简繁
        for (ep, sigs) in crate::scheduler::duplicate_report(&items) {
            dup_lines.push(format!(
                "第{}话: {}",
                fmt_episode_i(ep as i64),
                sigs.join(" / ")
            ));
        }
    }
    let title = preview.unwrap_or_else(|| "(无法自动识别番名)".to_string());
    db::conv_set(
        &state.db,
        chat_id,
        &json!({
            "step":"await_sub_confirm",
            "rss_url":url,
            "title":title,
            "sources":sources
        })
        .to_string(),
    )?;
    let kb = InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            "✅ 用默认配置",
            "subc:default",
        )],
        vec![
            InlineKeyboardButton::callback("⚙️ 自定义", "subc:yes"),
            InlineKeyboardButton::callback("取消", "subc:no"),
        ],
    ]);
    let mut text = format!(
        "识别到的番名可能是:\n<b>{}</b>\n\n默认配置：从第01话起 · 简繁每次问我。\n可直接用默认，或点自定义逐步调整。",
        html_escape(&title)
    );
    if !dup_lines.is_empty() {
        text = format!(
            "识别到的番名可能是:\n<b>{}</b>\n\n⚠️ 检测到<b>重复集数</b>（同集不同来源/版本/简繁，推送时会按你的偏好处理）:\n{}\n\n默认配置：从第01话起 · 简繁每次问我。",
            html_escape(&title),
            dup_lines.join("\n")
        );
    }
    send(bot, chat_id, text, Some(kb)).await?;
    Ok(())
}

async fn handle_conversation_text(
    bot: &Bot,
    state: &Arc<AppState>,
    msg: &Message,
    chat_id: i64,
    admin: Option<i64>,
) -> Result<()> {
    if admin != Some(chat_id) {
        return Ok(());
    }
    let Some(data) = db::conv_get(&state.db, chat_id) else {
        return Ok(());
    };
    let v: serde_json::Value = serde_json::from_str(&data)?;
    let step = v.get("step").and_then(|s| s.as_str()).unwrap_or("");
    match step {
        "await_channel" => {
            // 直接输入频道ID
            if let Some(t) = msg.text() {
                if let Ok(ch) = t.trim().parse::<i64>() {
                    db::meta_set(&state.db, "channel_id", &ch.to_string())?;
                    db::conv_clear(&state.db, chat_id)?;
                    send(bot, chat_id, format!("✅ 已绑定频道 <b>{ch}</b>"), None).await?;
                    return Ok(());
                }
            }
            // 或转发一条该频道的消息
            if let Some(teloxide::types::MessageOrigin::Channel { chat, .. }) =
                msg.forward_origin()
            {
                let ch = chat.id.0;
                db::meta_set(&state.db, "channel_id", &ch.to_string())?;
                db::conv_clear(&state.db, chat_id)?;
                send(bot, chat_id, format!("✅ 已绑定频道 <b>{ch}</b>"), None).await?;
                return Ok(());
            }
            send(
                bot,
                chat_id,
                "未识别到频道。请<b>转发一条该频道的消息</b>，或<b>直接输入频道ID</b>，或 /cancel 取消",
                None,
            )
            .await?;
        }
        "flow" => {
            let input = msg.text().unwrap_or("").to_string();
            return handle_flow_text(bot, state, chat_id, &v, &input).await;
        }
        "await_sub_url" => {
            let url = msg.text().unwrap_or("").trim().to_string();
            if url.starts_with("http") {
                db::conv_clear(&state.db, chat_id)?;
                start_sub_flow(bot, state, chat_id, &url).await?;
            } else {
                send(
                    bot,
                    chat_id,
                    "❌ 链接格式不对，请输入以 http 开头的 RSS 链接（/cancel 取消）",
                    None,
                )
                .await?;
            }
        }
        "await_sub_confirm" => {
            send(bot, chat_id, "请点击上方按钮确认，或 /cancel 取消", None).await?;
        }
        "await_sub_edit" => {
            let text = msg.text().unwrap_or("").trim().to_string();
            if text.is_empty() {
                return Ok(());
            }
            let mut m = serde_json::Map::new();
            for (k, val) in v.as_object().unwrap() {
                m.insert(k.clone(), val.clone());
            }
            m.insert("start_episode".to_string(), json!(text.parse::<i64>().unwrap_or(1)));
            show_sub_config(bot, state, chat_id, &serde_json::to_string(&m)?).await?;
        }
        "await_sub_title" => {
            let text = msg.text().unwrap_or("").trim().to_string();
            if text.is_empty() {
                return Ok(());
            }
            let mut m = serde_json::Map::new();
            for (k, val) in v.as_object().unwrap() {
                m.insert(k.clone(), val.clone());
            }
            m.insert("title".to_string(), json!(text));
            show_new_poster_picker(bot, state, chat_id, &serde_json::to_string(&m)?).await?;
        }
        "await_edit_value" => {
            let field = v["field"].as_str().unwrap_or("").to_string();
            let sub_id = v["sub_id"].as_i64().unwrap_or(0);
            let val = msg.text().unwrap_or("").trim().to_string();
            if val.is_empty() {
                return Ok(());
            }
            apply_edit(state, bot, chat_id, sub_id, &field, &val).await?;
        }
        "await_sub_include" => {
            let text = msg.text().unwrap_or("").trim().to_string();
            let kw = if text.is_empty() || text == "-" {
                String::new()
            } else {
                text
            };
            let mut m = serde_json::Map::new();
            for (k, val) in v.as_object().unwrap() {
                m.insert(k.clone(), val.clone());
            }
            m.insert("include_kw".to_string(), json!(kw));
            show_sub_config(bot, state, chat_id, &serde_json::to_string(&m)?).await?;
        }
        "await_sub_exclude" => {
            let text = msg.text().unwrap_or("").trim().to_string();
            let kw = if text.is_empty() || text == "-" {
                String::new()
            } else {
                text
            };
            let mut m = serde_json::Map::new();
            for (k, val) in v.as_object().unwrap() {
                m.insert(k.clone(), val.clone());
            }
            m.insert("exclude_kw".to_string(), json!(kw));
            show_sub_config(bot, state, chat_id, &serde_json::to_string(&m)?).await?;
        }
        "await_sub_config" | "await_sub_lang" | "await_sub_source" | "await_sub_poster"
        | "await_edit_poster" => {
            send(bot, chat_id, "请点击下方按钮操作，或 /cancel 取消", None).await?;
        }
        "await_import" => {
            if let Some(doc) = msg.document() {
                match bot.get_file(doc.file.id.clone()).await {
                    Ok(file) => {
                        let mut buf: Vec<u8> = Vec::new();
                        if let Err(e) = bot.download_file(&file.path, &mut buf).await {
                            send(bot, chat_id, format!("❌ 下载文件失败: {e}"), None).await?;
                            return Ok(());
                        }
                        match String::from_utf8(buf) {
                            Ok(text) => apply_import(state, bot, chat_id, &text).await?,
                            Err(_) => {
                                send(bot, chat_id, "❌ 文件不是 UTF-8 文本", None).await?
                            }
                        }
                    }
                    Err(e) => send(bot, chat_id, format!("❌ 获取文件失败: {e}"), None).await?,
                }
            } else if let Some(t) = msg.text() {
                apply_import(state, bot, chat_id, t).await?;
            } else {
                send(
                    bot,
                    chat_id,
                    "请粘贴 JSON 内容，或发送 .json 文件（/cancel 取消）",
                    None,
                )
                .await?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// 解析 /export 的 JSON 并重建订阅，按 rss_url 去重
async fn apply_import(
    state: &Arc<AppState>,
    bot: &Bot,
    chat_id: i64,
    content: &str,
) -> Result<()> {
    let arr: Vec<serde_json::Value> =
        serde_json::from_str(content).context("JSON 解析失败，请确认是 /export 导出的内容")?;
    let mut added = 0;
    let mut skipped = 0;
    for v in arr {
        let rss_url = v["rss_url"].as_str().unwrap_or("").trim().to_string();
        if rss_url.is_empty() || db::has_subscription_rss(&state.db, &rss_url)? {
            skipped += 1;
            continue;
        }
        let title = v["title"].as_str().unwrap_or("(导入订阅)");
        let id = db::add_subscription(&state.db, &rss_url, title)?;
        if let Some(n) = v["start_episode"].as_i64() {
            db::set_sub_start(&state.db, id, n)?;
        }
        if let Some(l) = v["lang_pref"].as_str() {
            db::set_sub_lang(&state.db, id, l)?;
        }
        let inc = v["include_kw"].as_str().unwrap_or("");
        let exc = v["exclude_kw"].as_str().unwrap_or("");
        db::set_sub_kw(&state.db, id, inc, exc)?;
        if let Some(b) = v["backup_rss_url"].as_str().filter(|s| !s.is_empty()) {
            db::set_sub_backup(&state.db, id, Some(b))?;
        }
        if let Some(t) = v["total_episodes"].as_i64() {
            db::set_sub_total(&state.db, id, Some(t))?;
        }
        if let Some(g) = v["bgm_id"].as_i64() {
            db::set_sub_bgm(&state.db, id, Some(g))?;
        }
        if let Some(p) = v["poster_url"].as_str().filter(|s| !s.is_empty()) {
            db::set_sub_poster(&state.db, id, Some(p))?;
        }
        if let Some(e) = v["enabled"].as_bool() {
            db::set_sub_enabled(&state.db, id, e)?;
        }
        added += 1;
    }
    db::conv_clear(&state.db, chat_id)?;
    send(
        bot,
        chat_id,
        format!("✅ 导入完成：新增 {added} 个，跳过 {skipped} 个（重复或无效）"),
        None,
    )
    .await?;
    Ok(())
}

async fn apply_edit(
    state: &Arc<AppState>,
    bot: &Bot,
    chat_id: i64,
    sub_id: i64,
    field: &str,
    val: &str,
) -> Result<()> {
    match field {
        "title" => {
            let title = val.trim();
            if title.is_empty() {
                send(bot, chat_id, "❌ 番名不能为空", None).await?;
                return Ok(());
            }
            db::set_sub_title(&state.db, sub_id, title)?;
            db::clear_sub_poster(&state.db, sub_id)?;
            state.poster_cache.lock().unwrap().remove(&sub_id);
            return re_search_poster_for_sub(bot, state, chat_id, sub_id, title).await;
        }
        "start_episode" => {
            let n: i64 = val.parse().unwrap_or(1);
            db::set_sub_start(&state.db, sub_id, n)?;
        }
        "include_kw" => {
            let sub = db::get_subscription(&state.db, sub_id)?;
            let exc = sub.map(|s| s.exclude_kw).unwrap_or_default();
            db::set_sub_kw(&state.db, sub_id, val, &exc)?;
        }
        "exclude_kw" => {
            let sub = db::get_subscription(&state.db, sub_id)?;
            let inc = sub.map(|s| s.include_kw).unwrap_or_default();
            db::set_sub_kw(&state.db, sub_id, &inc, val)?;
        }
        _ => {}
    }
    db::conv_clear(&state.db, chat_id)?;
    send(bot, chat_id, format!("✅ 已更新 #{sub_id} 的 {field}"), None).await?;
    Ok(())
}

/// 现有订阅改番名后，按新名字重搜 TMDB 展示封面选择（step=await_edit_poster）。
/// 无 Key / 无结果时清对话并仅确认改名。
async fn re_search_poster_for_sub(
    bot: &Bot,
    state: &Arc<AppState>,
    uid: i64,
    sub_id: i64,
    title: &str,
) -> Result<()> {
    let Some(key) = state.config.tmdb_api_key.clone() else {
        db::conv_clear(&state.db, uid)?;
        send(
            bot,
            uid,
            format!(
                "✅ 已更新 #{sub_id} 番名为 <b>{}</b>（未配置 TMDB Key，封面未变动）",
                html_escape(title)
            ),
            Some(edit_kb(sub_id)),
        )
        .await?;
        return Ok(());
    };
    let hits = match crate::tmdb::search(&state.http, &key, title).await {
        Ok(h) if !h.is_empty() => h,
        _ => {
            db::conv_clear(&state.db, uid)?;
            send(
                bot,
                uid,
                format!(
                    "✅ 已更新 #{sub_id} 番名为 <b>{}</b>（未搜索到封面结果）",
                    html_escape(title)
                ),
                Some(edit_kb(sub_id)),
            )
            .await?;
            return Ok(());
        }
    };
    let hits_json: Vec<serde_json::Value> = hits
        .iter()
        .map(|h| json!({"title": h.title, "year": h.year, "poster": h.poster}))
        .collect();
    db::conv_set(
        &state.db,
        uid,
        &json!({
            "step": "await_edit_poster",
            "sub_id": sub_id,
            "hits": hits_json,
        })
        .to_string(),
    )?;

    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    for (i, h) in hits.iter().take(6).enumerate() {
        let label = match (&h.year, &h.poster) {
            (Some(y), Some(_)) => format!("{} ({})", h.title, y),
            _ => h.title.clone(),
        };
        let label: String = label.chars().take(40).collect();
        rows.push(vec![InlineKeyboardButton::callback(
            label,
            format!("editposter:{sub_id}:{i}"),
        )]);
    }
    rows.push(vec![InlineKeyboardButton::callback(
        "无封面",
        format!("editposter:{sub_id}:none"),
    )]);
    let list: Vec<String> = hits
        .iter()
        .take(6)
        .map(|h| {
            let year = h.year.as_deref().unwrap_or("?");
            let has = if h.poster.is_some() { "🖼" } else { "—" };
            format!("{has} {} ({year})", h.title)
        })
        .collect();
    send(
        bot,
        uid,
        format!(
            "✅ 番名已改为 <b>{}</b>\n🎬 按新番名重搜 TMDB，选一个作为封面:\n{}",
            html_escape(title),
            list.join("\n")
        ),
        Some(InlineKeyboardMarkup::new(rows)),
    )
    .await?;
    Ok(())
}

/// 现有订阅改番名后的封面选择回调：`editposter:{sub_id}:{idx|none}`
async fn handle_edit_poster(bot: &Bot, state: &Arc<AppState>, uid: i64, rest: &str) -> Result<()> {
    let mut it = rest.splitn(2, ':');
    let sub_id = it.next().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
    let pick = it.next().unwrap_or("none");
    let Some(data) = db::conv_get(&state.db, uid) else {
        return Ok(());
    };
    let v: serde_json::Value = serde_json::from_str(&data)?;
    let poster_url = if pick == "none" {
        String::new()
    } else {
        let idx: usize = pick.parse().unwrap_or(0);
        v["hits"][idx]["poster"].as_str().unwrap_or("").to_string()
    };
    db::set_sub_poster(
        &state.db,
        sub_id,
        if poster_url.is_empty() {
            None
        } else {
            Some(&poster_url)
        },
    )?;
    state.poster_cache.lock().unwrap().remove(&sub_id);
    db::conv_clear(&state.db, uid)?;
    let title = db::get_subscription(&state.db, sub_id)?
        .map(|s| s.title)
        .unwrap_or_default();
    send(
        bot,
        uid,
        format!(
            "✅ 已更新 #{} 番名与封面\n番名: <b>{}</b>",
            sub_id,
            html_escape(&title)
        ),
        Some(edit_kb(sub_id)),
    )
    .await?;
    Ok(())
}

pub async fn handle_callback(bot: Bot, q: CallbackQuery, state: Arc<AppState>) -> ResponseResult<()> {
    let Some(data) = q.data.clone() else { return Ok(()) };
    let uid = q.from.id.0 as i64;
    let admin = crate::resolve_admin(&state);
    if admin != Some(uid) {
        return Ok(());
    }

    let (prefix, rest) = match data.find(':') {
        Some(i) => (&data[..i], &data[i + 1..]),
        None => (data.as_str(), ""),
    };

    let r = match prefix {
        "pick" | "pickall" | "skip" | "later" | "add" | "noadd" => {
            crate::scheduler::handle_decision(&bot, &state, &data).await
        }
        "pickcmd" => handle_pick_cmd(&bot, &state, uid, rest).await,
        "resend" => {
            let pid: i64 = rest.parse().unwrap_or(0);
            let res: anyhow::Result<()> = async {
                match db::get_pending(&state.db, pid)? {
                    Some(p) => crate::scheduler::render_pending(&state, &p).await,
                    None => {
                        send(&bot, uid, "该待选择项已处理", None).await?;
                        Ok(())
                    }
                }
            }
            .await;
            res
        }
        "menu" => handle_menu(&bot, &state, uid, rest).await,
        "pushall" => {
            let res: anyhow::Result<()> = async {
                crate::scheduler::process_all(&state).await?;
                let n = db::count_subscriptions(&state.db)?;
                send(
                    &bot,
                    uid,
                    format!("⚡ 已触发全部拉取（{n} 个订阅），明细见 /logs"),
                    None,
                )
                .await?;
                Ok(())
            }
            .await;
            res
        }
        "subc" => handle_sub_confirm(&bot, &state, uid, rest).await,
        "subedit" => handle_sub_edit(&bot, &state, uid, rest).await,
        "sublang" => handle_sub_lang(&bot, &state, uid, rest).await,
        "subsrc" => handle_sub_source(&bot, &state, uid, rest).await,
        "subposter" => handle_sub_poster(&bot, &state, uid, rest).await,
        "subinc" => handle_sub_keyword(&bot, &state, uid, "include").await,
        "subexc" => handle_sub_keyword(&bot, &state, uid, "exclude").await,
        "subfin" => handle_sub_finish(&bot, &state, uid).await,
        "edit" => handle_edit_start(&bot, &state, uid, rest).await,
        "editlangsel" => handle_edit_lang_sel(&bot, &state, uid, rest).await,
        "editlang" => handle_edit_lang(&bot, &state, uid, rest).await,
        "editposter" => handle_edit_poster(&bot, &state, uid, rest).await,
        "toggle" => handle_toggle(&bot, &state, uid, rest).await,
        "del" => handle_del(&bot, &state, uid, rest).await,
        "delc" => handle_del_confirm(&bot, &state, uid, rest).await,
        _ => Ok(()),
    };
    if let Err(e) = r {
        tracing::error!("callback 处理失败: {e}");
    }
    bot.answer_callback_query(q.id).await?;
    Ok(())
}

async fn handle_sub_confirm(bot: &Bot, state: &Arc<AppState>, uid: i64, rest: &str) -> Result<()> {
    if rest == "no" {
        db::conv_clear(&state.db, uid)?;
        send(bot, uid, "已取消添加", None).await?;
        return Ok(());
    }
    let Some(data) = db::conv_get(&state.db, uid) else {
        return Ok(());
    };
    let v: serde_json::Value = serde_json::from_str(&data)?;
    let rss_url = v["rss_url"].as_str().unwrap_or("").to_string();
    let title = v["title"].as_str().unwrap_or("").to_string();
    let sources: Vec<String> = v["sources"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    // 自动挑一个封面（TMDB 首个带海报的结果），失败则无封面
    let mut poster_url = String::new();
    if let Some(key) = state.config.tmdb_api_key.clone() {
        if let Ok(hits) = crate::tmdb::search(&state.http, &key, &title).await {
            if let Some(p) = hits.into_iter().find_map(|h| h.poster) {
                poster_url = p;
            }
        }
    }

    let base = json!({
        "rss_url": rss_url,
        "title": title,
        "sources": sources,
        "start_episode": 1,
        "lang": "ask",
        "include_kw": "",
        "exclude_kw": "",
        "poster_url": poster_url,
    });

    if rest == "default" {
        finish_sub(state, bot, uid, &base).await
    } else {
        show_sub_config(bot, state, uid, &base.to_string()).await
    }
}

fn sub_lang_kb() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback("简中", "sublang:简中"),
        InlineKeyboardButton::callback("繁中", "sublang:繁中"),
        InlineKeyboardButton::callback("简繁都要", "sublang:简繁"),
        InlineKeyboardButton::callback("每次问我", "sublang:ask"),
    ]])
}

/// 订阅配置屏：一屏改完所有项（起始集/简繁/片源/关键词/封面）
async fn show_sub_config(
    bot: &Bot,
    state: &Arc<AppState>,
    uid: i64,
    conv_json: &str,
) -> Result<()> {
    let v: serde_json::Value = serde_json::from_str(conv_json)?;
    let mut m = serde_json::Map::new();
    for (k, val) in v.as_object().unwrap() {
        m.insert(k.clone(), val.clone());
    }
    m.insert("step".to_string(), json!("await_sub_config"));

    let start = v["start_episode"].as_i64().unwrap_or(1);
    let lang = v["lang"].as_str().unwrap_or("ask");
    let inc = v["include_kw"].as_str().unwrap_or("");
    let exc = v["exclude_kw"].as_str().unwrap_or("");
    let poster = v["poster_url"].as_str().unwrap_or("");
    let sources: Vec<String> = v["sources"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    // inc 既可能存片源、也可能存关键词：拆开展示，避免同一值出现两次
    let is_source = !inc.is_empty() && sources.iter().any(|s| s == inc);
    let src_show = if is_source {
        inc
    } else if sources.len() >= 2 {
        "全部片源"
    } else {
        "全部"
    };
    let kw_show = if is_source || inc.is_empty() { "无" } else { inc };
    let poster_show = if poster.is_empty() { "无" } else { "已选" };

    let kb = InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("番名", "subedit:title"),
            InlineKeyboardButton::callback("起始集", "subedit:start"),
            InlineKeyboardButton::callback("简繁", "subedit:lang"),
        ],
        vec![
            InlineKeyboardButton::callback("片源", "subedit:source"),
            InlineKeyboardButton::callback("包含词", "subinc"),
            InlineKeyboardButton::callback("排除词", "subexc"),
        ],
        vec![
            InlineKeyboardButton::callback("封面", "subedit:poster"),
            InlineKeyboardButton::callback("✅ 完成", "subfin"),
        ],
    ]);

    db::conv_set(&state.db, uid, &serde_json::to_string(&m)?)?;
    send(
        bot,
        uid,
        format!(
            "⚙️ <b>订阅配置</b>\n起始集: 第{}话 · 简繁: {} · 片源: {}\n包含词: {} · 排除词: {}\n封面: {}\n\n点按钮修改，或直接点 ✅ 完成",
            fmt_episode_i(start),
            if lang == "ask" { "每次问" } else { lang },
            src_show,
            kw_show,
            if exc.is_empty() { "无" } else { exc },
            poster_show,
        ),
        Some(kb),
    )
    .await?;
    Ok(())
}

/// 订阅编辑入口：title / start / lang / source / poster
async fn handle_sub_edit(bot: &Bot, state: &Arc<AppState>, uid: i64, rest: &str) -> Result<()> {
    match rest {
        "title" => {
            let Some(data) = db::conv_get(&state.db, uid) else {
                return Ok(());
            };
            let v: serde_json::Value = serde_json::from_str(&data)?;
            let mut m = serde_json::Map::new();
            for (k, val) in v.as_object().unwrap() {
                m.insert(k.clone(), val.clone());
            }
            m.insert("step".to_string(), json!("await_sub_title"));
            db::conv_set(&state.db, uid, &serde_json::to_string(&m)?)?;
            let cur = v["title"].as_str().unwrap_or("");
            send(
                bot,
                uid,
                format!(
                    "当前番名: <b>{}</b>\n回复新的番名（改后会自动重搜封面，/cancel 取消）",
                    html_escape(cur)
                ),
                None,
            )
            .await?;
        }
        "start" => {
            let Some(data) = db::conv_get(&state.db, uid) else {
                return Ok(());
            };
            let v: serde_json::Value = serde_json::from_str(&data)?;
            let mut m = serde_json::Map::new();
            for (k, val) in v.as_object().unwrap() {
                m.insert(k.clone(), val.clone());
            }
            m.insert("step".to_string(), json!("await_sub_edit"));
            m.insert("field".to_string(), json!("start_episode"));
            db::conv_set(&state.db, uid, &serde_json::to_string(&m)?)?;
            let cur = v["start_episode"].as_i64().unwrap_or(1);
            send(
                bot,
                uid,
                format!(
                    "当前起始集: 第{}话\n回复新的集数（数字，/cancel 取消）",
                    fmt_episode_i(cur)
                ),
                None,
            )
            .await?;
        }
        "lang" => {
            let Some(data) = db::conv_get(&state.db, uid) else {
                return Ok(());
            };
            let v: serde_json::Value = serde_json::from_str(&data)?;
            let mut m = serde_json::Map::new();
            for (k, val) in v.as_object().unwrap() {
                m.insert(k.clone(), val.clone());
            }
            m.insert("step".to_string(), json!("await_sub_lang"));
            db::conv_set(&state.db, uid, &serde_json::to_string(&m)?)?;
            send(bot, uid, "选择简繁偏好:", Some(sub_lang_kb())).await?;
        }
        "source" => {
            let Some(data) = db::conv_get(&state.db, uid) else {
                return Ok(());
            };
            let v: serde_json::Value = serde_json::from_str(&data)?;
            let sources: Vec<String> = v["sources"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            if sources.len() < 2 {
                send(bot, uid, "该番未检测到多个片源，无需设置", None).await?;
                return show_sub_config(bot, state, uid, &data).await;
            }
            let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
            for s in &sources {
                rows.push(vec![InlineKeyboardButton::callback(
                    s.clone(),
                    format!("subsrc:{s}"),
                )]);
            }
            rows.push(vec![
                InlineKeyboardButton::callback("全部都要", "subsrc:ALL"),
                InlineKeyboardButton::callback("每次问我", "subsrc:ASK"),
            ]);
            let mut m = serde_json::Map::new();
            for (k, val) in v.as_object().unwrap() {
                m.insert(k.clone(), val.clone());
            }
            m.insert("step".to_string(), json!("await_sub_source"));
            db::conv_set(&state.db, uid, &serde_json::to_string(&m)?)?;
            send(
                bot,
                uid,
                format!("🎬 选择固定片源（{}）:", sources.join(" / ")),
                Some(InlineKeyboardMarkup::new(rows)),
            )
            .await?;
        }
        "poster" => {
            let Some(data) = db::conv_get(&state.db, uid) else {
                return Ok(());
            };
            return show_new_poster_picker(bot, state, uid, &data).await;
        }
        _ => {}
    }
    Ok(())
}

/// 按当前 conv 里的 title 搜 TMDB 并展示封面选择（step=await_sub_poster）。
/// 未配置 Key 或无结果时回退订阅配置屏。
async fn show_new_poster_picker(
    bot: &Bot,
    state: &Arc<AppState>,
    uid: i64,
    conv_json: &str,
) -> Result<()> {
    let v: serde_json::Value = serde_json::from_str(conv_json)?;
    let title = v["title"].as_str().unwrap_or("").to_string();
    let Some(key) = state.config.tmdb_api_key.clone() else {
        send(bot, uid, "未配置 TMDB Key，跳过封面搜索", None).await?;
        return show_sub_config(bot, state, uid, conv_json).await;
    };
    let hits = match crate::tmdb::search(&state.http, &key, &title).await {
        Ok(h) if !h.is_empty() => h,
        _ => {
            send(bot, uid, "未搜索到封面结果", None).await?;
            return show_sub_config(bot, state, uid, conv_json).await;
        }
    };
    let hits_json: Vec<serde_json::Value> = hits
        .iter()
        .map(|h| json!({"title": h.title, "year": h.year, "poster": h.poster}))
        .collect();
    let mut m = serde_json::Map::new();
    for (k, val) in v.as_object().unwrap() {
        m.insert(k.clone(), val.clone());
    }
    m.insert("step".to_string(), json!("await_sub_poster"));
    m.insert("hits".to_string(), json!(hits_json));
    db::conv_set(&state.db, uid, &serde_json::to_string(&m)?)?;

    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    for (i, h) in hits.iter().take(6).enumerate() {
        let label = match (&h.year, &h.poster) {
            (Some(y), Some(_)) => format!("{} ({})", h.title, y),
            _ => h.title.clone(),
        };
        let label: String = label.chars().take(40).collect();
        rows.push(vec![InlineKeyboardButton::callback(
            label,
            format!("subposter:{i}"),
        )]);
    }
    rows.push(vec![InlineKeyboardButton::callback("无封面", "subposter:none")]);
    let list: Vec<String> = hits
        .iter()
        .take(6)
        .map(|h| {
            let year = h.year.as_deref().unwrap_or("?");
            let has = if h.poster.is_some() { "🖼" } else { "—" };
            format!("{has} {} ({year})", h.title)
        })
        .collect();
    send(
        bot,
        uid,
        format!("🎬 TMDB 搜索结果，选一个作为封面:\n{}", list.join("\n")),
        Some(InlineKeyboardMarkup::new(rows)),
    )
    .await?;
    Ok(())
}

async fn handle_sub_lang(bot: &Bot, state: &Arc<AppState>, uid: i64, rest: &str) -> Result<()> {
    let Some(data) = db::conv_get(&state.db, uid) else {
        return Ok(());
    };
    let v: serde_json::Value = serde_json::from_str(&data)?;
    let lang = if rest == "ask" { "ask" } else { rest };
    let mut m = serde_json::Map::new();
    for (k, val) in v.as_object().unwrap() {
        m.insert(k.clone(), val.clone());
    }
    m.insert("lang".to_string(), json!(lang));
    show_sub_config(bot, state, uid, &serde_json::to_string(&m)?).await
}

async fn handle_sub_source(bot: &Bot, state: &Arc<AppState>, uid: i64, rest: &str) -> Result<()> {
    let Some(data) = db::conv_get(&state.db, uid) else {
        return Ok(());
    };
    let v: serde_json::Value = serde_json::from_str(&data)?;
    // 片源偏好 → 写成包含词，订阅后自动只推该片源
    let include_kw = match rest {
        "ALL" | "ASK" => String::new(),
        src => src.to_string(),
    };
    let mut m = serde_json::Map::new();
    for (k, val) in v.as_object().unwrap() {
        m.insert(k.clone(), val.clone());
    }
    m.insert("include_kw".to_string(), json!(include_kw));
    show_sub_config(bot, state, uid, &serde_json::to_string(&m)?).await
}

async fn handle_sub_keyword(bot: &Bot, state: &Arc<AppState>, uid: i64, which: &str) -> Result<()> {
    let Some(data) = db::conv_get(&state.db, uid) else {
        return Ok(());
    };
    let v: serde_json::Value = serde_json::from_str(&data)?;
    let mut m = serde_json::Map::new();
    for (k, val) in v.as_object().unwrap() {
        m.insert(k.clone(), val.clone());
    }
    let (step, prompt) = if which == "include" {
        (
            "await_sub_include",
            "📌 输入<b>必包含</b>词（逗号分隔，如 1080P,HEVC；输入 <code>-</code> 清除后返回菜单）",
        )
    } else {
        (
            "await_sub_exclude",
            "🚫 输入<b>排除</b>词（逗号分隔，如 生肉,720P；输入 <code>-</code> 清除后返回菜单）",
        )
    };
    m.insert("step".to_string(), json!(step));
    db::conv_set(&state.db, uid, &serde_json::to_string(&m)?)?;
    send(bot, uid, prompt, None).await?;
    Ok(())
}

async fn handle_sub_finish(bot: &Bot, state: &Arc<AppState>, uid: i64) -> Result<()> {
    let Some(data) = db::conv_get(&state.db, uid) else {
        return Ok(());
    };
    let v: serde_json::Value = serde_json::from_str(&data)?;
    finish_sub(state, bot, uid, &v).await
}

async fn handle_sub_poster(bot: &Bot, state: &Arc<AppState>, uid: i64, rest: &str) -> Result<()> {
    let Some(data) = db::conv_get(&state.db, uid) else {
        return Ok(());
    };
    let v: serde_json::Value = serde_json::from_str(&data)?;
    let poster_url = if rest == "none" {
        String::new()
    } else {
        let idx: usize = rest.parse().unwrap_or(0);
        v["hits"][idx]["poster"].as_str().unwrap_or("").to_string()
    };
    let mut m = serde_json::Map::new();
    for (k, val) in v.as_object().unwrap() {
        m.insert(k.clone(), val.clone());
    }
    m.insert("poster_url".to_string(), json!(poster_url));
    show_sub_config(bot, state, uid, &serde_json::to_string(&m)?).await
}

async fn finish_sub(
    state: &Arc<AppState>,
    bot: &Bot,
    uid: i64,
    v: &serde_json::Value,
) -> Result<()> {
    let rss_url = v["rss_url"].as_str().unwrap_or("").to_string();
    let title = v["title"].as_str().unwrap_or("").to_string();
    let start = v["start_episode"].as_i64().unwrap_or(1);
    let lang = v["lang"].as_str().unwrap_or("ask").to_string();
    let include_kw = v["include_kw"].as_str().unwrap_or("").to_string();
    let exclude_kw = v["exclude_kw"].as_str().unwrap_or("").to_string();
    let poster_url = v["poster_url"].as_str().unwrap_or("").to_string();

    let id = db::add_subscription(&state.db, &rss_url, &title)?;
    db::set_sub_start(&state.db, id, start)?;
    db::set_sub_lang(&state.db, id, &lang)?;
    db::set_sub_kw(&state.db, id, &include_kw, &exclude_kw)?;
    if !poster_url.is_empty() {
        db::set_sub_poster(&state.db, id, Some(&poster_url))?;
    }
    db::conv_clear(&state.db, uid)?;
    let idx = db::count_subscriptions(&state.db)?;
    send(
        bot,
        uid,
        format!(
            "✅ 已添加订阅 <b>#{idx}</b> {}\n从{}话起 · 简繁: {} · 包含: {} · 排除: {}\n\
             绑定频道后即开始定时推送（/bind）",
            html_escape(&title),
            fmt_episode_i(start),
            if lang == "ask" { "每次问" } else { &lang },
            if include_kw.is_empty() { "-" } else { &include_kw },
            if exclude_kw.is_empty() { "-" } else { &exclude_kw },
        ),
        None,
    )
    .await?;
    Ok(())
}

async fn handle_edit_start(
    bot: &Bot,
    state: &Arc<AppState>,
    uid: i64,
    rest: &str,
) -> Result<()> {
    let mut it = rest.splitn(2, ':');
    let sub_id = it.next().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
    let field = it.next().unwrap_or("");
    let cur = db::get_subscription(&state.db, sub_id)?
        .map(|s| match field {
            "title" => s.title.clone(),
            "start_episode" => s.start_episode.to_string(),
            "include_kw" => s.include_kw.clone(),
            "exclude_kw" => s.exclude_kw.clone(),
            _ => String::new(),
        })
        .unwrap_or_default();
    db::conv_set(
        &state.db,
        uid,
        &json!({"step":"await_edit_value","field":field,"sub_id":sub_id}).to_string(),
    )?;
    send(
        bot,
        uid,
        format!("当前值: <b>{}</b>\n输入新的值（/cancel 取消）:", html_escape(&cur)),
        None,
    )
    .await?;
    Ok(())
}

async fn handle_edit_lang_sel(
    bot: &Bot,
    _state: &Arc<AppState>,
    uid: i64,
    rest: &str,
) -> Result<()> {
    let sub_id: i64 = rest.parse().unwrap_or(0);
    let kb = InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback("简中", format!("editlang:{sub_id}:简中")),
        InlineKeyboardButton::callback("繁中", format!("editlang:{sub_id}:繁中")),
        InlineKeyboardButton::callback("简繁都要", format!("editlang:{sub_id}:简繁")),
        InlineKeyboardButton::callback("每次问我", format!("editlang:{sub_id}:ask")),
    ]]);
    send(bot, uid, format!("选择 #{sub_id} 的简繁偏好:"), Some(kb)).await?;
    Ok(())
}

async fn handle_edit_lang(bot: &Bot, state: &Arc<AppState>, uid: i64, rest: &str) -> Result<()> {
    let mut it = rest.splitn(2, ':');
    let sub_id = it.next().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
    let lang = it.next().unwrap_or("ask");
    db::set_sub_lang(&state.db, sub_id, lang)?;
    send(bot, uid, format!("✅ #{sub_id} 简繁偏好已设为 {lang}"), None).await?;
    Ok(())
}

async fn handle_toggle(bot: &Bot, state: &Arc<AppState>, uid: i64, rest: &str) -> Result<()> {
    let sub_id: i64 = rest.parse().unwrap_or(0);
    if let Some(s) = db::get_subscription(&state.db, sub_id)? {
        let new = !s.enabled;
        db::set_sub_enabled(&state.db, sub_id, new)?;
        send(
            bot,
            uid,
            format!(
                "✅ #{sub_id} 已{}: {}",
                if new { "启用" } else { "停用" },
                html_escape(&s.title)
            ),
            None,
        )
        .await?;
    }
    Ok(())
}

async fn handle_del(bot: &Bot, state: &Arc<AppState>, uid: i64, rest: &str) -> Result<()> {
    let sub_id: i64 = rest.parse().unwrap_or(0);
    if let Some(s) = db::get_subscription(&state.db, sub_id)? {
        let kb = InlineKeyboardMarkup::new(vec![vec![
            InlineKeyboardButton::callback("确认删除", format!("delc:{sub_id}:yes")),
            InlineKeyboardButton::callback("取消", format!("delc:{sub_id}:no")),
        ]]);
        send(
            bot,
            uid,
            format!("确认删除订阅 <b>{}</b> (#{sub_id})?", html_escape(&s.title)),
            Some(kb),
        )
        .await?;
    }
    Ok(())
}

async fn handle_del_confirm(bot: &Bot, state: &Arc<AppState>, uid: i64, rest: &str) -> Result<()> {
    let mut it = rest.splitn(2, ':');
    let sub_id = it.next().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
    let yes = it.next() == Some("yes");
    if yes {
        db::delete_subscription(&state.db, sub_id)?;
        send(bot, uid, format!("🗑️ 已删除订阅 #{sub_id}"), None).await?;
    } else {
        send(bot, uid, "已取消", None).await?;
    }
    Ok(())
}

// ===================== 交互式参数引导 =====================

/// 需要逐项收集参数的命令: (命令, [(字段, 提示)])
fn flow_fields(cmd: &str) -> Option<&'static [(&'static str, &'static str)]> {
    match cmd {
        "interval" => Some(&[("n", "轮询间隔（分钟，1-1440）？")]),
        "slack" => Some(&[("days", "摸鱼检测天数（填 off 关闭）？")]),
        "total" => Some(&[("id", "订阅ID？"), ("n", "总集数？")]),
        "bgm" => Some(&[("id", "订阅ID？"), ("bgm", "Bangumi 主题 ID？（bgm.tv 详情页 URL 里的数字）")]),
        "backup" => Some(&[("id", "订阅ID？"), ("url", "备用 RSS 链接？")]),
        _ => None,
    }
}

async fn start_flow(bot: &Bot, state: &Arc<AppState>, chat_id: i64, cmd: &str) -> Result<()> {
    let spec = flow_fields(cmd).unwrap();
    let data = json!({"step":"flow","cmd":cmd,"next":0,"fields":{}}).to_string();
    db::conv_set(&state.db, chat_id, &data)?;
    let (label, prompt) = spec[0];
    send(bot, chat_id, format!("⚙️ {label}\n{prompt}（/cancel 取消）"), None).await?;
    Ok(())
}

async fn handle_flow_text(
    bot: &Bot,
    state: &Arc<AppState>,
    chat_id: i64,
    v: &serde_json::Value,
    input: &str,
) -> Result<()> {
    let cmd = v["cmd"].as_str().unwrap_or("").to_string();
    let next = v["next"].as_u64().unwrap_or(0) as usize;
    let mut fields: serde_json::Map<String, serde_json::Value> = v
        .get("fields")
        .and_then(|f| f.as_object())
        .cloned()
        .unwrap_or_default();
    let Some(spec) = flow_fields(&cmd) else {
        db::conv_clear(&state.db, chat_id)?;
        return Ok(());
    };
    let Some((fkey, _)) = spec.get(next) else {
        db::conv_clear(&state.db, chat_id)?;
        return Ok(());
    };

    let val = input.trim();
    if val.is_empty() {
        send(bot, chat_id, "输入不能为空，/cancel 取消", None).await?;
        return Ok(());
    }
    let ok = match *fkey {
        "id" | "n" | "bgm" => val.parse::<i64>().is_ok(),
        "days" => val == "off" || val.parse::<i64>().is_ok(),
        "url" => val.starts_with("http"),
        _ => true,
    };
    if !ok {
        let hint = match *fkey {
            "id" | "n" | "bgm" => "请输入数字",
            "days" => "请输入天数或 off",
            "url" => "请输入 http(s) 链接",
            _ => "输入无效",
        };
        send(bot, chat_id, format!("❌ {hint}，请重试（/cancel 取消）"), None).await?;
        return Ok(());
    }
    fields.insert(fkey.to_string(), json!(val.to_string()));

    if next + 1 < spec.len() {
        let data = json!({"step":"flow","cmd":cmd,"next":next+1,"fields":fields}).to_string();
        db::conv_set(&state.db, chat_id, &data)?;
        let (label, prompt) = spec[next + 1];
        send(bot, chat_id, format!("⚙️ {label}\n{prompt}（/cancel 取消）"), None).await?;
    } else {
        db::conv_clear(&state.db, chat_id)?;
        apply_flow(state, bot, chat_id, &cmd, &fields).await?;
    }
    Ok(())
}

async fn apply_flow(
    state: &Arc<AppState>,
    bot: &Bot,
    chat_id: i64,
    cmd: &str,
    fields: &serde_json::Map<String, serde_json::Value>,
) -> Result<()> {
    let get = |k: &str| -> Option<String> {
        fields.get(k).and_then(|v| v.as_str()).map(|s| s.to_string())
    };
    match cmd {
        "interval" => {
            let n: u64 = get("n").and_then(|s| s.parse().ok()).unwrap_or(5);
            db::meta_set(&state.db, "fetch_interval_min", &n.to_string())?;
            send(bot, chat_id, format!("✅ 轮询间隔设为 {n} 分钟"), None).await?;
        }
        "slack" => {
            let days: i64 = match get("days").as_deref() {
                Some("off") | Some("0") => 0,
                s => s.and_then(|x| x.parse().ok()).unwrap_or(0),
            };
            db::meta_set(&state.db, "slack_days", &days.to_string())?;
            if days > 0 {
                send(bot, chat_id, format!("✅ 摸鱼检测: {days} 天没更新会通知"), None).await?;
            } else {
                send(bot, chat_id, "✅ 摸鱼检测已关闭", None).await?;
            }
        }
        "total" => {
            let id: i64 = get("id").and_then(|s| s.parse().ok()).unwrap_or(0);
            let n: i64 = get("n").and_then(|s| s.parse().ok()).unwrap_or(0);
            db::set_sub_total(&state.db, id, Some(n))?;
            send(bot, chat_id, format!("✅ #{id} 总集数设为 {n}"), None).await?;
        }
        "bgm" => {
            let id: i64 = get("id").and_then(|s| s.parse().ok()).unwrap_or(0);
            let bgm: i64 = get("bgm").and_then(|s| s.parse().ok()).unwrap_or(0);
            db::set_sub_bgm(&state.db, id, Some(bgm))?;
            match crate::scheduler::fetch_bgm_total(&state.http, bgm).await {
                Ok(Some(total)) => {
                    db::set_sub_total(&state.db, id, Some(total))?;
                    send(bot, chat_id, format!("✅ #{id} 绑定 BGM {bgm}，总集数 {total}"), None).await?;
                }
                _ => send(
                    bot,
                    chat_id,
                    format!("✅ #{id} 已绑定 BGM {bgm}（暂时取不到总集数，稍后自动刷新）"),
                    None,
                )
                .await?,
            }
        }
        "backup" => {
            let id: i64 = get("id").and_then(|s| s.parse().ok()).unwrap_or(0);
            let url = get("url").unwrap_or_default();
            db::set_sub_backup(&state.db, id, Some(&url))?;
            send(bot, chat_id, format!("✅ #{id} 备用 RSS 已设置"), None).await?;
        }
        _ => {}
    }
    Ok(())
}

/// 选择订阅（通用），供 show/edit/del/total/bgm/backup/rmbackup 使用
async fn show_sub_picker(bot: &Bot, state: &Arc<AppState>, chat_id: i64, cmd: &str) -> Result<()> {
    let subs = db::list_subscriptions(&state.db)?;
    if subs.is_empty() {
        send(bot, chat_id, "📭 还没有订阅，先 /sub 添加", None).await?;
        return Ok(());
    }
    let rows: Vec<Vec<InlineKeyboardButton>> = subs
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut label = format!("#{} {}", i + 1, s.title);
            if label.chars().count() > 40 {
                label = label.chars().take(40).collect();
            }
            vec![InlineKeyboardButton::callback(label, format!("pickcmd:{cmd}:{}", i + 1))]
        })
        .collect();
    send(
        bot,
        chat_id,
        format!("选择订阅（{cmd}）:"),
        Some(InlineKeyboardMarkup::new(rows)),
    )
    .await?;
    Ok(())
}

/// /push 结果文案：明确告诉用户成没成功
fn push_result_text(id: i64, r: &crate::scheduler::ProcessReport) -> String {
    if r.new == 0 {
        format!("✅ #{id} 没有新内容（已是最新）")
    } else if r.asked > 0 {
        format!(
            "✅ #{id} 拉取完成: 新候选 {} · 已推送 {} · 有 {} 个待你选择",
            r.new, r.pushed, r.asked
        )
    } else {
        format!("✅ #{id} 拉取完成: 新候选 {} · 已推送 {}", r.new, r.pushed)
    }
}

/// /push 对话框：全部拉取 + 逐个订阅
async fn show_push_dialog(bot: &Bot, state: &Arc<AppState>, chat_id: i64) -> Result<()> {
    let subs = db::list_subscriptions(&state.db)?;
    if subs.is_empty() {
        send(bot, chat_id, "📭 还没有订阅，先 /sub 添加", None).await?;
        return Ok(());
    }
    let mut rows = vec![vec![InlineKeyboardButton::callback("⚡ 全部拉取", "pushall")]];
    for (i, s) in subs.iter().enumerate() {
        let mut label = format!("#{} {}", i + 1, s.title);
        if label.chars().count() > 40 {
            label = label.chars().take(40).collect();
        }
        rows.push(vec![InlineKeyboardButton::callback(
            label,
            format!("pickcmd:push:{}", i + 1),
        )]);
    }
    send(
        bot,
        chat_id,
        "⚡ 立即拉取\n选择要拉取的订阅：",
        Some(InlineKeyboardMarkup::new(rows)),
    )
    .await?;
    Ok(())
}

async fn handle_pick_cmd(bot: &Bot, state: &Arc<AppState>, uid: i64, rest: &str) -> Result<()> {
    let mut it = rest.splitn(2, ':');
    let cmd = it.next().unwrap_or("");
    // 选择器按钮带的是"序号"(列表位置)，先换算成真实 id
    let idx: i64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let Some(id) = db::resolve_sub_by_index(&state.db, idx)? else {
        send(bot, uid, format!("未找到订阅 #{idx}"), None).await?;
        return Ok(());
    };
    match cmd {
        "push" => {
            if let Some(s) = db::get_subscription(&state.db, id)? {
                match crate::scheduler::process_subscription(state, &s, false).await {
                    Ok(r) => send(bot, uid, push_result_text(idx, &r), None).await?,
                    Err(e) => {
                        send(bot, uid, format!("❌ #{idx} 拉取失败: {e}"), None).await?
                    }
                }
            }
        }
        "show" => {
            if let Some(s) = db::get_subscription(&state.db, id)? {
                send(bot, uid, sub_detail(state, &s), None).await?;
            }
        }
        "del" => {
            if let Some(s) = db::get_subscription(&state.db, id)? {
                let kb = InlineKeyboardMarkup::new(vec![vec![
                    InlineKeyboardButton::callback("确认删除", format!("delc:{id}:yes")),
                    InlineKeyboardButton::callback("取消", format!("delc:{id}:no")),
                ]]);
                send(
                    bot,
                    uid,
                    format!("确认删除订阅 <b>{}</b> (#{idx})?", html_escape(&s.title)),
                    Some(kb),
                )
                .await?;
            }
        }
        "edit" => {
            send(bot, uid, format!("编辑订阅 <b>#{idx}</b>"), Some(edit_kb(id))).await?;
        }
        "rmbackup" => {
            db::set_sub_backup(&state.db, id, None)?;
            send(bot, uid, format!("✅ #{idx} 备用 RSS 已移除"), None).await?;
        }
        "total" | "bgm" | "backup" => {
            let spec = flow_fields(cmd).unwrap();
            let mut fields = serde_json::Map::new();
            fields.insert("id".to_string(), json!(id.to_string()));
            let data = json!({"step":"flow","cmd":cmd,"next":1,"fields":fields}).to_string();
            db::conv_set(&state.db, uid, &data)?;
            let (label, prompt) = spec[1];
            send(bot, uid, format!("⚙️ {label}\n{prompt}（/cancel 取消）"), None).await?;
        }
        _ => {}
    }
    Ok(())
}
