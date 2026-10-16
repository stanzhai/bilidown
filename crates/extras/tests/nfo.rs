//! `test/…/test_metadata_nfo.py` vectors: movie / tvshow / episodedetails
//! documents, JSON sidecar, date and runtime helpers.

mod common;

use common::*;
use serde_json::json;

#[test]
fn movie_nfo() {
    let task = episode_task();
    let docs = extras::nfo::generate(&task, "Metadata");
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].name, "N");
    assert_eq!(docs[0].qualifier, ["Metadata"]);
    let expected = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<movie>
    <title>T</title>
    <plot>P</plot>
    <runtime>2</runtime>
    <premiered>{date}</premiered>
    <year>{year}</year>
    <actor>
        <name>U</name>
        <role>UP主</role>
        <profile>https://space.bilibili.com/42</profile>
        <thumb>F</thumb>
    </actor>
    <tag>a</tag>
    <tag>b</tag>
    <thumb>C</thumb>
    <uniqueid type="bvid">BV1</uniqueid>
</movie>
"#,
        date = local_date(1_764_000_000),
        year = local_date(1_764_000_000).split('-').next().unwrap(),
    );
    assert_eq!(docs[0].contents, expected);
}

fn bangumi_task() -> database::TaskInfo {
    let mut task = episode_task();
    task.Episode.attribute = 1 << 1;
    task.Episode.season_title = "季标题".to_owned();
    task.Episode.description = "描述".to_owned();
    task.Episode.premiered = 1_764_000_000;
    task.Episode.uploader = "UP".to_owned();
    task.Episode.styles = vec!["科幻".to_owned(), "热血".to_owned()];
    task.Episode.areas = vec!["日本".to_owned()];
    task.Episode.rating = 9.6;
    task.Episode.rating_votes = 15_000;
    task.Episode.new_ep_status = true;
    task.Episode.poster = "海报".to_owned();
    task.Episode.season_id = 777;
    task.Episode.episode_title = "集标题".to_owned();
    task.Episode.episode_plot = "集简介".to_owned();
    task.Episode.episode_number = 3;
    task.Episode.duration = 125;
    task.Episode.ep_id = 10_001;
    task
}

#[test]
fn tvshow_and_episode_nfo() {
    let task = bangumi_task();
    let docs = extras::nfo::generate(&task, "Metadata");
    assert_eq!(docs.len(), 2);
    assert_eq!(
        (docs[0].name.as_str(), docs[0].qualifier.as_slice()),
        ("tvshow", &[][..])
    );
    assert_eq!(
        (docs[1].name.as_str(), docs[1].qualifier.as_slice()),
        ("N", &[][..])
    );

    let year = local_date(1_764_000_000)
        .split('-')
        .next()
        .unwrap()
        .to_owned();
    let date = local_date(1_764_000_000);
    let tvshow = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<tvshow>
    <title>季标题</title>
    <plot>描述</plot>
    <premiered>{date}</premiered>
    <year>{year}</year>
    <studio>Bilibili</studio>
    <director>UP</director>
    <genre>科幻</genre>
    <genre>热血</genre>
    <country>日本</country>
    <ratings>
        <rating default="true" max="10" name="Bilibili">
            <value>9.6</value>
            <votes>15000</votes>
        </rating>
    </ratings>
    <rating>9.6</rating>
    <status>Ongoing</status>
    <thumb aspect="poster">海报</thumb>
    <uniqueid type="season_id">777</uniqueid>
</tvshow>
"#,
        date = date,
        year = year
    );
    assert_eq!(docs[0].contents, tvshow);

    let episode = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<episodedetails>
    <title>集标题</title>
    <plot>集简介</plot>
    <runtime>3</runtime>
    <premiered>{date}</premiered>
    <year>{year}</year>
    <studio>Bilibili</studio>
    <episode>3</episode>
    <director>UP</director>
    <genre>科幻</genre>
    <genre>热血</genre>
    <country>日本</country>
    <thumb>C</thumb>
    <uniqueid type="ep_id">10001</uniqueid>
</episodedetails>
"#,
        date = local_date(1_764_000_000),
        year = year
    );
    assert_eq!(docs[1].contents, episode);
}

#[test]
fn empty_slots_leave_blank_lines_for_write_strip() {
    let mut task = bangumi_task();
    task.Episode.styles.clear();
    task.Episode.areas.clear();
    task.Episode.rating = 0.0;
    task.Episode.new_ep_status = false;
    let docs = extras::nfo::generate(&task, "Metadata");
    let raw = &docs[0].contents;
    // rating 为空时 ratings 块不存在，但模板槽位仍保留占位
    let between = raw
        .split("<director>UP</director>")
        .nth(1)
        .unwrap()
        .split("<status>")
        .next()
        .unwrap();
    assert_eq!(between, "\n    \n    \n    \n    ");
    assert!(raw.contains("<status>Ended</status>"));
    assert!(!raw.contains("<ratings>"));
}

#[test]
fn tvshow_skipped_when_file_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let mut task = bangumi_task();
    task.File.download_path = tmp.path().display().to_string();
    task.File.folder = "f".to_owned();
    std::fs::create_dir_all(tmp.path().join("f")).unwrap();
    std::fs::write(tmp.path().join("f/tvshow.nfo"), b"existing").unwrap();
    let docs = extras::nfo::generate(&task, "Metadata");
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].name, "N");
}

#[test]
fn json_sidecar_filters_falsy_fields() {
    let mut task = database::TaskInfo::default();
    task.Episode.attribute = 1;
    task.Episode.bvid = "BV1".to_owned();
    task.Episode.rating = 9.6;
    task.Episode.rating_votes = 10;
    task.Episode.new_ep_status = true;
    assert_eq!(
        extras::nfo::episode_json(&task),
        "{\n  \"attribute\": 1,\n  \"bvid\": \"BV1\",\n  \"rating\": 9.6,\n  \"rating_votes\": 10,\n  \"new_ep_status\": true\n}"
    );
}

#[test]
fn json_sidecar_keeps_integral_float_fraction() {
    let mut task = database::TaskInfo::default();
    task.Episode.rating = 8.0;
    assert_eq!(extras::nfo::episode_json(&task), "{\n  \"rating\": 8.0\n}");
}

#[test]
fn date_from_timestamp_normalizes_milliseconds() {
    let secs = 1_764_000_000i64;
    assert_eq!(extras::nfo::date_from_timestamp(secs), local_date(secs));
    assert_eq!(
        extras::nfo::date_from_timestamp(secs * 1000),
        local_date(secs)
    );
}

#[tokio::test]
async fn parse_nfo_fetches_tags_and_writes_files() {
    let tmp = tempfile::tempdir().unwrap();
    let server = wiremock::MockServer::start().await;
    mount_json(
        &server,
        "/x/web-interface/view/detail/tag",
        json!({"data": [{"tag_name": "纪录片"}, {"tag_name": "人文"}]}),
        1,
    )
    .await;
    mount_bytes(&server, "/poster.jpg", vec![1, 2, 3, 4], 1).await;

    let mut task = episode_task();
    task.File.download_path = tmp.path().display().to_string();
    task.File.folder = "f".to_owned();
    task.Episode.cover = "C".to_owned();
    task.Episode.poster = format!("{}/poster.jpg", server.uri());

    extras::nfo::parse(
        &mock_client(),
        &server.uri(),
        &mut task,
        &empty_store(),
        &labels(),
    )
    .await
    .unwrap();

    let nfo = std::fs::read_to_string(tmp.path().join("f/N.Metadata.nfo")).unwrap();
    assert!(nfo.contains("<tag>纪录片</tag>"));
    assert!(nfo.contains("<tag>人文</tag>"));
    // _write 的空行剥离：内容里不得有空行
    assert!(!nfo.contains("\n\n"));
    assert_eq!(
        std::fs::read(tmp.path().join("f/poster.jpg")).unwrap(),
        vec![1, 2, 3, 4]
    );
    assert_eq!(
        task.Episode.tags,
        vec!["纪录片".to_owned(), "人文".to_owned()]
    );
}

#[tokio::test]
async fn parse_json_writes_sidecar() {
    let tmp = tempfile::tempdir().unwrap();
    let mut task = bangumi_task();
    task.File.download_path = tmp.path().display().to_string();
    task.File.folder = "f".to_owned();
    task.Options.metadata_type = Some("json".to_owned());

    extras::nfo::parse(&mock_client(), "", &mut task, &empty_store(), &labels())
        .await
        .unwrap();

    let sidecar = std::fs::read_to_string(tmp.path().join("f/N.Metadata.json")).unwrap();
    assert!(sidecar.starts_with("{\n  \"attribute\": 2"));
    assert!(sidecar.contains("\"season_title\": \"季标题\""));
}
