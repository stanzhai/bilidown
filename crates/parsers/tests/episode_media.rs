//! Slice-3 episode-parser tests: video single/分P/合集, bangumi, cheese,
//! mall lesson. Fixtures mirror real API `data` shapes.

use serde_json::{json, Value};

use parsers::episode::bangumi::{build_sections, parse_bangumi};
use parsers::episode::list_base::episode_duration;
use parsers::episode::media::{parse_cheese, parse_lesson};
use parsers::episode::video::{get_cid, parse_video};
use parsers::tree::{Attribute, EpisodeData, Number};

fn owner() -> Value {
    json!({"mid": 42, "name": "测试UP主", "face": "face.jpg"})
}

fn video_data() -> Value {
    json!({
        "aid": 170001, "bvid": "BV17x411w7KC", "cid": 1001,
        "pic": "cover.jpg", "pubdate": 1700000000, "title": "单视频标题",
        "duration": 300, "desc": "视频简介", "tid": 21, "tid_v2": 171,
        "is_upower_exclusive": false, "owner": owner(),
        "pages": [{"cid": 1001, "page": 1, "part": "单视频标题", "duration": 300, "ctime": 1700000000}]
    })
}

#[test]
fn single_video() {
    let data = EpisodeData::default();
    let id = data.add_episode();
    let (node, (key, value)) = parse_video(&data, &id, &video_data());

    assert_eq!(node.title, "");
    assert!(node.has_attribute(Attribute::TREE_NODE_BIT));
    assert_eq!(node.count(), 1);

    let item = node.child(0);
    assert_eq!(item.title, "单视频标题");
    assert_eq!(item.cid, 1001);
    assert_eq!(item.duration, 300);
    assert_eq!(item.pubtime, 1700000000);
    assert_eq!(item.url, "https://www.bilibili.com/video/BV17x411w7KC");
    assert!(item.has_attribute(Attribute::VIDEO_BIT | Attribute::NORMAL_BIT));
    assert_eq!(item.number, Number::Int(1));

    assert_eq!(key, "cid");
    assert_eq!(value, json!(1001));

    let side = data.get_episode_data(&id);
    assert_eq!(side["description"], "视频简介");
    assert_eq!(side["tid"], 21);
    assert_eq!(side["uploader"], "测试UP主");
    assert_eq!(side["uploader_uid"], 42);
}

#[test]
fn multi_part_video() {
    let data = EpisodeData::default();
    let id = data.add_episode();
    let mut info = video_data();
    info["title"] = json!("分P视频");
    info["pages"] = json!([
        {"cid": 2001, "page": 1, "part": "第一P", "duration": 100, "ctime": 1700000100},
        {"cid": 2002, "page": 2, "part": "第二P", "duration": 110, "ctime": 1700000100},
        {"cid": 2003, "page": 3, "part": "第三P", "duration": 120, "ctime": 1700000100}
    ]);

    let (node, (key, value)) = parse_video(&data, &id, &info);

    assert_eq!(node.title, "分P视频");
    assert_eq!(node.count(), 3);
    let second = node.child(1);
    assert_eq!(second.title, "第二P");
    assert_eq!(second.cid, 2002);
    assert_eq!(second.pubtime, 1700000100);
    assert_eq!(
        second.url,
        "https://www.bilibili.com/video/BV17x411w7KC?p=2"
    );
    assert!(second.has_attribute(Attribute::VIDEO_BIT | Attribute::PART_BIT));
    assert_eq!(second.related_titles["parent_title"], "分P视频");
    assert_eq!(key, "cid");
    assert_eq!(value, json!(1001)); // no current_page_number → info.cid
}

#[test]
fn current_page_number_pins_cid() {
    let mut info = video_data();
    info["current_page_number"] = json!(3);
    info["pages"] = json!([
        {"cid": 2001, "page": 1, "part": "一", "duration": 1, "ctime": 0},
        {"cid": 2003, "page": 3, "part": "三", "duration": 3, "ctime": 0}
    ]);
    assert_eq!(get_cid(&info), 2003);
}

#[test]
fn ugc_season_single_section_flattens() {
    let data = EpisodeData::default();
    let id = data.add_episode();
    let mut info = video_data();
    info["title"] = json!("合集入口视频");
    info["ugc_season"] = json!({
        "title": "我的合集",
        "sections": [
            {"title": "正片", "episodes": [
                {"aid": 2, "bvid": "BV1ep1", "cid": 10, "title": "合集视频一",
                 "attribute": 0, "duration": 11,
                 "pages": [{"cid": 10, "page": 1, "part": "合集视频一", "duration": 11}],
                 "arc": {"pic": "ep1.jpg", "pubdate": 1700000010}},
                {"aid": 3, "bvid": "BV1ep2", "cid": 20, "title": "合集视频二",
                 "attribute": 8, "duration": 22,
                 "pages": [{"cid": 20, "page": 1, "part": "合集视频二", "duration": 22}],
                 "arc": {"pic": "ep2.jpg", "pubdate": 1700000020}}
            ]}
        ]
    });

    let (node, _) = parse_video(&data, &id, &info);

    assert_eq!(node.title, "我的合集");
    // Single section → no 章节 layer, episodes hang off the root.
    assert_eq!(node.count(), 2);
    let first = node.child(0);
    assert!(first.has_attribute(Attribute::VIDEO_BIT | Attribute::COLLECTION_BIT));
    assert_eq!(first.title, "合集视频一");
    assert_eq!(first.cover, "ep1.jpg");
    assert_eq!(first.pubtime, 1700000010);
    assert_eq!(first.related_titles["collection_title"], "我的合集");
    assert_eq!(first.related_titles["section_title"], "");
    // attribute bit 3 (1<<3 = 8) → 充电专属
    assert_eq!(node.child(1).badge, "充电专属");
}

#[test]
fn ugc_season_multi_section_keeps_layers() {
    let data = EpisodeData::default();
    let id = data.add_episode();
    let mut info = video_data();
    info["ugc_season"] = json!({
        "title": "多层合集",
        "sections": [
            {"title": "正片", "episodes": [
                {"aid": 2, "bvid": "BV1m1", "cid": 31, "title": "第一节",
                 "attribute": 0, "duration": 5,
                 "pages": [{"cid": 31, "page": 1, "part": "第一节", "duration": 5},
                           {"cid": 32, "page": 2, "part": "第一节P2", "duration": 6}],
                 "arc": {"pic": "m1.jpg", "pubdate": 1700000030}}
            ]},
            {"title": "花絮", "episodes": [
                {"aid": 4, "bvid": "BV1m2", "cid": 41, "title": "花絮一",
                 "attribute": 0, "duration": 7,
                 "pages": [{"cid": 41, "page": 1, "part": "花絮一", "duration": 7}],
                 "arc": {"pic": "m2.jpg", "pubdate": 1700000040}}
            ]}
        ]
    });

    let (node, _) = parse_video(&data, &id, &info);

    assert_eq!(node.count(), 2); // two 章节 sections
    let section0 = node.child(0);
    assert_eq!(section0.title, "正片");
    assert_eq!(section0.count(), 1); // one 分P sub-node
    let page_node = section0.child(0);
    assert_eq!(page_node.title, "第一节");
    assert_eq!(page_node.count(), 2);
    let p2 = page_node.child(1);
    assert_eq!(p2.title, "第一节P2");
    assert_eq!(p2.url, "https://www.bilibili.com/video/BV1m1?p=2");
    assert_eq!(p2.related_titles["section_title"], "正片");
    assert_eq!(p2.related_titles["parent_title"], "第一节");

    let section1 = node.child(1);
    assert_eq!(section1.title, "花絮");
    assert_eq!(section1.child(0).title, "花絮一");
}

// ---------------------------------------------------------------------------
// Bangumi
// ---------------------------------------------------------------------------

fn bangumi_info() -> Value {
    json!({
        "season_id": 431, "media_id": 2823, "season_title": "测试番剧",
        "series": {"series_id": 77, "series_title": "测试系列"},
        "seasons": [{"season_id": 100}, {"season_id": 431}],
        "areas": [{"name": "日本"}],
        "publish": {"pub_time": "2023-05-01 00:00:00"},
        "evaluate": "这是简介",
        "styles": ["科幻"],
        "cover": "poster.jpg", "actors": "演员表",
        "rating": {"score": 95, "count": 1234},
        "up_info": {"uname": "番剧UP", "mid": 55, "avatar": "ava.jpg"},
        "episodes": [
            {"aid": 11, "bvid": "BV1ep1", "cid": 111, "ep_id": 501, "badge": "",
             "cover": "e1.jpg", "duration": 1440000, "link": "https://www.bilibili.com/bangumi/play/ep501",
             "pub_time": 1682870400, "show_title": "第一话", "title": "第1话"},
            {"aid": 12, "bvid": "BV1ep2", "cid": 112, "ep_id": 502, "badge": "预告",
             "cover": "e2.jpg", "duration": 600000, "link": "https://www.bilibili.com/bangumi/play/ep502",
             "pub_time": 1682870500, "show_title": "PV", "title": "预告"}
        ],
        "section": [
            {"title": "番外", "episodes": [
                {"aid": 13, "bvid": "BV1ex", "cid": 113, "ep_id": 503, "badge": "番外",
                 "cover": "ex.jpg", "duration": 900000, "link": "https://www.bilibili.com/bangumi/play/ep503",
                 "pub_time": 1682870600, "show_title": "番外篇", "title": "番外"}
            ]},
            {"title": "UP主陪你看", "episodes": [
                {"aid": 14, "cover": "x.jpg", "duration": 100, "id": 999,
                 "link": "https://www.bilibili.com/video/x", "pub_time": 1682870700,
                 "show_title": "无cid", "title": "无cid剧集"}
            ]}
        ]
    })
}

#[test]
fn bangumi_sections_and_numbers() {
    let info = bangumi_info();
    let (sections, number_map) = build_sections(&info);

    // 正片 + 番外; the UP主陪你看 section (no bvid/cid) is dropped.
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0]["title"], "正片");
    assert_eq!(sections[1]["title"], "番外");
    // 预告 does not consume an episode number.
    assert_eq!(number_map.get(&111), Some(&1));
    assert!(!number_map.contains_key(&112));
}

#[test]
fn bangumi_tree_and_side_data() {
    let data = EpisodeData::default();
    let id = data.add_episode();
    let node = parse_bangumi(&data, &id, &bangumi_info());

    assert_eq!(node.title, "测试番剧");
    assert_eq!(node.count(), 2); // 正片 + 番外

    let main = node.child(0);
    assert_eq!(main.title, "正片");
    assert_eq!(main.count(), 2);
    let ep1 = main.child(0);
    assert_eq!(ep1.title, "第一话"); // show_title wins
    assert_eq!(ep1.duration, 1440); // /1000
    assert_eq!(ep1.ep_id, 501);
    assert_eq!(ep1.episode_number, 1);
    assert!(ep1.has_attribute(Attribute::BANGUMI_BIT));
    assert_eq!(ep1.related_titles["season_title"], "测试番剧");
    assert_eq!(ep1.related_titles["section_title"], "正片");
    assert_eq!(ep1.episode_plot, "《测试番剧》第一话");
    let trailer = main.child(1);
    assert_eq!(trailer.badge, "预告");
    assert_eq!(trailer.episode_number, 0); // not in the map

    let extra = node.child(1);
    assert_eq!(extra.title, "番外");
    assert_eq!(extra.child(0).title, "番外篇");

    let side = data.get_episode_data(&id);
    assert_eq!(side["series_title"], "测试系列");
    assert_eq!(side["season_number"], 2); // seasons[1] == 431
    assert_eq!(side["areas"], json!(["日本"]));
    assert_eq!(side["premiered"], 1682870400);
    assert_eq!(side["rating"], 95);
    assert_eq!(side["rating_votes"], 1234);
    assert_eq!(side["uploader"], "番剧UP");
    let styles = side["styles"].as_array().unwrap();
    assert!(styles.contains(&json!("科幻")));
    assert!(styles.contains(&json!("Bilibili 番剧")));
}

#[test]
fn timestamp_vectors() {
    use parsers::episode::video::timestamp_from_string;
    assert_eq!(timestamp_from_string("2023-05-01 00:00:00"), 1682870400);
    assert_eq!(timestamp_from_string("2023-06-01 12:30:00"), 1685593800);
}

// ---------------------------------------------------------------------------
// Cheese
// ---------------------------------------------------------------------------

#[test]
fn cheese_tree_and_side_data() {
    let data = EpisodeData::default();
    let id = data.add_episode();
    let info = json!({
        "title": "测试课程", "season_id": 888, "cover": "cheese.jpg",
        "subtitle": "课程副标题", "release_date": 1682870400,
        "up_info": {"uname": "课程UP", "mid": 66, "avatar": "cava.jpg"},
        "sections": [
            {"title": "第1季", "episodes": [
                {"aid": 21, "cid": 211, "cover": "c1.jpg", "duration": 300,
                 "id": 3001, "play_way_subtitle": "正片", "subtitle": "第1集副标题",
                 "release_date": 1682870401, "status": 2, "title": "第1集"},
                {"aid": 22, "cid": 212, "cover": "c2.jpg", "duration": 301,
                 "id": 3002, "label": "限时免费", "play_way_subtitle": "正片",
                 "release_date": 1682870402, "status": 1, "subtitle": "第2集副标题",
                 "title": "第2集"}
            ]},
            {"title": "空章节", "episodes": []}
        ]
    });

    let node = parse_cheese(&data, &id, &info);

    assert_eq!(node.title, "测试课程");
    assert_eq!(node.count(), 1); // empty section skipped
    let season = node.child(0);
    assert_eq!(season.title, "第1季");
    let ep1 = season.child(0);
    assert_eq!(ep1.title, "第1集");
    assert_eq!(ep1.ep_id, 3001);
    assert_eq!(ep1.badge, "付费"); // status 2
    assert_eq!(ep1.url, "https://www.bilibili.com/cheese/play/3001");
    assert_eq!(ep1.episode_plot, "正片 · 第1集副标题");
    assert!(ep1.has_attribute(Attribute::CHEESE_BIT));
    let ep2 = season.child(1);
    assert_eq!(ep2.badge, "限时免费"); // label wins
    assert_eq!(ep2.episode_number, 2);

    let side = data.get_episode_data(&id);
    assert_eq!(side["season_id"], 888);
    assert_eq!(side["premiered"], 1682870400);
    assert_eq!(side["styles"], json!(["Bilibili 课堂"]));
    assert_eq!(side["uploader"], "课程UP");
}

// ---------------------------------------------------------------------------
// Mall lesson
// ---------------------------------------------------------------------------

#[test]
fn lesson_tree_and_side_data() {
    let data = EpisodeData::default();
    let id = data.add_episode();
    let info = json!({
        "courseId": 100, "lessonId": 200, "itemId": 300,
        "lessonName": "测试课程", "itemsName": "【营销】测试课程",
        "locationInfo": {"sectionId": 5002},
        "chapterList": [
            {"chapterName": "  ", "sectionList": [
                {"sectionId": 5001, "sectionName": "未更新小节", "videoTime": 0},
                {"sectionId": 5002, "sectionName": "第一课", "videoTime": 90500,
                 "sectionIndex": 1, "hasWatchRight": true},
                {"sectionId": 5003, "sectionName": "第二课", "videoTime": 80000,
                 "sectionIndex": 2, "hasWatchRight": false, "couldPreview": true},
                {"sectionId": 5004, "sectionName": "第三课", "videoTime": 70000,
                 "sectionIndex": 3, "hasWatchRight": false, "couldPreview": false}
            ]},
            {"chapterName": "进阶篇", "sectionList": [
                {"sectionId": 5005, "sectionName": "进阶课", "videoTime": 60000,
                 "sectionIndex": 4, "hasWatchRight": true, "lessonId": 201}
            ]}
        ]
    });

    let node = parse_lesson(&data, &id, &info);

    assert_eq!(node.title, "测试课程"); // lessonName over itemsName
                                        // Blank chapter name → sections attach to root; named chapter kept.
    assert_eq!(node.count(), 4);

    let first = node.child(0);
    assert_eq!(first.section_id, 5002); // videoTime==0 section skipped
    assert_eq!(first.title, "第一课");
    assert_eq!(first.duration, 90); // videoTime/1000, floor
    assert_eq!(first.badge, ""); // hasWatchRight
    assert!(first.has_attribute(Attribute::LESSON_BIT));
    assert_eq!(
        first.url,
        "https://mall.bilibili.com/lesson/play?courseId=100&lessonId=200&itemId=300"
    );
    assert_eq!(first.related_titles["section_title"], "测试课程"); // blank chapter → course title

    assert_eq!(node.child(1).badge, "试看"); // couldPreview
    assert_eq!(node.child(2).badge, "付费");

    let chapter = node.child(3);
    assert_eq!(chapter.title, "进阶篇");
    let inner = chapter.child(0);
    assert_eq!(inner.title, "进阶课");
    assert_eq!(inner.lesson_id, 201); // section lessonId wins
    assert_eq!(inner.related_titles["section_title"], "进阶篇");

    let side = data.get_episode_data(&id);
    assert_eq!(side["season_id"], 200);
    assert_eq!(side["styles"], json!(["Bilibili 课堂"]));
    assert_eq!(
        parsers::episode::media::lesson_get_section_id(&info),
        json!(5002)
    );
}

#[test]
fn duration_helpers() {
    assert_eq!(episode_duration(&json!({"duration": 42})), 42);
    assert_eq!(episode_duration(&json!({"arc": {"duration": 50}})), 50);
    assert_eq!(episode_duration(&json!({"length": "1:02:03"})), 3723);
}
