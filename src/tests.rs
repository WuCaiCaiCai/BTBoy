//! 基于用户提供的真实 bangumi.moe RSS 数据的管线测试
#![cfg(test)]

use std::sync::{Arc, Mutex};

use teloxide::Bot;

use crate::models::{Candidate, SubRow};
use crate::{config, db, parser, rss, scheduler};

const FIXTURE: &str = include_str!("../tests/fixtures/bangumi_moe_sample.xml");

fn test_state() -> Arc<crate::AppState> {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    let db = Arc::new(Mutex::new(conn));
    db::migrate(&db).unwrap();
    let config = config::Config {
        bot_token: "test".into(),
        admin_id: Some(1),
        channel_id: None,
        fetch_interval_min: 5,
        db_path: ":memory:".into(),
        log_level: "warn".into(),
        tmdb_api_key: None,
    };
    let http = reqwest::Client::builder().user_agent("test").build().unwrap();
    let bot = Bot::new("test");
    Arc::new(crate::AppState {
        config,
        db,
        http,
        bot,
        started: std::time::Instant::now(),
        poster_cache: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
    })
}

#[test]
fn parse_real_fixture_xml() {
    let items = rss::parse_rss_bytes(FIXTURE.as_bytes()).unwrap();
    assert_eq!(items.len(), 3);
    // 三条都是第07话，来源各不相同
    let mut sources = vec![];
    for it in &items {
        let p = parser::parse_title(&it.title);
        assert_eq!(p.episode, Some(7));
        assert_eq!(p.anime, "再見菈菈 / Sayonara Lara");
        assert_eq!(p.fansub.as_deref(), Some("黒ネズミたち"));
        sources.push(p.source.clone().unwrap());
    }
    assert_eq!(sources, vec!["ABEMA".to_string(), "CR".to_string(), "Baha".to_string()]);
}

#[test]
fn enclosure_is_torrent_not_magnet() {
    let items = rss::parse_rss_bytes(FIXTURE.as_bytes()).unwrap();
    for it in &items {
        assert!(it.magnet.is_none());
        let u = it.enclosure_url.as_ref().unwrap();
        assert!(rss::looks_like_torrent(u));
    }
}

#[test]
fn full_01_07_titles_parse() {
    // 覆盖用户 feed 中 01-07 共 21 条真实标题（ep 生成 + 三来源），验证统一识别
    let sources = [("ABEMA", "MKV"), ("CR", "MKV"), ("Baha", "MP4")];
    for ep in 1..=7 {
        for (src, ext) in sources {
            let title = format!(
                "[黒ネズミたち] 再見菈菈 / Sayonara Lara - {ep:02} ({src} 1920x1080 AVC AAC {ext})"
            );
            let p = parser::parse_title(&title);
            assert_eq!(p.episode, Some(ep), "title={title}");
            assert_eq!(p.source.as_deref(), Some(src), "title={title}");
            assert_eq!(p.anime, "再見菈菈 / Sayonara Lara", "title={title}");
        }
    }
}

#[tokio::test]
async fn pipeline_detects_3_sources_and_asks() {
    let state = test_state();
    let sub_id = db::add_subscription(&state.db, "http://test/feed", "再見菈菈 / Sayonara Lara").unwrap();
    db::set_sub_lang(&state.db, sub_id, "ask").unwrap();
    let sub: SubRow = db::get_subscription(&state.db, sub_id).unwrap().unwrap();

    // 用真实 fixture 走完整 collect 管线；模拟磁力已解析（rin.pr.com 离线，无法真下载）
    let mut items = rss::parse_rss_bytes(FIXTURE.as_bytes()).unwrap();
    for it in &mut items {
        let hash: String = sha1_digest(it.title.as_bytes());
        it.magnet = Some(format!("magnet:?xt=urn:btih:{hash}"));
    }

    let candidates = scheduler::collect_candidates(&state, &sub, &items, false, "http://test/feed")
        .await
        .unwrap();
    assert_eq!(candidates.len(), 3);
    let mut sources: Vec<&str> = candidates.iter().map(|c| c.source.as_deref().unwrap()).collect();
    sources.sort();
    assert_eq!(sources, vec!["ABEMA", "Baha", "CR"]);

    // 3 个候选且 lang_pref=ask → 应触发询问让用户选
    match scheduler::decide_fresh(&state, &sub, 7, &candidates).unwrap() {
        scheduler::Decision::Ask => {}
        other => panic!("期望 Ask，实际 {other:?}"),
    }

    // 每条候选都带着可用的磁力（下游能直接喂光鸭）
    for c in &candidates {
        assert!(c.magnet.starts_with("magnet:?xt=urn:btih:"));
    }
}

#[test]
fn torrent_infohash_is_sha1_of_info() {
    // 合成一个合法 .torrent，验证 infohash = SHA1(bencode(info)) 的磁力生成正确
    let data = b"d4:infod4:name1:a6:lengthi5ee".to_vec();
    let magnet = rss::torrent_to_magnet(&data).unwrap();
    let expected = sha1_digest(b"d4:name1:a6:lengthi5ee");
    assert_eq!(magnet, format!("magnet:?xt=urn:btih:{expected}"));
}

fn sha1_digest(bytes: &[u8]) -> String {
    use sha1::Digest;
    sha1::Sha1::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn index_mapping_survives_deletion() {
    let state = test_state();
    let a = db::add_subscription(&state.db, "http://a", "番A").unwrap();
    let b = db::add_subscription(&state.db, "http://b", "番B").unwrap();
    let c = db::add_subscription(&state.db, "http://c", "番C").unwrap();
    assert_eq!(db::resolve_sub_by_index(&state.db, 1).unwrap(), Some(a));
    assert_eq!(db::resolve_sub_by_index(&state.db, 2).unwrap(), Some(b));
    assert_eq!(db::resolve_sub_by_index(&state.db, 3).unwrap(), Some(c));
    assert_eq!(db::resolve_sub_by_index(&state.db, 4).unwrap(), None);

    // 删除第 1 个(序号1)后，序号重排：原 b 变 1，原 c 变 2
    db::delete_subscription(&state.db, a).unwrap();
    assert_eq!(db::resolve_sub_by_index(&state.db, 1).unwrap(), Some(b));
    assert_eq!(db::resolve_sub_by_index(&state.db, 2).unwrap(), Some(c));
    assert_eq!(db::resolve_sub_by_index(&state.db, 3).unwrap(), None);
}

// 让 unused 警告不出现：Candidate 在测试里用到即消除，若未用到则此辅助仅作占位
#[allow(dead_code)]
fn _touch(c: Candidate) {
    let _ = c.link;
}
