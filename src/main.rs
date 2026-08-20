mod bencode;
mod bot;
mod config;
mod db;
mod filter;
mod logging;
mod models;
mod notifier;
mod parser;
mod rss;
mod scheduler;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use teloxide::prelude::*;

pub struct AppState {
    pub config: config::Config,
    pub db: db::Db,
    pub http: reqwest::Client,
    pub bot: Bot,
    pub started: Instant,
}

/// 管理员：环境变量 ADMIN_ID 优先，其次 meta（/admin 注册）
pub fn resolve_admin(state: &AppState) -> Option<i64> {
    state
        .config
        .admin_id
        .or_else(|| db::meta_get(&state.db, "admin_id").and_then(|s| s.trim().parse().ok()))
}

/// 频道：环境变量 CHANNEL_ID 优先，其次 meta（/bind 绑定）
pub fn resolve_channel(state: &AppState) -> Option<i64> {
    state
        .config
        .channel_id
        .or_else(|| db::meta_get(&state.db, "channel_id").and_then(|s| s.trim().parse().ok()))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let config = config::Config::from_env()?;
    let _guard = logging::init(&config.log_level)?;

    let conn = db::open(&config.db_path)?;
    let db = Arc::new(Mutex::new(conn));
    db::migrate(&db)?;

    let http = reqwest::Client::builder()
        .user_agent("BTBoy/0.1 (https://github.com/)")
        .build()?;
    let bot = Bot::new(&config.bot_token);

    let state = Arc::new(AppState {
        config,
        db,
        http,
        bot: bot.clone(),
        started: Instant::now(),
    });

    let sched_state = state.clone();
    let scheduler_handle = tokio::spawn(async move {
        scheduler::run(sched_state).await;
    });

    let handler = dptree::entry()
        .branch(Update::filter_message().endpoint(bot::handle_message))
        .branch(Update::filter_callback_query().endpoint(bot::handle_callback));

    tracing::info!("BTBoy 启动完成，等待指令...");

    Dispatcher::builder(bot.clone(), handler)
        .dependencies(dptree::deps![state])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    scheduler_handle.abort();
    Ok(())
}
