use anyhow::{Context, Result};

/// 应用配置，全部来自环境变量（兼容 Docker / .env）
#[derive(Debug, Clone)]
pub struct Config {
    /// Telegram Bot Token（BotFather 获取）
    pub bot_token: String,
    /// 管理员数字 ID，可选（未设置则用 /admin 注册第一个使用者）
    pub admin_id: Option<i64>,
    /// 全局推送频道 ID，可选（未设置可用 /bind 绑定）
    pub channel_id: Option<i64>,
    /// 拉取 RSS 的间隔（分钟）
    pub fetch_interval_min: u64,
    /// SQLite 数据库路径
    pub db_path: String,
    /// 日志级别
    pub log_level: String,
    /// TMDB API Key（可选，用于封面推送）
    pub tmdb_api_key: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let bot_token = std::env::var("TELEGRAM_BOT_TOKEN")
            .context("缺少环境变量 TELEGRAM_BOT_TOKEN（BotFather 获取）")?;

        let admin_id = std::env::var("ADMIN_ID")
            .ok()
            .and_then(|s| s.trim().parse().ok());
        let channel_id = std::env::var("CHANNEL_ID")
            .ok()
            .and_then(|s| s.trim().parse().ok());
        let fetch_interval_min = std::env::var("FETCH_INTERVAL_MIN")
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(5);
        let db_path = std::env::var("DB_PATH").unwrap_or_else(|_| "data/btboy.db".into());
        let log_level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into());
        let tmdb_api_key = std::env::var("TMDB_API_KEY").ok().filter(|s| !s.is_empty());

        Ok(Config {
            bot_token,
            admin_id,
            channel_id,
            fetch_interval_min,
            db_path,
            log_level,
            tmdb_api_key,
        })
    }
}
