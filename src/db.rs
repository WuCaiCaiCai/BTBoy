use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::models::{Pending, SubRow};

pub type Db = Arc<Mutex<Connection>>;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS subscriptions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    rss_url TEXT NOT NULL,
    title TEXT NOT NULL,
    start_episode INTEGER NOT NULL DEFAULT 1,
    lang_pref TEXT NOT NULL DEFAULT 'ask',
    include_kw TEXT NOT NULL DEFAULT '',
    exclude_kw TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1,
    last_fetch_at TEXT,
    created_at TEXT NOT NULL,
    backup_rss_url TEXT NOT NULL DEFAULT '',
    total_episodes INTEGER,
    bgm_id INTEGER,
    last_push_at TEXT,
    last_slack_notified TEXT,
    gap_notified TEXT DEFAULT ''
);
CREATE TABLE IF NOT EXISTS pushed_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subscription_id INTEGER NOT NULL,
    episode INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    lang TEXT NOT NULL DEFAULT '',
    magnet TEXT NOT NULL,
    title TEXT NOT NULL,
    link TEXT NOT NULL DEFAULT '',
    pushed_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_pushed_magnet ON pushed_items(magnet);
CREATE INDEX IF NOT EXISTS idx_pushed_link ON pushed_items(link);
CREATE INDEX IF NOT EXISTS idx_pushed_sub_ep ON pushed_items(subscription_id, episode);
CREATE TABLE IF NOT EXISTS episode_prefs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subscription_id INTEGER NOT NULL,
    episode INTEGER NOT NULL,
    chosen_lang TEXT NOT NULL DEFAULT '',
    chosen_version INTEGER,
    decided_at TEXT NOT NULL,
    UNIQUE(subscription_id, episode)
);
CREATE TABLE IF NOT EXISTS pending_decisions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subscription_id INTEGER NOT NULL,
    episode INTEGER NOT NULL,
    candidates_json TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'fresh',
    created_at TEXT NOT NULL,
    UNIQUE(subscription_id, episode)
);
CREATE TABLE IF NOT EXISTS conversations (
    chat_id INTEGER PRIMARY KEY,
    data TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
"#;

pub fn open(path: &str) -> Result<Connection> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let conn = Connection::open(path).context("打开数据库失败")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    Ok(conn)
}

pub fn migrate(db: &Db) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute_batch(SCHEMA)?;
    Ok(())
}

fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

// ---------------- meta ----------------

pub fn meta_get(db: &Db, key: &str) -> Option<String> {
    let conn = db.lock().unwrap();
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| {
        r.get(0)
    })
    .ok()
}

pub fn meta_set(db: &Db, key: &str, value: &str) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// 读取开关类 meta（"1" 为真）
pub fn meta_bool(db: &Db, key: &str, default: bool) -> bool {
    meta_get(db, key)
        .map(|s| s == "1")
        .unwrap_or(default)
}

pub fn meta_int(db: &Db, key: &str, default: i64) -> i64 {
    meta_get(db, key)
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(default)
}

// ---------------- subscriptions ----------------

pub fn add_subscription(db: &Db, rss_url: &str, title: &str) -> Result<i64> {
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO subscriptions (rss_url, title, created_at) VALUES (?1, ?2, ?3)",
        params![rss_url, title, now()],
    )?;
    Ok(conn.last_insert_rowid())
}

fn row_to_sub(r: &rusqlite::Row) -> rusqlite::Result<SubRow> {
    Ok(SubRow {
        id: r.get(0)?,
        rss_url: r.get(1)?,
        title: r.get(2)?,
        start_episode: r.get(3)?,
        lang_pref: r.get(4)?,
        include_kw: r.get(5)?,
        exclude_kw: r.get(6)?,
        enabled: r.get::<_, i64>(7)? != 0,
        last_fetch_at: r.get(8)?,
        created_at: r.get(9)?,
        backup_rss_url: r.get(10)?,
        total_episodes: r.get(11)?,
        bgm_id: r.get(12)?,
        last_push_at: r.get(13)?,
        last_slack_notified: r.get(14)?,
        gap_notified: r.get(15)?,
    })
}

const SUB_COLS: &str = "id, rss_url, title, start_episode, lang_pref, include_kw, exclude_kw, enabled, last_fetch_at, created_at, backup_rss_url, total_episodes, bgm_id, last_push_at, last_slack_notified, gap_notified";

pub fn list_subscriptions(db: &Db) -> Result<Vec<SubRow>> {
    let conn = db.lock().unwrap();
    let mut stmt =
        conn.prepare(&format!("SELECT {SUB_COLS} FROM subscriptions ORDER BY id"))?;
    let rows = stmt
        .query_map([], row_to_sub)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get_subscription(db: &Db, id: i64) -> Result<Option<SubRow>> {
    let conn = db.lock().unwrap();
    let mut stmt =
        conn.prepare(&format!("SELECT {SUB_COLS} FROM subscriptions WHERE id = ?1"))?;
    let mut rows = stmt.query_map(params![id], row_to_sub)?;
    match rows.next() {
        Some(r) => Ok(Some(r?)),
        None => Ok(None),
    }
}

pub fn set_sub_start(db: &Db, id: i64, start: i64) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "UPDATE subscriptions SET start_episode = ?1 WHERE id = ?2",
        params![start, id],
    )?;
    Ok(())
}

pub fn set_sub_lang(db: &Db, id: i64, lang: &str) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "UPDATE subscriptions SET lang_pref = ?1 WHERE id = ?2",
        params![lang, id],
    )?;
    Ok(())
}

pub fn set_sub_kw(db: &Db, id: i64, include: &str, exclude: &str) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "UPDATE subscriptions SET include_kw = ?1, exclude_kw = ?2 WHERE id = ?3",
        params![include, exclude, id],
    )?;
    Ok(())
}

pub fn set_sub_enabled(db: &Db, id: i64, enabled: bool) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "UPDATE subscriptions SET enabled = ?1 WHERE id = ?2",
        params![enabled as i64, id],
    )?;
    Ok(())
}

pub fn touch_fetch(db: &Db, id: i64) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "UPDATE subscriptions SET last_fetch_at = ?1 WHERE id = ?2",
        params![now(), id],
    )?;
    Ok(())
}

pub fn delete_subscription(db: &Db, id: i64) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute("DELETE FROM subscriptions WHERE id = ?1", params![id])?;
    conn.execute("DELETE FROM pushed_items WHERE subscription_id = ?1", params![id])?;
    conn.execute("DELETE FROM episode_prefs WHERE subscription_id = ?1", params![id])?;
    conn.execute("DELETE FROM pending_decisions WHERE subscription_id = ?1", params![id])?;
    Ok(())
}

pub fn set_sub_backup(db: &Db, id: i64, url: Option<&str>) -> Result<()> {
    let conn = db.lock().unwrap();
    match url {
        Some(u) => conn.execute(
            "UPDATE subscriptions SET backup_rss_url = ?1 WHERE id = ?2",
            params![u, id],
        )?,
        None => conn.execute(
            "UPDATE subscriptions SET backup_rss_url = '' WHERE id = ?1",
            params![id],
        )?,
    };
    Ok(())
}

pub fn set_sub_total(db: &Db, id: i64, total: Option<i64>) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "UPDATE subscriptions SET total_episodes = ?1 WHERE id = ?2",
        params![total, id],
    )?;
    Ok(())
}

pub fn set_sub_bgm(db: &Db, id: i64, bgm_id: Option<i64>) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "UPDATE subscriptions SET bgm_id = ?1 WHERE id = ?2",
        params![bgm_id, id],
    )?;
    Ok(())
}

pub fn update_total_episodes(db: &Db, id: i64, total: i64) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "UPDATE subscriptions SET total_episodes = ?1 WHERE id = ?2",
        params![total, id],
    )?;
    Ok(())
}

pub fn list_subs_with_bgm(db: &Db) -> Result<Vec<(i64, i64)>> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, bgm_id FROM subscriptions WHERE bgm_id IS NOT NULL",
    )?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn set_sub_last_push(db: &Db, id: i64) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "UPDATE subscriptions SET last_push_at = ?1 WHERE id = ?2",
        params![now(), id],
    )?;
    Ok(())
}

pub fn set_sub_last_slack_notified(db: &Db, id: i64) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "UPDATE subscriptions SET last_slack_notified = ?1 WHERE id = ?2",
        params![now(), id],
    )?;
    Ok(())
}

pub fn set_sub_gap_notified(db: &Db, id: i64, sig: &str) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "UPDATE subscriptions SET gap_notified = ?1 WHERE id = ?2",
        params![sig, id],
    )?;
    Ok(())
}

// ---------------- pushed items ----------------

/// 该订阅+该集已推送过的 (版本, 语言)
pub fn pushed_for_episode(db: &Db, sub_id: i64, ep: i64) -> Result<Vec<(i64, String)>> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT version, lang FROM pushed_items WHERE subscription_id = ?1 AND episode = ?2",
    )?;
    let rows = stmt
        .query_map(params![sub_id, ep], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn is_pushed(db: &Db, magnet: &str) -> Result<bool> {
    let conn = db.lock().unwrap();
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pushed_items WHERE magnet = ?1",
        params![magnet],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// 按 RSS 条目标识（页面链接）去重，避免重复解析 .torrent
pub fn is_pushed_link(db: &Db, link: &str) -> Result<bool> {
    if link.is_empty() {
        return Ok(false);
    }
    let conn = db.lock().unwrap();
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pushed_items WHERE link = ?1",
        params![link],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

#[allow(clippy::too_many_arguments)]
pub fn insert_pushed(
    db: &Db,
    sub_id: i64,
    ep: i64,
    version: i64,
    lang: &str,
    magnet: &str,
    title: &str,
    link: &str,
) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO pushed_items (subscription_id, episode, version, lang, magnet, title, link, pushed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![sub_id, ep, version, lang, magnet, title, link, now()],
    )?;
    Ok(())
}

pub fn count_pushed(db: &Db) -> Result<i64> {
    let conn = db.lock().unwrap();
    Ok(conn.query_row("SELECT COUNT(*) FROM pushed_items", [], |r| {
        r.get(0)
    })?)
}

/// 某订阅已推送的不重复集数（用于自动禁用判断）
pub fn count_pushed_for_sub(db: &Db, sub_id: i64) -> Result<i64> {
    let conn = db.lock().unwrap();
    Ok(conn.query_row(
        "SELECT COUNT(DISTINCT episode) FROM pushed_items WHERE subscription_id = ?1",
        params![sub_id],
        |r| r.get(0),
    )?)
}

/// 某订阅已推送的集号列表（用于遗漏检测）
pub fn pushed_episodes(db: &Db, sub_id: i64) -> Result<Vec<i64>> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT DISTINCT episode FROM pushed_items WHERE subscription_id = ?1 ORDER BY episode",
    )?;
    let rows = stmt
        .query_map(params![sub_id], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

// ---------------- episode prefs ----------------

/// (语言, 版本可选) 版本为 None 表示"该集忽略"
pub fn get_episode_pref(db: &Db, sub_id: i64, ep: i64) -> Result<Option<(String, Option<i64>)>> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT chosen_lang, chosen_version FROM episode_prefs WHERE subscription_id = ?1 AND episode = ?2",
    )?;
    let mut rows = stmt.query_map(params![sub_id, ep], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?))
    })?;
    match rows.next() {
        Some(r) => Ok(Some(r?)),
        None => Ok(None),
    }
}

pub fn save_episode_pref(
    db: &Db,
    sub_id: i64,
    ep: i64,
    lang: &str,
    version: Option<i64>,
) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO episode_prefs (subscription_id, episode, chosen_lang, chosen_version, decided_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(subscription_id, episode) DO UPDATE SET
           chosen_lang = excluded.chosen_lang,
           chosen_version = excluded.chosen_version,
           decided_at = excluded.decided_at",
        params![sub_id, ep, lang, version, now()],
    )?;
    Ok(())
}

// ---------------- pending decisions ----------------

pub fn pending_exists(db: &Db, sub_id: i64, ep: i64) -> Result<bool> {
    let conn = db.lock().unwrap();
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pending_decisions WHERE subscription_id = ?1 AND episode = ?2",
        params![sub_id, ep],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

pub fn save_pending(
    db: &Db,
    sub_id: i64,
    ep: i64,
    kind: &str,
    candidates_json: &str,
) -> Result<i64> {
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO pending_decisions (subscription_id, episode, candidates_json, kind, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(subscription_id, episode) DO UPDATE SET
           candidates_json = excluded.candidates_json,
           kind = excluded.kind,
           created_at = excluded.created_at",
        params![sub_id, ep, candidates_json, kind, now()],
    )?;
    let id = conn.query_row(
        "SELECT id FROM pending_decisions WHERE subscription_id = ?1 AND episode = ?2",
        params![sub_id, ep],
        |r| r.get(0),
    )?;
    Ok(id)
}

pub fn get_pending(db: &Db, id: i64) -> Result<Option<Pending>> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, subscription_id, episode, candidates_json, kind FROM pending_decisions WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], |r| {
        Ok(Pending {
            id: r.get(0)?,
            subscription_id: r.get(1)?,
            episode: r.get(2)?,
            candidates_json: r.get(3)?,
            kind: r.get(4)?,
        })
    })?;
    match rows.next() {
        Some(r) => Ok(Some(r?)),
        None => Ok(None),
    }
}

pub fn delete_pending(db: &Db, id: i64) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute("DELETE FROM pending_decisions WHERE id = ?1", params![id])?;
    Ok(())
}

// ---------------- conversations ----------------

pub fn conv_get(db: &Db, chat_id: i64) -> Option<String> {
    let conn = db.lock().unwrap();
    conn.query_row(
        "SELECT data FROM conversations WHERE chat_id = ?1",
        params![chat_id],
        |r| r.get(0),
    )
    .ok()
}

pub fn conv_set(db: &Db, chat_id: i64, data: &str) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO conversations (chat_id, data, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(chat_id) DO UPDATE SET data = excluded.data, updated_at = excluded.updated_at",
        params![chat_id, data, now()],
    )?;
    Ok(())
}

pub fn conv_clear(db: &Db, chat_id: i64) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "DELETE FROM conversations WHERE chat_id = ?1",
        params![chat_id],
    )?;
    Ok(())
}

pub fn count_subscriptions(db: &Db) -> Result<i64> {
    let conn = db.lock().unwrap();
    Ok(conn.query_row("SELECT COUNT(*) FROM subscriptions", [], |r| {
        r.get(0)
    })?)
}
