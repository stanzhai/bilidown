//! Port of `test/test_episode_list_parsers.py` (flat-list episode
//! parsers; `TestAudioParser` is covered by the audio module's own
//! tests).

use serde_json::{json, Value};

use parsers::episode::list_parsers::{
    parse_favlist, parse_history, parse_list, parse_popular, parse_space, parse_watch_later,
};
use parsers::tree::{Attribute, EpisodeData};

fn space_data() -> Value {
    json!({
        "list": {"tlist": {}, "vlist": [
            {"aid": 1, "bvid": "BV1", "pic": "https://i.hdslb.com/bfs/archive/1.jpg",
             "created": 1700000000, "description": "投稿A描述", "duration": 90,
             "is_charging_arc": false, "is_lesson_video": false, "is_union_video": false,
             "mid": 123, "title": "投稿A"},
            {"aid": 2, "bvid": "BV2", "pic": "https://i.hdslb.com/bfs/archive/2.jpg",
             "created": 1700000100, "description": "充电视频描述", "duration": 120,
             "is_charging_arc": true, "is_lesson_video": false, "is_union_video": false,
             "mid": 123, "title": "充电视频"}
        ]},
        "info": {"mid": 123, "name": "某UP主", "uname": "某UP主"}
    })
}

#[test]
fn space_parser_basic() {
    let data = EpisodeData::default();
    let info = space_data();
    let node = parse_space(&data, &info);

    assert_eq!(node.number, parsers::tree::Number::Text("User Uploads".into()));
    assert_eq!(node.title, "某UP主");
    assert!(node.has_attribute(Attribute::TREE_NODE_BIT));
    assert_eq!(node.count(), 2);

    let first = node.child(0);
    assert_eq!(first.title, "投稿A");
    assert_eq!(first.duration, 90);
    assert_eq!(first.url, "https://www.bilibili.com/video/BV1");
    assert_eq!(first.pubtime, 1700000000);
    assert!(first.has_attribute(Attribute::VIDEO_BIT));
    assert!(first.has_attribute(Attribute::SPACE_BIT));
    assert!(first.has_attribute(Attribute::NEED_PARSE_BIT));
    assert_eq!(
        first.number,
        parsers::tree::Number::Int(1)
    );
    let second = node.child(1);
    assert_eq!(second.badge, "充电专属");

    let side = data.get_episode_data(&first.episode_id);
    assert_eq!(side["space_owner"], "某UP主");
    assert_eq!(side["space_owner_id"], 123);
}

#[test]
fn space_parser_empty_vlist() {
    let data = EpisodeData::default();
    let info = json!({"list": {"vlist": []}, "info": {"mid": 123, "name": "某UP主"}});
    let node = parse_space(&data, &info);
    assert_eq!(node.count(), 0);
}

#[test]
fn watch_later_parser_basic() {
    let data = EpisodeData::default();
    let info = json!({
        "list": [
            {"aid": 101, "bvid": "BV101", "cid": 1001, "pic": "https://i.hdslb.com/bfs/archive/w1.jpg",
             "title": "稍后再看视频", "duration": 300, "progress": 60, "add_at": 1700000000,
             "pubdate": 1699990000},
            {"aid": 102, "title": "稍后再看番剧", "pic": "https://i.hdslb.com/bfs/archive/w2.jpg",
             "duration": 1440, "progress": -1, "add_at": 1700000100, "pubdate": 1699990100,
             "bangumi": {"season_id": 999, "ep_id": 999, "title": "番剧标题"},
             "pgc_label": "第1话"}
        ]
    });
    let node = parse_watch_later(&data, &info);

    assert!(node.has_attribute(Attribute::TREE_NODE_BIT));
    assert_eq!(node.count(), 2);

    let first = node.child(0);
    assert!(first.has_attribute(Attribute::VIDEO_BIT));
    assert!(first.has_attribute(Attribute::WATCH_LATER_BIT));
    assert_eq!(first.favtime, 1700000000);
    assert_eq!(first.title, "稍后再看视频");

    let bangumi = node.child(1);
    assert!(bangumi.has_attribute(Attribute::BANGUMI_BIT));
    assert!(bangumi.has_attribute(Attribute::WATCH_LATER_BIT));
    assert_eq!(bangumi.badge, "第1话");
    assert_eq!(bangumi.ep_id, 999);

    let side = data.get_episode_data(&first.episode_id);
    assert_eq!(side["source_title"], "Watch Later");
    assert!(side.get("parent_title").is_none());
}

#[test]
fn watch_later_parser_none_list() {
    let data = EpisodeData::default();
    let info = json!({"list": null});
    let node = parse_watch_later(&data, &info);
    assert_eq!(node.count(), 0);
}

#[test]
fn favlist_parser_basic() {
    let data = EpisodeData::default();
    let info = json!({
        "info": {"id": 5, "fid": 5, "mid": 123, "title": "我的收藏夹",
                 "upper": {"mid": 77, "name": "夹主", "face": ""}},
        "medias": [
            {"id": 1, "bv_id": "BV101", "bvid": "BV101", "cover": "https://i.hdslb.com/bfs/archive/f1.jpg",
             "intro": "视频简介", "page": 1, "duration": 100, "fav_time": 1700000000,
             "pubtime": 1690000000, "title": "收藏的视频", "type": 2, "type_name": "视频",
             "upper": {"mid": 123, "name": "视频UP", "face": ""}},
            {"id": 2, "bv_id": "BV102", "bvid": "BV102", "cover": "https://i.hdslb.com/bfs/archive/f2.jpg",
             "intro": "分P视频简介", "page": 3, "duration": 600, "fav_time": 1700000001,
             "pubtime": 1690000001, "title": "分P视频", "type": 2, "type_name": "视频",
             "upper": {"mid": 123, "name": "视频UP", "face": ""}},
            {"id": 3, "cover": "https://i.hdslb.com/bfs/archive/f3.jpg",
             "intro": "简介", "duration": 1440, "fav_time": 1700000002,
             "pubtime": 1690000002, "title": "番剧", "type": 24, "type_name": "番剧",
             "upper": {"mid": 123, "name": "番剧UP", "face": ""},
             "ogv": {"season_id": 888, "ep_id": 888, "type_name": "番剧"}}
        ]
    });
    let node = parse_favlist(&data, &info);

    assert_eq!(node.title, "我的收藏夹");
    assert_eq!(node.count(), 3);

    let video = node.child(0);
    assert_eq!(video.title, "收藏的视频");
    assert_eq!(video.badge, "");
    assert!(video.has_attribute(Attribute::VIDEO_BIT));
    assert!(!video.has_attribute(Attribute::FAVORITE_WITH_MULTI_PART_VIDEO_BIT));

    let multi = node.child(1);
    assert_eq!(multi.badge, "分P");
    assert!(multi.has_attribute(Attribute::FAVORITE_WITH_MULTI_PART_VIDEO_BIT));

    let ogv = node.child(2);
    assert!(ogv.has_attribute(Attribute::BANGUMI_BIT));
    assert_eq!(ogv.title, "番剧 - 简介");
    assert_eq!(ogv.badge, "番剧");

    for i in 0..3 {
        assert!(node.child(i).has_attribute(Attribute::FAVLIST_BIT));
    }

    let side = data.get_episode_data(&video.episode_id);
    assert_eq!(side["favorites_name"], "我的收藏夹");
    assert_eq!(side["favorites_id"], 5);
    assert_eq!(side["favorites_owner"], "夹主");
    assert_eq!(side["favorites_owner_id"], 77);
}

#[test]
fn favlist_parser_none_medias() {
    let data = EpisodeData::default();
    let info = json!({"info": {"id": 5, "title": "我的收藏夹", "upper": {"mid": 77, "name": "夹主"}}, "medias": null});
    let node = parse_favlist(&data, &info);
    assert_eq!(node.count(), 0);
}

#[test]
fn history_parser_basic() {
    let data = EpisodeData::default();
    let info = json!({
        "list": [
            {"cover": "https://i.hdslb.com/bfs/archive/h1.jpg", "covers": ["https://i.hdslb.com/bfs/archive/h1.jpg"],
             "duration": 300, "history": {"oid": 101, "epid": 0, "bvid": "BV101", "page": 1, "cid": 1001,
                                          "business": "archive"},
             "title": "历史视频", "view_at": 1700000000},
            {"cover": "https://i.hdslb.com/bfs/archive/h2.jpg", "covers": [],
             "duration": 0, "history": {"oid": 999, "epid": 999, "bvid": "", "page": 1, "cid": 2001,
                                        "business": "pgc"},
             "title": "番剧", "show_title": "第1话", "view_at": 1700000100,
             "uri": "https://www.bilibili.com/bangumi/play/ep999"}
        ]
    });
    let node = parse_history(&data, &info);

    assert_eq!(node.count(), 2);

    let video = node.child(0);
    assert_eq!(video.title, "历史视频");
    assert_eq!(video.url, "https://www.bilibili.com/video/BV101");
    assert!(video.has_attribute(Attribute::VIDEO_BIT));

    let bangumi = node.child(1);
    assert_eq!(bangumi.title, "番剧 - 第1话");
    assert_eq!(bangumi.url, "https://www.bilibili.com/bangumi/play/ep999");
    assert!(bangumi.has_attribute(Attribute::BANGUMI_BIT));
    assert_eq!(bangumi.badge, "Expired");
    assert_eq!(bangumi.duration, 0);

    for i in 0..2 {
        assert!(node.child(i).has_attribute(Attribute::HISTORY_BIT));
    }

    let side = data.get_episode_data(&video.episode_id);
    assert_eq!(side["source_title"], "History");
    assert!(side.get("parent_title").is_none());
}

#[test]
fn popular_parser_basic() {
    let data = EpisodeData::default();
    let info = json!({
        "config": {"label": "每周必看 第1期", "name": "ep-1"},
        "list": [
            {"aid": 11, "bvid": "BV11", "cid": 1101, "duration": 200,
             "pic": "https://i.hdslb.com/bfs/archive/p1.jpg", "pubdate": 1700000000,
             "title": "热门视频", "owner": {"mid": 123, "name": "热门UP"}}
        ]
    });
    let node = parse_popular(&data, &info);

    assert_eq!(node.title, "每周必看 第1期");
    assert_eq!(node.count(), 1);
    let first = node.child(0);
    assert_eq!(first.title, "热门视频");
    assert_eq!(first.cid, 1101);
    assert!(first.has_attribute(Attribute::VIDEO_BIT));
    assert!(first.has_attribute(Attribute::WEEKLY_BIT));
    assert_eq!(first.related_titles["source_title"], "每周必看 第1期");
}

#[test]
fn list_parser_basic() {
    let data = EpisodeData::default();
    let info = json!({
        "meta": {"season_id": 789, "mid": 99999, "name": "合集·测试合集", "total": 1},
        "archives": [
            {"aid": 123, "bvid": "BV1xx411c7mD", "cover": "https://i.hdslb.com/bfs/archive/a1.jpg",
             "duration": 100, "pubdate": 1700000000, "title": "测试视频"}
        ]
    });
    let node = parse_list(&data, &info);

    // meta.title absent → falls back to meta.name
    assert_eq!(node.title, "合集·测试合集");
    assert_eq!(node.count(), 1);
    let first = node.child(0);
    assert!(first.has_attribute(Attribute::COLLECTION_LIST_BIT));
    assert!(first.has_attribute(Attribute::COLLECTION_BIT));
    assert!(first.has_attribute(Attribute::VIDEO_BIT));
    assert!(first.has_attribute(Attribute::NEED_PARSE_BIT));

    let side = data.get_episode_data(&first.episode_id);
    assert_eq!(side["collection_title"], "合集·测试合集");
}

#[test]
fn list_parser_numbering() {
    let data = EpisodeData::default();
    let archives: Vec<Value> = (0..3)
        .map(|i| {
            json!({"aid": i + 1, "bvid": format!("BV{i}"), "title": format!("视频{i}"),
                   "duration": 60, "pubdate": 1700000000, "pic": ""})
        })
        .collect();
    let info = json!({"meta": {"season_id": 1, "name": "合集"}, "archives": archives});
    let node = parse_list(&data, &info);
    for i in 0..3 {
        assert_eq!(node.child(i).number, parsers::tree::Number::Int(i as i64 + 1));
    }
}
