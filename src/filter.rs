use crate::models::{Candidate, Lang};

/// include / exclude 关键词匹配（逗号分隔；include 任一命中即可，exclude 任一命中即排除）
pub fn matches_keywords(title: &str, include: &str, exclude: &str) -> bool {
    let title_lower = title.to_lowercase();

    if !exclude.trim().is_empty() {
        for kw in exclude.split(',') {
            let kw = kw.trim().to_lowercase();
            if !kw.is_empty() && title_lower.contains(&kw) {
                return false;
            }
        }
    }

    if !include.trim().is_empty() {
        let mut any = false;
        for kw in include.split(',') {
            let kw = kw.trim().to_lowercase();
            if !kw.is_empty() && title_lower.contains(&kw) {
                any = true;
                break;
            }
        }
        if !any {
            return false;
        }
    }
    true
}

/// 按订阅的简繁偏好过滤候选，返回匹配下标。
/// 语义：简中=排除明确繁中，繁中=排除明确简中；"未知"语言始终保留（无法判断就不误杀），
/// 否则像 bangumi.moe 这种标题不带简繁标记的源会被全部过滤掉。
pub fn filter_by_lang(cands: &[Candidate], lang_pref: &str) -> Vec<usize> {
    match lang_pref {
        "简中" => cands
            .iter()
            .enumerate()
            .filter(|(_, c)| !matches!(c.lang_label(), Lang::Trad))
            .map(|(i, _)| i)
            .collect(),
        "繁中" => cands
            .iter()
            .enumerate()
            .filter(|(_, c)| !matches!(c.lang_label(), Lang::Simp))
            .map(|(i, _)| i)
            .collect(),
        _ => (0..cands.len()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Candidate;

    fn cand(lang: &str) -> Candidate {
        Candidate {
            title: "t".into(),
            magnet: "magnet:?xt=urn:btih:0000000000000000000000000000000000000000".into(),
            fansub: None,
            episode: 1,
            version: 1,
            lang: lang.into(),
            quality: None,
            codec: None,
            source: None,
            link: String::new(),
        }
    }

    #[test]
    fn keyword_exclude() {
        assert!(!matches_keywords("[xx] 番 - 01 (生肉)", "", "生肉"));
        assert!(matches_keywords("[xx] 番 - 01 (简中)", "", "生肉"));
    }

    #[test]
    fn keyword_include() {
        assert!(matches_keywords("[xx] 番 - 01 (1080P)", "1080P", ""));
        assert!(!matches_keywords("[xx] 番 - 01 (720P)", "1080P", ""));
    }

    #[test]
    fn lang_pref_keeps_unknown() {
        // 简中偏好：ABEMA/CR/Baha 标题无简繁标记(未知) 时不能被过滤光
        let cands = vec![cand("未知"), cand("未知"), cand("未知")];
        let idxs = filter_by_lang(&cands, "简中");
        assert_eq!(idxs.len(), 3);
    }

    #[test]
    fn lang_pref_excludes_opposite_only() {
        let cands = vec![cand("简中"), cand("繁中"), cand("未知"), cand("简繁")];
        let idxs = filter_by_lang(&cands, "简中");
        assert_eq!(idxs, vec![0, 2, 3]);
        let idxs = filter_by_lang(&cands, "繁中");
        assert_eq!(idxs, vec![1, 2, 3]);
    }
}
