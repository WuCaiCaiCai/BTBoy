use std::sync::Arc;

use anyhow::Result;
use serde_json::json;
use teloxide::prelude::*;
use teloxide::types::{
    CallbackQuery, ChatId, InlineKeyboardButton, InlineKeyboardMarkup, Message, ParseMode,
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
        "start" | "help" => send(bot, chat_id, help_text(), None).await?,

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
                    "📣 请<b>转发一条来自目标频道</b>的消息给我（先把机器人加为该频道管理员），\
                     或直接发 /bind <频道ID>",
                    None,
                )
                .await?;
            }
        }

        "sub" => {
            let url = args.first().map(|s| s.as_str()).unwrap_or("");
            if url.is_empty() || !url.starts_with("http") {
                send(
                    bot,
                    chat_id,
                    "用法: /sub <蜜柑RSS链接>\n例如 /sub https://mikanani.me/RSS/xxxxxx",
                    None,
                )
                .await?;
            } else {
                start_sub_flow(bot, state, chat_id, url).await?;
            }
        }

        "list" => {
            let subs = db::list_subscriptions(&state.db)?;
            if subs.is_empty() {
                send(bot, chat_id, "📭 还没有订阅，用 /sub <RSS链接> 添加", None).await?;
            } else {
                let mut lines = Vec::new();
                for s in subs {
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
                    lines.push(format!(
                        "{flag} <b>#{}</b> {} · 从{ep}话起 · {lang}{extra}\n    {}",
                        s.id,
                        html_escape(&s.title),
                        html_escape(&s.rss_url)
                    ));
                }
                send(bot, chat_id, lines.join("\n\n"), None).await?;
            }
        }

        "show" => {
            let id: i64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(0);
            match db::get_subscription(&state.db, id)? {
                Some(s) => send(bot, chat_id, sub_detail(&s), None).await?,
                None => send(bot, chat_id, format!("未找到订阅 #{id}"), None).await?,
            }
        }

        "edit" => {
            let id: i64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(0);
            if db::get_subscription(&state.db, id)?.is_none() {
                send(bot, chat_id, format!("未找到订阅 #{id}"), None).await?;
            } else {
                let kb = edit_kb(id);
                send(bot, chat_id, format!("编辑订阅 <b>#{id}</b>"), Some(kb)).await?;
            }
        }

        "del" => {
            let id: i64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(0);
            match db::get_subscription(&state.db, id)? {
                Some(s) => {
                    let kb = InlineKeyboardMarkup::new(vec![vec![
                        InlineKeyboardButton::callback("确认删除", format!("delc:{id}:yes")),
                        InlineKeyboardButton::callback("取消", format!("delc:{id}:no")),
                    ]]);
                    send(
                        bot,
                        chat_id,
                        format!("确认删除订阅 <b>{}</b> (#{id})?", html_escape(&s.title)),
                        Some(kb),
                    )
                    .await?;
                }
                None => send(bot, chat_id, format!("未找到订阅 #{id}"), None).await?,
            }
        }

        "push" => {
            let id: i64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(0);
            match db::get_subscription(&state.db, id)? {
                Some(s) => {
                    let r = crate::scheduler::process_subscription(state, &s).await?;
                    send(
                        bot,
                        chat_id,
                        format!(
                            "✅ #{id} 拉取完成: 新候选 {} · 推送 {} · 询问 {}",
                            r.new, r.pushed, r.asked
                        ),
                        None,
                    )
                    .await?;
                }
                None => send(bot, chat_id, format!("未找到订阅 #{id}"), None).await?,
            }
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

        "status" => {
            let nsub = db::count_subscriptions(&state.db)?;
            let npush = db::count_pushed(&state.db)?;
            let ch = crate::resolve_channel(state)
                .map(|c| c.to_string())
                .unwrap_or_else(|| "未绑定".into());
            let admin_id = admin.map(|a| a.to_string()).unwrap_or_else(|| "未设置".into());
            let uptime = state.started.elapsed().as_secs();
            let rss_on = db::meta_bool(&state.db, "rss_enabled", true);
            let interval = db::meta_int(&state.db, "fetch_interval_min", state.config.fetch_interval_min as i64);
            let skip_half = db::meta_bool(&state.db, "skip_half", false);
            let gap = db::meta_bool(&state.db, "gap_detect", false);
            let slack = db::meta_int(&state.db, "slack_days", 0);
            let autodisable = db::meta_bool(&state.db, "autodisable", false);
            let text = format!(
                "📊 <b>BTBoy 状态</b>\n\
                 管理员: {admin_id}\n推送频道: {ch}\n\
                 订阅数: {nsub} · 已推送: {npush}\n\
                 运行时长: {uptime}s\n\
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
        }

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
            let on = parse_on_off(args.first().map(|s| s.as_str()).unwrap_or(""))?;
            db::meta_set(&state.db, "rss_enabled", if on { "1" } else { "0" })?;
            send(
                bot,
                chat_id,
                format!("✅ RSS 轮询总开关: {}", bool_str(on)),
                None,
            )
            .await?;
        }

        "interval" => {
            let n: u64 = args.first()
                .and_then(|s| s.parse().ok())
                .filter(|n| *n >= 1 && *n <= 1440)
                .unwrap_or(5);
            db::meta_set(&state.db, "fetch_interval_min", &n.to_string())?;
            send(bot, chat_id, format!("✅ 轮询间隔设为 {n} 分钟"), None).await?;
        }

        "skiphalf" => {
            let on = parse_on_off(args.first().map(|s| s.as_str()).unwrap_or(""))?;
            db::meta_set(&state.db, "skip_half", if on { "1" } else { "0" })?;
            send(bot, chat_id, format!("✅ 跳过 .5 特殊集: {}", bool_str(on)), None).await?;
        }

        "gap" => {
            let on = parse_on_off(args.first().map(|s| s.as_str()).unwrap_or(""))?;
            db::meta_set(&state.db, "gap_detect", if on { "1" } else { "0" })?;
            send(bot, chat_id, format!("✅ 遗漏检测通知: {}", bool_str(on)), None).await?;
        }

        "slack" => {
            let days = match args.first().map(|s| s.as_str()) {
                Some("off") | Some("0") | None => 0,
                Some(s) => s.parse::<i64>().unwrap_or(0),
            };
            db::meta_set(&state.db, "slack_days", &days.to_string())?;
            if days > 0 {
                send(bot, chat_id, format!("✅ 摸鱼检测: {days} 天没更新会通知"), None).await?;
            } else {
                send(bot, chat_id, "✅ 摸鱼检测已关闭", None).await?;
            }
        }

        "autodisable" => {
            let on = parse_on_off(args.first().map(|s| s.as_str()).unwrap_or(""))?;
            db::meta_set(&state.db, "autodisable", if on { "1" } else { "0" })?;
            send(
                bot,
                chat_id,
                format!("✅ 全部集数推送后自动停用订阅: {}", bool_str(on)),
                None,
            )
            .await?;
        }

        // ---- 订阅级 ----
        "total" => {
            let (id, n) = (args.first().and_then(|s| s.parse().ok()), args.get(1).and_then(|s| s.parse().ok()));
            let (Some(id), Some(n)) = (id, n) else {
                send(bot, chat_id, "用法: /total <订阅ID> <总集数>", None).await?;
                return Ok(());
            };
            db::set_sub_total(&state.db, id, Some(n))?;
            send(bot, chat_id, format!("✅ #{id} 总集数设为 {n}"), None).await?;
        }

        "bgm" => {
            let (id, bgm) = (args.first().and_then(|s| s.parse().ok()), args.get(1).and_then(|s| s.parse().ok()));
            let (Some(id), Some(bgm)) = (id, bgm) else {
                send(bot, chat_id, "用法: /bgm <订阅ID> <Bangumi主题ID>\n可在 bgm.tv 番剧详情页 URL 里找到 ID", None).await?;
                return Ok(());
            };
            db::set_sub_bgm(&state.db, id, Some(bgm))?;
            match crate::scheduler::fetch_bgm_total(&state.http, bgm).await {
                Ok(Some(total)) => {
                    db::set_sub_total(&state.db, id, Some(total))?;
                    send(bot, chat_id, format!("✅ #{id} 绑定 BGM {bgm}，总集数 {total}"), None).await?;
                }
                _ => send(bot, chat_id, format!("✅ #{id} 已绑定 BGM {bgm}（暂时取不到总集数，稍后会自动刷新）"), None).await?,
            }
        }

        "backup" => {
            let id: Option<i64> = args.first().and_then(|s| s.parse().ok());
            let url = args.get(1).map(|s| s.as_str()).unwrap_or("");
            let Some(id) = id else {
                send(bot, chat_id, "用法: /backup <订阅ID> <备用RSS链接>", None).await?;
                return Ok(());
            };
            if url.is_empty() || !url.starts_with("http") {
                send(bot, chat_id, "用法: /backup <订阅ID> <备用RSS链接>", None).await?;
                return Ok(());
            }
            db::set_sub_backup(&state.db, id, Some(url))?;
            send(bot, chat_id, format!("✅ #{id} 备用 RSS 已设置"), None).await?;
        }

        "rmbackup" => {
            let id: i64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(0);
            db::set_sub_backup(&state.db, id, None)?;
            send(bot, chat_id, format!("✅ #{id} 备用 RSS 已移除"), None).await?;
        }

        _ => send(bot, chat_id, "未知命令，发送 /help 查看帮助", None).await?,
    }
    Ok(())
}

fn parse_on_off(s: &str) -> Result<bool> {
    match s {
        "on" | "开" | "1" => Ok(true),
        "off" | "关" | "0" => Ok(false),
        _ => Ok(true),
    }
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
        "<b>其他</b>",
        "/test — 发测试消息到频道",
        "/status — 状态",
        "/logs [行数] — 查看日志",
        "/cancel — 取消当前对话",
        "/admin [id] — 首个使用者成为管理员",
    ]
    .join("\n")
}

fn sub_detail(s: &SubRow) -> String {
    let lang = if s.lang_pref.is_empty() || s.lang_pref == "ask" {
        "ask".to_string()
    } else {
        s.lang_pref.clone()
    };
    format!(
        "<b>#{id} {title}</b>\n\
         状态: {st}\nRSS: {rss}\n\
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
            InlineKeyboardButton::callback("起始集", format!("edit:{sub_id}:start_episode")),
            InlineKeyboardButton::callback("简繁", format!("editlangsel:{sub_id}")),
        ],
        vec![
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
    let mut req = bot.send_message(ChatId(chat_id), text.into());
    req = req.parse_mode(ParseMode::Html);
    if let Some(kb) = kb {
        req = req.reply_markup(kb);
    }
    req.await?;
    Ok(())
}

async fn start_sub_flow(bot: &Bot, state: &Arc<AppState>, chat_id: i64, url: &str) -> Result<()> {
    let mut preview = None;
    if let Ok(items) = crate::rss::fetch_rss(&state.http, url).await {
        if let Some(item) = items.first() {
            let p = crate::parser::parse_title(&item.title);
            if !p.anime.is_empty() {
                preview = Some(p.anime);
            }
        }
    }
    let title = preview.unwrap_or_else(|| "(无法自动识别番名)".to_string());
    db::conv_set(
        &state.db,
        chat_id,
        &json!({"step":"await_sub_confirm","rss_url":url,"title":title}).to_string(),
    )?;
    let kb = InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback("确认", "subc:yes"),
        InlineKeyboardButton::callback("取消", "subc:no"),
    ]]);
    let text = format!(
        "识别到的番名可能是:\n<b>{}</b>\n\n确认后设置起始集和简繁偏好。",
        html_escape(&title)
    );
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
            if let Some(teloxide::types::MessageOrigin::Channel { chat, .. }) =
                msg.forward_origin()
            {
                let ch = chat.id.0;
                db::meta_set(&state.db, "channel_id", &ch.to_string())?;
                db::conv_clear(&state.db, chat_id)?;
                send(bot, chat_id, format!("✅ 已绑定频道 <b>{ch}</b>"), None).await?;
                return Ok(());
            }
            send(bot, chat_id, "请<b>转发一条来自目标频道</b>的消息，或 /cancel 取消", None).await?;
        }
        "await_sub_confirm" => {
            send(bot, chat_id, "请点击上方按钮确认，或 /cancel 取消", None).await?;
        }
        "await_sub_episode" => {
            let rss_url = v["rss_url"].as_str().unwrap_or("").to_string();
            let title = v["title"].as_str().unwrap_or("").to_string();
            let text = msg.text().unwrap_or("").trim().to_string();
            if text.is_empty() {
                return Ok(());
            }
            let start: i64 = text.parse().unwrap_or(1);
            db::conv_set(
                &state.db,
                chat_id,
                &json!({"step":"await_sub_lang","rss_url":rss_url,"title":title,"start_episode":start})
                    .to_string(),
            )?;
            let kb = InlineKeyboardMarkup::new(vec![vec![
                InlineKeyboardButton::callback("简中", "sublang:简中"),
                InlineKeyboardButton::callback("繁中", "sublang:繁中"),
                InlineKeyboardButton::callback("简繁都要", "sublang:简繁"),
                InlineKeyboardButton::callback("每次问我", "sublang:ask"),
            ]]);
            send(
                bot,
                chat_id,
                format!(
                    "从第{start}集开始。现在选简繁偏好（可后续 /edit 修改）:"
                ),
                Some(kb),
            )
            .await?;
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
        _ => {}
    }
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
        "subc" => handle_sub_confirm(&bot, &state, uid, rest).await,
        "sublang" => handle_sub_lang(&bot, &state, uid, rest).await,
        "edit" => handle_edit_start(&bot, &state, uid, rest).await,
        "editlangsel" => handle_edit_lang_sel(&bot, &state, uid, rest).await,
        "editlang" => handle_edit_lang(&bot, &state, uid, rest).await,
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
    let Some(data) = db::conv_get(&state.db, uid) else {
        return Ok(());
    };
    let v: serde_json::Value = serde_json::from_str(&data)?;
    if rest == "yes" {
        let rss_url = v["rss_url"].as_str().unwrap_or("").to_string();
        let title = v["title"].as_str().unwrap_or("").to_string();
        db::conv_set(
            &state.db,
            uid,
            &json!({"step":"await_sub_episode","rss_url":rss_url,"title":title}).to_string(),
        )?;
        send(bot, uid, "从第几集开始推送? (回复数字，默认 1，/cancel 取消)", None).await?;
    } else {
        db::conv_clear(&state.db, uid)?;
        send(bot, uid, "已取消添加", None).await?;
    }
    Ok(())
}

async fn handle_sub_lang(bot: &Bot, state: &Arc<AppState>, uid: i64, rest: &str) -> Result<()> {
    let Some(data) = db::conv_get(&state.db, uid) else {
        return Ok(());
    };
    let v: serde_json::Value = serde_json::from_str(&data)?;
    let rss_url = v["rss_url"].as_str().unwrap_or("").to_string();
    let title = v["title"].as_str().unwrap_or("").to_string();
    let start = v["start_episode"].as_i64().unwrap_or(1);
    let lang = if rest == "ask" { "ask" } else { rest };

    let id = db::add_subscription(&state.db, &rss_url, &title)?;
    db::set_sub_start(&state.db, id, start)?;
    db::set_sub_lang(&state.db, id, lang)?;
    db::conv_clear(&state.db, uid)?;
    send(
        bot,
        uid,
        format!(
            "✅ 已添加订阅 <b>#{id}</b> {}\n从{}话起 · 简繁: {}\n\
             绑定频道后即开始定时推送（/bind）",
            html_escape(&title),
            fmt_episode_i(start),
            lang
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
