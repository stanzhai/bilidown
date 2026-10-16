//! `test/…/test_subtitles.py` vectors: per-format export + language
//! selection + MKV embed registration.

mod common;

use common::*;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn srt_format() {
    let data = subtitle_body();
    assert_eq!(
        extras::subtitles::to_srt(&data),
        "1\n00:00:01,500 --> 00:00:03,250\n你好 world\n\n2\n00:00:10,000 --> 00:00:12,500\n第二行"
    );
}

#[test]
fn lrc_format() {
    let data = subtitle_body();
    assert_eq!(
        extras::subtitles::to_lrc(&data),
        "[00:01.50]你好 world\n[00:10.00]第二行"
    );
}

#[test]
fn txt_format() {
    assert_eq!(
        extras::subtitles::to_txt(&subtitle_body()),
        "你好 world\n第二行"
    );
}

#[test]
fn json_format() {
    assert_eq!(
        extras::subtitles::to_json(&subtitle_body()),
        serde_json::to_string_pretty(&subtitle_body()).unwrap()
    );
}

#[test]
fn iso_639_2_mapping() {
    use extras::subtitles::to_iso_639_2;
    assert_eq!(to_iso_639_2("zh-CN"), "chi");
    assert_eq!(to_iso_639_2("zh-Hans"), "chi");
    assert_eq!(to_iso_639_2("en"), "eng");
    assert_eq!(to_iso_639_2("ja"), "jpn");
    assert_eq!(to_iso_639_2("ai-zh"), "chi");
    assert_eq!(to_iso_639_2("AI-zh"), "chi");
    assert_eq!(to_iso_639_2("klingon"), "klingon");
}

fn player_data(_server: &MockServer, urls: &[&str]) -> serde_json::Value {
    json!({
        "subtitle": {
            "subtitles": urls
                .iter()
                .map(|u| json!({"lan": "zh", "lan_doc": "中文（自动生成）", "subtitle_url": u}))
                .collect::<Vec<_>>()
        }
    })
}

#[tokio::test]
async fn fetches_protocol_relative_subtitle_url() {
    let server = MockServer::start().await;
    mount_json(&server, "/subtitle/zh.json", subtitle_body(), 1).await;
    let client = mock_client();
    let path = format!("{}/subtitle/zh.json", server.uri());

    let list = extras::subtitles::get_subtitles_data_list(
        &client,
        &player_data(&server, &[&path]),
        &json!({"download_specified": false}),
    )
    .await
    .unwrap();

    assert_eq!(list.len(), 1);
    assert_eq!(list[0].language, "zh");
    assert_eq!(list[0].language_doc, "中文（自动生成）");
    assert_eq!(list[0].data, subtitle_body());

    let request = &server.received_requests().await.unwrap()[0];
    assert!(request.url.path().ends_with("/subtitle/zh.json"));
}

#[tokio::test]
async fn language_filter_drops_unselected_entries() {
    let server = MockServer::start().await;
    mount_json(&server, "/subtitle/zh.json", subtitle_body(), 1).await;
    let client = mock_client();
    let url = format!("{}/subtitle/zh.json", server.uri());

    let player = json!({"subtitle": {"subtitles": [
        {"lan": "zh", "lan_doc": "中文", "subtitle_url": url},
        {"lan": "en", "lan_doc": "English", "subtitle_url": url},
        {"lan": "ai-zh", "lan_doc": "中文（自动生成）", "subtitle_url": url}
    ]}});
    let config = json!({"download_specified": true, "specified_language": ["zh"]});

    let list = extras::subtitles::get_subtitles_data_list(&client, &player, &config)
        .await
        .unwrap();
    assert_eq!(
        list.iter().map(|e| e.language.as_str()).collect::<Vec<_>>(),
        ["zh"]
    );
    // 只选 zh 时，en / ai-zh 都不该发请求
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn falsy_subtitle_body_is_dropped() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/subtitle/empty.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;
    let client = mock_client();
    let url = format!("{}/subtitle/empty.json", server.uri());

    let list = extras::subtitles::get_subtitles_data_list(
        &client,
        &player_data(&server, &[&url]),
        &json!({"download_specified": false}),
    )
    .await
    .unwrap();
    assert!(list.is_empty());
}

#[tokio::test]
async fn parse_writes_srt_and_embeds_ass_track() {
    let tmp = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    mount_json(&server, "/subtitle/zh.json", subtitle_body(), 2).await;
    let client = mock_client();
    let url = format!("{}/subtitle/zh.json", server.uri());

    let mut task = database::TaskInfo::default();
    task.Basic.show_title = "S".to_owned();
    task.File.name = "V".to_owned();
    task.File.download_path = tmp.path().display().to_string();
    task.File.folder = "f".to_owned();
    task.File.merge_file_ext = "mkv".to_owned();
    task.Options.subtitle_type = Some("srt".to_owned());
    task.Options.embed_subtitle = Some(true);

    let player = json!({"subtitle": {"subtitles": [
        {"lan": "zh", "lan_doc": "中文", "subtitle_url": url},
        {"lan": "ai-zh", "lan_doc": "中文（AI）", "subtitle_url": url}
    ]}});
    extras::subtitles::parse(&client, &mut task, &empty_store(), &labels(), &player)
        .await
        .unwrap();

    let srt = std::fs::read_to_string(tmp.path().join("f/V.Subtitles.zh.srt")).unwrap();
    assert!(srt.starts_with("1\n00:00:01,500"));
    let ai = std::fs::read_to_string(tmp.path().join("f/V.Subtitles.ai-zh.srt")).unwrap();
    assert!(ai.starts_with("1\n00:00:01,500"));
    // srt 不是 ass：即便 embed_subtitle 打开也不注册字幕轨
    assert!(task.File.subtitle_track_list.is_empty());
}

#[tokio::test]
async fn parse_ass_embeds_track_with_ai_title() {
    let tmp = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    mount_json(&server, "/subtitle/zh.json", subtitle_body(), 1).await;
    let client = mock_client();
    let url = format!("{}/subtitle/zh.json", server.uri());

    let mut task = database::TaskInfo::default();
    task.Basic.show_title = "S".to_owned();
    task.File.name = "V".to_owned();
    task.File.download_path = tmp.path().display().to_string();
    task.File.folder = "f".to_owned();
    task.File.merge_file_ext = "mkv".to_owned();
    task.Options.embed_subtitle = Some(true);
    // 全局默认 subtitle_type=ass

    let player = json!({"subtitle": {"subtitles": [
        {"lan": "ai-zh", "lan_doc": "中文", "subtitle_url": url}
    ]}});
    extras::subtitles::parse(&client, &mut task, &empty_store(), &labels(), &player)
        .await
        .unwrap();

    let ass = std::fs::read_to_string(tmp.path().join("f/V.Subtitles.ai-zh.ass")).unwrap();
    assert!(ass.contains("[Script Info]"));
    assert!(ass.contains("Aspect Ratio: 1280:720"));
    assert!(ass.contains("Dialogue: 0,0:00:01.50,0:00:03.25,Default,,0,0,0,,你好 world"));

    assert_eq!(task.File.subtitle_track_list.len(), 1);
    let track = &task.File.subtitle_track_list[0];
    assert_eq!(track["file"], "V.Subtitles.ai-zh.ass");
    assert_eq!(track["title"], "(AI Generated) 中文");
    assert_eq!(track["language"], "chi");
    assert_eq!(track["kind"], "subtitle");
}

#[tokio::test]
async fn embed_requires_mkv() {
    let mut task = database::TaskInfo::default();
    task.File.merge_file_ext = "mp4".to_owned();
    assert!(!extras::writer::is_embed_available(&task));
    task.File.merge_file_ext = "mkv".to_owned();
    assert!(extras::writer::is_embed_available(&task));
}
