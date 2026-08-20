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

/// 按订阅的简繁偏好过滤候选，返回匹配下标
pub fn filter_by_lang(cands: &[Candidate], lang_pref: &str) -> Vec<usize> {
    let target = match lang_pref {
        "简中" => Lang::Simp,
        "繁中" => Lang::Trad,
        "简繁" => Lang::Bilingual,
        _ => return (0..cands.len()).collect(),
    };
    cands
        .iter()
        .enumerate()
        .filter(|(_, c)| c.lang_label() == target)
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
