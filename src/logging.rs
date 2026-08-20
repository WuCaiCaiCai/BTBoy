use anyhow::Result;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// 初始化双层日志：Docker stdout + 滚动文件 logs/btboy.log
pub fn init(level: &str) -> Result<tracing_appender::non_blocking::WorkerGuard> {
    std::fs::create_dir_all("logs")?;

    let file = tracing_appender::rolling::daily("logs", "btboy.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file);

    let filter = tracing_subscriber::EnvFilter::try_new(level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(file_writer)
                .with_ansi(false),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stdout))
        .with(filter)
        .try_init()?;

    Ok(guard)
}

/// 读取日志文件末尾若干行（供 /logs 命令使用）
pub fn tail_logs(n: usize) -> String {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let candidates = [
        std::path::PathBuf::from("logs").join(format!("btboy.log.{today}")),
        std::path::PathBuf::from("logs").join("btboy.log"),
    ];
    let Some(path) = candidates.into_iter().find(|c| c.exists()) else {
        return "(暂无日志)".to_string();
    };
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(n);
            lines[start..].join("\n")
        }
        Err(e) => format!("(读取日志失败: {e})"),
    }
}
