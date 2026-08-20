use serde::{Deserialize, Serialize};

/// 字幕语言（简中 / 繁中 / 简繁双语 / 未知）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    Simp,
    Trad,
    Bilingual,
    Unknown,
}

impl Lang {
    pub fn label(&self) -> &'static str {
        match self {
            Lang::Simp => "简中",
            Lang::Trad => "繁中",
            Lang::Bilingual => "简繁",
            Lang::Unknown => "未知",
        }
    }

    pub fn from_label(s: &str) -> Lang {
        match s {
            "繁中" => Lang::Trad,
            "简繁" => Lang::Bilingual,
            "简中" => Lang::Simp,
            _ => Lang::Unknown,
        }
    }
}

/// Mikan 标题解析结果
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ParsedTitle {
    pub fansub: Option<String>,
    pub anime: String,
    pub episode: Option<u32>,
    pub version: u32,
    pub lang: Lang,
    pub quality: Option<String>,
    pub codec: Option<String>,
    pub source: Option<String>,
    pub is_collection: bool,
    pub is_special: bool,
    pub is_half: bool,
    pub raw: String,
}

/// RSS 条目
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RssItem {
    pub title: String,
    pub link: String,
    pub magnet: Option<String>,
    pub enclosure_url: Option<String>,
    pub enclosure_type: Option<String>,
    pub guid: String,
}

/// 通过过滤、等待推送/询问的候选
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub title: String,
    pub magnet: String,
    pub fansub: Option<String>,
    pub episode: u32,
    pub version: u32,
    pub lang: String,
    pub quality: Option<String>,
    pub codec: Option<String>,
    pub source: Option<String>,
    pub link: String,
}

impl Candidate {
    pub fn lang_label(&self) -> Lang {
        Lang::from_label(&self.lang)
    }
}

/// 订阅行
#[derive(Debug, Clone)]
pub struct SubRow {
    pub id: i64,
    pub rss_url: String,
    pub title: String,
    pub start_episode: i64,
    pub lang_pref: String,
    pub include_kw: String,
    pub exclude_kw: String,
    pub enabled: bool,
    pub last_fetch_at: Option<String>,
    pub created_at: String,
    pub backup_rss_url: String,
    pub total_episodes: Option<i64>,
    pub bgm_id: Option<i64>,
    pub last_push_at: Option<String>,
    pub last_slack_notified: Option<String>,
    pub gap_notified: Option<String>,
    pub poster_url: Option<String>,
}

/// 待用户决策的冲突
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Pending {
    pub id: i64,
    pub subscription_id: i64,
    pub episode: i64,
    pub candidates_json: String,
    pub kind: String,
}

/// 集数统一格式：01 02 03 ...（<100 补零）
pub fn fmt_episode(ep: u32) -> String {
    if ep < 100 {
        format!("{ep:02}")
    } else {
        ep.to_string()
    }
}
