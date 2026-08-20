use once_cell::sync::Lazy;
use regex::Regex;

use crate::models::{Lang, ParsedTitle};

static RE_FANSUB: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\[([^\]]+)\]\s*").unwrap());

// 第07话 / 第07集 / 第7话 / 第07話
static RE_EP_CN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"第\s*(\d{1,3})\s*[话話集]").unwrap());

// 第07.5话
static RE_EP_CN_HALF: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"第\s*(\d{1,3})\.(\d+)\s*[话話集]").unwrap());

// S01E07 / S1E07 / S01 EP07
static RE_EP_SEASON: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:^|[^\d])\s*[Ss]\s*(\d{1,2})\s*[Ee]\s*(\d{1,3})").unwrap());

// - 07 / - 07v2 / - 07.5 （边界校验在代码里做：后一个字符须为空格/括号/行尾，避免误吞 2024 这类年份）
static RE_EP_DASH: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"-\s*(\d{1,3})(?:\.(\d+))?(?:\s*[vV]\s*(\d+))?").unwrap());

// 任意位置 vN 形式（含 07话v2 / 07v2），用于补版本号
static RE_EP_VER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(\d{1,3})\s*[话話集]?\s*[vV]\s*(\d+)").unwrap());

// 合集：01-12 / 合集 / 全12话 / 全集
static RE_COLLECTION: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\d{1,3}\s*-\s*\d{1,3}|合集|全\s*\d+\s*[话話集]|全集").unwrap()
});

// 特典/纯曲目类：NC OP ED SP PV 预告 特别篇 特典 菜单 番外 剧场版 OVA 总集
static RE_SPECIAL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b(?:NC|OP|ED|SP|PV|CM)\b|预告|特别篇|特典|菜单|番外|剧场版|OVA|总集").unwrap()
});

static RE_QUALITY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)(4K|2160P|1080P|720P|480P|1080|720)").unwrap());

static RE_CODEC: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)(H\.?265|HEVC|x265|H\.?264|AVC|x264|AV1)").unwrap());

// 语言关键词，必须按 双语 -> 繁中 -> 简中 -> 裸中文 顺序判断
const BILINGUAL: &[&str] = &[
    "简繁",
    "双语",
    "简繁日",
    "中日双语",
    "简繁内封",
    "简繁外挂",
    "简繁内嵌",
];
const TRAD: &[&str] = &[
    "繁中",
    "繁體中文",
    "繁體",
    "繁体中文",
    "繁体",
    "繁日",
    "繁中内嵌",
    "繁中外挂",
    "CHT",
];
const SIMP: &[&str] = &[
    "简中",
    "简体中文",
    "简体",
    "简日",
    "简中内嵌",
    "简中内挂",
    "简中外挂",
    "中文字幕",
    "中文",
    "国语",
    "CHS",
];

fn detect_lang(s: &str) -> Lang {
    for kw in BILINGUAL {
        if s.contains(kw) {
            return Lang::Bilingual;
        }
    }
    for kw in TRAD {
        if s.contains(kw) {
            return Lang::Trad;
        }
    }
    for kw in SIMP {
        if s.contains(kw) {
            return Lang::Simp;
        }
    }
    Lang::Unknown
}

fn normalize_quality(raw: &str) -> String {
    let upper = raw.to_uppercase();
    if upper.contains("4K") || upper.contains("2160") {
        "4K".into()
    } else if upper.contains("1080") {
        "1080P".into()
    } else if upper.contains("720") {
        "720P".into()
    } else {
        "480P".into()
    }
}

fn normalize_codec(raw: &str) -> &'static str {
    let upper = raw.to_uppercase();
    if upper.contains("265") || upper.contains("HEVC") {
        "HEVC"
    } else if upper.contains("264") || upper.contains("AVC") {
        "x264"
    } else {
        "AV1"
    }
}

fn strip_trailing_paren(s: &str) -> String {
    let re = Regex::new(r"\s*[\(\[（【][^)）\]】]*[\)\]）】]\s*$").unwrap();
    re.replace(s, "").to_string()
}

/// 解析 Mikan 风格的标题，例如：
/// `[Lilith-Raws] 败犬女主太多了！ - 07v2 (Baha 1080P 简中 内嵌)`
pub fn parse_title(raw: &str) -> ParsedTitle {
    let raw = raw.trim();
    let mut fansub = None;
    let mut rest = raw;

    if let Some(cap) = RE_FANSUB.captures(raw) {
        fansub = Some(cap.get(1).unwrap().as_str().to_string());
        rest = raw[cap.get(0).unwrap().end()..].trim();
    }

    // 1) 合集优先：整包跳过，不解析单集
    if RE_COLLECTION.is_match(rest) {
        return ParsedTitle {
            fansub,
            anime: strip_trailing_paren(rest),
            episode: None,
            version: 1,
            lang: detect_lang(rest),
            quality: RE_QUALITY
                .find(rest)
                .map(|m| normalize_quality(m.as_str())),
            codec: RE_CODEC
                .find(rest)
                .map(|m| normalize_codec(m.as_str()).to_string()),
            is_collection: true,
            is_special: RE_SPECIAL.is_match(rest),
            is_half: false,
            raw: raw.to_string(),
        };
    }

    let is_special = RE_SPECIAL.is_match(rest);

    // 2) 集数：第X.Y话 > 第X话 > SxxEyy > - XX
    let (episode, ep_pos, dash_version, is_half) = if let Some(cap) = RE_EP_CN_HALF.captures(rest) {
        let m = cap.get(0).unwrap();
        let half = cap.get(2).map(|g| g.as_str() != "0").unwrap_or(false);
        (
            cap.get(1).unwrap().as_str().parse::<u32>().ok(),
            Some(m.start()),
            None,
            half,
        )
    } else if let Some(cap) = RE_EP_CN.captures(rest) {
        let m = cap.get(0).unwrap();
        (
            cap.get(1).unwrap().as_str().parse::<u32>().ok(),
            Some(m.start()),
            None,
            false,
        )
    } else if let Some(cap) = RE_EP_SEASON.captures(rest) {
        let m = cap.get(0).unwrap();
        (
            cap.get(2).unwrap().as_str().parse::<u32>().ok(),
            Some(m.start()),
            None,
            false,
        )
    } else {
        let mut found = None;
        for cap in RE_EP_DASH.captures_iter(rest) {
            let m = cap.get(0).unwrap();
            // 后一个字符必须是 空格/括号/行尾，否则视为更大数字的一部分（如 2024）
            let next = rest[m.end()..].chars().next();
            if !matches!(next, None | Some(' ' | '\t' | '(' | '[' | '（' | '【')) {
                continue;
            }
            let ep = cap.get(1).and_then(|g| g.as_str().parse::<u32>().ok());
            let version = cap.get(3).and_then(|g| g.as_str().parse::<u32>().ok());
            let half = cap.get(2).map(|g| g.as_str() != "0").unwrap_or(false);
            found = Some((ep, Some(m.start()), version, half));
        }
            found.unwrap_or_default()
    };

    // 3) 版本号：dash 捕获优先，否则找任意位置 vN（须与集号一致）
    let version = dash_version.or_else(|| {
        let ep = episode?;
        RE_EP_VER.captures(rest).and_then(|cap| {
            let n = cap.get(1)?.as_str().parse::<u32>().ok()?;
            if n == ep {
                cap.get(2)?.as_str().parse::<u32>().ok()
            } else {
                None
            }
        })
    }).unwrap_or(1);

    // 4) 番名 = 集数匹配点之前的文本，去掉尾部符号
    let anime = match ep_pos {
        Some(pos) => {
            let head = rest[..pos].trim();
            let head = head.trim_end_matches('-').trim_end_matches('–').trim_end();
            strip_trailing_paren(head)
        }
        None => strip_trailing_paren(rest),
    };

    let quality = RE_QUALITY.find(rest).map(|m| normalize_quality(m.as_str()));
    let codec = RE_CODEC
        .find(rest)
        .map(|m| normalize_codec(m.as_str()).to_string());

    ParsedTitle {
        fansub,
        anime,
        episode,
        version,
        lang: detect_lang(rest),
        quality,
        codec,
        is_collection: false,
        is_special,
        is_half,
        raw: raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(p: &ParsedTitle) -> Option<u32> {
        p.episode
    }

    #[test]
    fn parse_simple() {
        let p = parse_title("[Lilith-Raws] 败犬女主太多了！ - 07 (Baha 1080P 简中 内嵌)");
        assert_eq!(ep(&p), Some(7));
        assert_eq!(p.version, 1);
        assert_eq!(p.lang, Lang::Simp);
        assert_eq!(p.quality.as_deref(), Some("1080P"));
        assert_eq!(p.anime, "败犬女主太多了！");
        assert_eq!(p.fansub.as_deref(), Some("Lilith-Raws"));
    }

    #[test]
    fn parse_v2() {
        let p = parse_title("[A-Raws] 我的推是反派大小姐 - 07v2 (1080P HEVC)");
        assert_eq!(ep(&p), Some(7));
        assert_eq!(p.version, 2);
        assert_eq!(p.lang, Lang::Unknown);
        assert_eq!(p.codec.as_deref(), Some("HEVC"));
    }

    #[test]
    fn parse_bilingual() {
        let p = parse_title("[桜都字幕组] 我的推是反派大小姐 - 07 (720P 简繁日双语)");
        assert_eq!(ep(&p), Some(7));
        assert_eq!(p.lang, Lang::Bilingual);
    }

    #[test]
    fn parse_trad() {
        let p = parse_title("[MingY] 葬送的芙莉莲 - 第08话 (1080P 繁體中文)");
        assert_eq!(ep(&p), Some(8));
        assert_eq!(p.lang, Lang::Trad);
    }

    #[test]
    fn parse_season_ep() {
        let p = parse_title("[xx] 间谍过家家 S01E07 (1080P 简中)");
        assert_eq!(ep(&p), Some(7));
    }

    #[test]
    fn parse_half_episode() {
        let p = parse_title("[xxx] 某番 - 07.5 (1080P)");
        assert_eq!(ep(&p), Some(7));
        assert!(p.is_half);
        assert!(!p.is_collection);
    }

    #[test]
    fn parse_cn_half() {
        let p = parse_title("[xxx] 某番 第13.5话 (1080P 简中)");
        assert_eq!(ep(&p), Some(13));
        assert!(p.is_half);
    }

    #[test]
    fn parse_collection() {
        let p = parse_title("[xxx] 某番 - 01-12 合集 (1080P)");
        assert!(p.is_collection);
        assert_eq!(ep(&p), None);
    }

    #[test]
    fn parse_special() {
        let p = parse_title("[xxx] 某番 - NC (1080P)");
        assert!(p.is_special);
        assert_eq!(ep(&p), None);
    }

    #[test]
    fn parse_year_not_episode() {
        let p = parse_title("[rec] 某老番 (2024 BDRip 1080P 简中)");
        assert_eq!(ep(&p), None);
    }

    #[test]
    fn parse_cn_with_v2() {
        let p = parse_title("[sub] 某番 第07话v2 (1080P 简体)");
        assert_eq!(ep(&p), Some(7));
        assert_eq!(p.version, 2);
    }

    #[test]
    fn parse_no_episode() {
        let p = parse_title("[sub] 某番全集 (1080P)");
        assert!(p.is_collection);
    }
}
