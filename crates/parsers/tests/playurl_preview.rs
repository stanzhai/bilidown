//! External-behavior tests for the playurl fetch + preview selection layer
//! (ticket 12): wiremock at the HTTP seam, per spec Testing Decisions. The
//! `TestDownloadSideWarning` vectors of `test/test_codec_fallback.py` are
//! ported verbatim; the remaining coverage exercises the selection paths the
//! Python suite drives through the GUI dialog (no portable vectors there).

use network::cdn::CdnConfig;
use network::{ClientOptions, HttpClient, ProxyConfig};
use parsers::playurl::{
    audio_quality_label, available_codecs, build_available_streams, check_drm,
    clear_warned_for_test, detect_media_type, fetch_audio_info, fetch_bangumi_info,
    fetch_cheese_info, fetch_lesson_info, fetch_video_info, fetch_video_streams,
    merge_video_streams, parse_declared_quality_map, resolve_codec, resolve_quality,
    sort_audio_ids_preview, video_quality_label, warn_codec_fallback, MediaType, PlayurlApis,
    AUDIO_QUALITY_AUTO, CODEC_AUTO, QUALITY_AUTO,
};
use parsers::preview::{
    audio_quality_choice_data, check_need_parse, video_codec_choice_data,
    video_quality_choice_data, Priorities, Preview,
};
use parsers::tree::{Attribute, Number, TreeItem};
use serde_json::{json, Value};
use wiremock::matchers::{body_partial_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client() -> HttpClient {
    HttpClient::new(ClientOptions {
        user_agent: "bilidown-rs/test".to_owned(),
        proxy: ProxyConfig::none(),
        cookies_from_persistence: Vec::new(),
        signer: None,
    })
    .unwrap()
}

fn item(attribute: Attribute) -> TreeItem {
    let mut it = TreeItem::new(
        Number::Int(1),
        "episode",
        "",
        0,
        0,
        0,
        "https://www.bilibili.com/video/BV1xx",
        "",
        serde_json::Map::new(),
    );
    it.attribute = attribute;
    it
}

/// A realistic DASH video playurl body: two qualities, one with two codecs.
fn dash_body() -> Value {
    json!({
        "code": 0, "message": "0", "ttl": 1,
        "data": {
            "format": "flv", "quality": 80, "accept_quality": [80, 64],
            "timelength": 120000,
            "support_formats": [
                {"quality": 127, "codecs": ["avc1.640034", "hev1.1.6.L120.90", "av01.0.08M.08"]},
                {"quality": 80, "codecs": ["avc1.64003e", "hev1.1.6.L120.90"]},
                {"quality": 64, "codecs": ["avc1.64001f"]}
            ],
            "dash": {
                "duration": 120,
                "video": [
                    {"id": 80, "codecid": 7, "baseUrl": "https://cdn/v80-avc.m4s",
                     "backupUrl": [], "bandwidth": 2000000, "mimeType": "video/mp4",
                     "codecs": "avc1.64003e", "frame_rate": "25"},
                    {"id": 80, "codecid": 12, "baseUrl": "https://cdn/v80-hevc.m4s",
                     "backupUrl": [], "bandwidth": 1800000, "mimeType": "video/mp4",
                     "codecs": "hev1.1.6.L120.90", "frame_rate": "25"},
                    {"id": 64, "codecid": 7, "baseUrl": "https://cdn/v64-avc.m4s",
                     "backupUrl": [], "bandwidth": 900000, "mimeType": "video/mp4",
                     "codecs": "avc1.64001f", "frame_rate": "25"}
                ],
                "audio": [
                    {"id": 30280, "baseUrl": "https://cdn/a192.m4s", "backupUrl": [],
                     "bandwidth": 320000, "mimeType": "audio/mp4", "codecs": "mp4a.40.2"},
                    {"id": 30216, "baseUrl": "https://cdn/a64.m4s", "backupUrl": [],
                     "bandwidth": 68000, "mimeType": "video/mp4", "codecs": "hevc.1.6.L120.90"}
                ],
                "flac": null, "dolby": {"type": 0, "audio": null}
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Declared quality map (quality.py)
// ---------------------------------------------------------------------------

#[test]
fn declared_map_caps_at_dash_video_ceiling_and_maps_codecs() {
    let body = dash_body();
    let data = body["data"].clone();
    let declared = parse_declared_quality_map(&data).unwrap();
    // 127 is declared but not present in dash.video → dropped; 80/64 kept,
    // DESC order, codecs mapped from support_formats prefixes.
    assert_eq!(declared, vec![(80, vec![7, 12]), (64, vec![7])]);
}

#[test]
fn declared_map_none_without_support_formats() {
    let mut data = dash_body()["data"].clone();
    data.as_object_mut().unwrap().remove("support_formats");
    assert!(parse_declared_quality_map(&data).is_none());
}

// ---------------------------------------------------------------------------
// Available streams + audio handling (preview worker/audio_info)
// ---------------------------------------------------------------------------

#[test]
fn dash_available_streams_use_declared_list_for_video_type() {
    let data = dash_body()["data"].clone();
    let streams = build_available_streams("video", MediaType::Dash, &data);
    assert_eq!(streams.quality_ids, vec![80, 64]);
    let codecs = available_codecs(&streams, parse_declared_quality_map(&data).as_deref(), 80);
    assert_eq!(codecs, vec![7, 12]);
    // flac/dolby absent → plain audio ids only.
    assert_eq!(streams.audio_ids, vec![30280, 30216]);
}

#[test]
fn dash_flac_forces_30251_and_dolby_unifies_30255_to_30250() {
    let mut data = dash_body()["data"].clone();
    let dash = data["dash"].as_object_mut().unwrap();
    dash.insert(
        "flac".into(),
        json!({"audio": {"id": 30251, "baseUrl": "https://cdn/flac.m4s", "bandwidth": 2000000}}),
    );
    dash.insert(
        "dolby".into(),
        json!({"audio": [{"id": 30255, "baseUrl": "https://cdn/ec3.m4s", "bandwidth": 640000}]}),
    );
    let streams = build_available_streams("video", MediaType::Dash, &data);
    let mut ids = streams.audio_ids.clone();
    sort_audio_ids_preview(&mut ids);
    assert_eq!(streams.audio_map.get(&30251).is_some(), true);
    assert_eq!(streams.audio_map.get(&30250).is_some(), true);
    assert!(ids.contains(&30251) && ids.contains(&30250) && ids.contains(&30280));
}

#[test]
fn mp4_available_streams_accept_quality_verbatim() {
    let data = json!({"accept_quality": [80, 64], "durl": [{"url": "https://cdn/v.mp4"}]});
    let streams = build_available_streams("video", MediaType::Mp4, &data);
    assert_eq!(streams.quality_ids, vec![80, 64]);
    let (codec, node) = &streams.video_map[&80][0];
    assert_eq!(*codec, 7);
    assert_eq!(node["url_entry_list"][0]["url"], "https://cdn/v.mp4");
}

#[test]
fn media_type_detection_from_body_shape() {
    let data = dash_body()["data"].clone();
    assert_eq!(detect_media_type(&data), MediaType::Dash);
    assert_eq!(detect_media_type(&json!({"format": "mp4"})), MediaType::Mp4);
    assert_eq!(detect_media_type(&json!({"format": "flv"})), MediaType::Flv);
    assert_eq!(detect_media_type(&json!({"format": "m4a"})), MediaType::M4a);
    assert_eq!(detect_media_type(&json!({})), MediaType::Unknown);
}

// ---------------------------------------------------------------------------
// Selection (video_info.py resolve_target)
// ---------------------------------------------------------------------------

#[test]
fn resolve_target_priority_auto_and_fallbacks() {
    let available = vec![80, 64];
    let priority = vec![127, 126, 125, 80, 64];
    // auto → first priority id present.
    assert_eq!(resolve_quality(&available, QUALITY_AUTO, &priority), Some(80));
    // requested not in list → best available.
    assert_eq!(resolve_quality(&available, 116, &priority), Some(80));
    // exact match wins.
    assert_eq!(resolve_quality(&available, 64, &priority), Some(64));

    let codecs = vec![7, 12];
    let codec_priority = vec![7, 12, 13];
    assert_eq!(resolve_codec(&codecs, CODEC_AUTO, &codec_priority), Some(7));
    assert_eq!(resolve_codec(&codecs, 13, &codec_priority), Some(7));
    assert_eq!(resolve_codec(&codecs, 12, &codec_priority), Some(12));
    // empty codec list → None (resolve_target's (None, None) path).
    assert_eq!(resolve_codec(&[], 7, &codec_priority), None);
}

#[test]
fn choice_data_labels_match_python_layout() {
    let quality = video_quality_choice_data(&[80, 64]);
    assert_eq!(quality[0], ("auto".to_string(), 200));
    assert_eq!(quality[1], ("1080P".to_string(), 80));
    let audio = audio_quality_choice_data(&[30251, 30280]);
    assert_eq!(audio[0], ("auto".to_string(), 30300));
    assert_eq!(audio[1], ("HI_RES".to_string(), 30251));
    assert_eq!(audio[2], ("192K".to_string(), 30280));
    let codec = video_codec_choice_data();
    assert_eq!(codec[0], ("auto".to_string(), 20));
    assert_eq!(codec[1], ("AVC/H.264".to_string(), 7));
    assert_eq!(codec[2], ("HEVC/H.265".to_string(), 12));
    assert_eq!(codec[3], ("AV1".to_string(), 13));
    // label helpers used by the dialog
    assert_eq!(video_quality_label(80), Some("1080P"));
    assert_eq!(audio_quality_label(30250), Some("DOLBY_ATMOS"));
}

#[test]
fn list_family_attributes_skip_parsing() {
    for attr in [
        Attribute::SPACE_BIT,
        Attribute::FAVLIST_BIT,
        Attribute::COLLECTION_LIST_BIT,
        Attribute::WATCH_LATER_BIT,
        Attribute::HISTORY_BIT,
    ] {
        assert!(!check_need_parse(attr), "should skip for {attr:?}");
    }
    assert!(check_need_parse(Attribute::VIDEO_BIT));
    assert!(check_need_parse(Attribute::BANGUMI_BIT));
}

// ---------------------------------------------------------------------------
// TestDownloadSideWarning (test_codec_fallback.py:54-101) — verbatim vectors
// ---------------------------------------------------------------------------

#[test]
fn fallback_emits_a_warning() {
    clear_warned_for_test();
    let warning = warn_codec_fallback(Some(13), 7).expect("expected a warning");
    assert_eq!(warning.category(), "WARNING");
    assert!(warning.message.contains("AV1"));
    assert!(warning.message.contains("AVC/H.264"));
    clear_warned_for_test();
}

#[test]
fn same_pair_warns_only_once() {
    clear_warned_for_test();
    warn_codec_fallback(Some(13), 7);
    assert!(warn_codec_fallback(Some(13), 7).is_none());
    clear_warned_for_test();
}

#[test]
fn different_pair_warns_again() {
    clear_warned_for_test();
    warn_codec_fallback(Some(13), 7);
    let second = warn_codec_fallback(Some(13), 12).expect("different pair warns");
    assert!(second.message.contains("HEVC/H.265"));
    clear_warned_for_test();
}

#[test]
fn no_warning_when_codec_matches() {
    clear_warned_for_test();
    assert!(warn_codec_fallback(Some(13), 13).is_none());
    clear_warned_for_test();
}

#[test]
fn no_warning_for_auto() {
    clear_warned_for_test();
    assert!(warn_codec_fallback(Some(CODEC_AUTO), 7).is_none());
    assert!(warn_codec_fallback(None, 7).is_none());
    clear_warned_for_test();
}

// ---------------------------------------------------------------------------
// HTTP seam: fetchers, fnval per parser, supplement, mp4, lesson, audio, DRM
// ---------------------------------------------------------------------------

#[tokio::test]
async fn video_fetch_and_supplement_round_trip() {
    let server = MockServer::start().await;
    let apis = PlayurlApis::with_base(&server.uri());
    let c = client();
    let body = dash_body();

    Mock::given(method("GET"))
        .and(path("/x/player/wbi/playurl"))
        .and(query_param("fnval", "4048"))
        .and(query_param("qn", "127"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let fetched = fetch_video_info(&c, &apis, "BV1xx", 42, 127).await.unwrap();
    assert_eq!(fetched.parser_type, "video");
    assert_eq!(fetched.media_type, MediaType::Dash);

    // Supplement: qn=80 returns only HEVC; AVC missing → pick HEVC, merge.
    let supplement_body = json!({
        "code": 0,
        "data": {"dash": {"video": [
            {"id": 80, "codecid": 12, "baseUrl": "https://cdn/v80-hevc.m4s", "bandwidth": 1800000}
        ]}}
    });
    Mock::given(method("GET"))
        .and(path("/x/player/wbi/playurl"))
        .and(query_param("qn", "80"))
        .respond_with(ResponseTemplate::new(200).set_body_json(supplement_body))
        .expect(1)
        .mount(&server)
        .await;

    let list = fetch_video_streams(&c, &apis, "BV1xx", 42, 80).await;
    let mut map = std::collections::HashMap::new();
    merge_video_streams(&mut map, &list);
    assert!(map.get(&80).is_some_and(|e| e.iter().any(|(cc, _)| *cc == 12)));
}

#[tokio::test]
async fn bangumi_fetch_uses_result_body_and_fnval_143312() {
    let server = MockServer::start().await;
    let apis = PlayurlApis::with_base(&server.uri());
    let c = client();

    let body = json!({
        "code": 0,
        "result": {
            "format": "flv", "accept_quality": [80], "timelength": 60000,
            "durl": [{"url": "https://cdn/bgm.flv", "size": 100, "length": 60000}],
            "dash": {"video": [
                {"id": 80, "codecid": 7, "baseUrl": "https://cdn/v80.m4s", "bandwidth": 1000000}
            ], "audio": []}
        }
    });
    Mock::given(method("GET"))
        .and(path("/pgc/player/web/playurl"))
        .and(query_param("fnval", "143312"))
        .and(query_param("qn", "80"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let fetched = fetch_bangumi_info(&c, &apis, "BV1xx", 42, 80).await.unwrap();
    assert_eq!(fetched.parser_type, "bangumi");
    assert_eq!(fetched.media_type, MediaType::Dash);
    // bangumi never declares (parser_type != "video") → sorted map keys.
    let streams = build_available_streams("bangumi", MediaType::Dash, &fetched.info_data);
    assert_eq!(streams.quality_ids, vec![80]);
}

#[tokio::test]
async fn cheese_fetch_uses_fnval_16_and_avid_epid() {
    let server = MockServer::start().await;
    let apis = PlayurlApis::with_base(&server.uri());
    let c = client();

    let body = json!({
        "code": 0,
        "data": {"format": "mp4", "accept_quality": [16], "timelength": 30000,
                  "durl": [{"url": "https://cdn/cheese.mp4", "size": 50, "length": 30000}]}
    });
    Mock::given(method("GET"))
        .and(path("/pugv/player/web/playurl"))
        .and(query_param("fnval", "16"))
        .and(query_param("avid", "100"))
        .and(query_param("ep_id", "200"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let fetched = fetch_cheese_info(&c, &apis, 100, 42, 0, 200).await.unwrap();
    assert_eq!(fetched.parser_type, "cheese");
    assert_eq!(fetched.media_type, MediaType::Mp4);
    let streams = build_available_streams("cheese", MediaType::Mp4, &fetched.info_data);
    assert_eq!(streams.quality_ids, vec![16]);
    assert_eq!(fetched.info_data["query_url"], format!("{}/pugv/player/web/playurl?avid=100&cid=42&qn=0&fnver=0&fnval=16&fourk=1&ep_id=200", server.uri()));
}

#[tokio::test]
async fn lesson_fetch_posts_payload_and_synthesizes_mp4() {
    let server = MockServer::start().await;
    let apis = PlayurlApis::with_base(&server.uri());
    let c = client();

    let body = json!({
        "code": 0,
        "data": {"videoUrl": "https://cdn/lesson.mp4", "videoTime": 90}
    });
    Mock::given(method("POST"))
        .and(path("/mall-search-items/items/course/section/play/detail"))
        .and(body_partial_json(json!({"courseId": 1, "lessonId": 2, "itemId": 3, "sectionId": 4})))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let fetched = fetch_lesson_info(&c, &apis, 1, 2, 3, 4).await.unwrap();
    assert_eq!(fetched.parser_type, "lesson");
    assert_eq!(fetched.media_type, MediaType::Mp4);
    let durl = &fetched.info_data["durl"][0];
    assert_eq!(durl["url"], "https://cdn/lesson.mp4");
    assert_eq!(durl["length"], 90000);
}

#[tokio::test]
async fn audio_fetch_forces_m4a_and_builds_30280_node() {
    let server = MockServer::start().await;
    let apis = PlayurlApis::with_base(&server.uri());
    let c = client();

    let body = json!({
        "code": 0,
        "data": {"cdns": ["https://cdn/a.m4a"], "size": 4096}
    });
    Mock::given(method("GET"))
        .and(path("/audio/music-service-c/web/url"))
        .and(query_param("sid", "777"))
        .and(query_param("privilege", "2"))
        .and(query_param("quality", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let fetched = fetch_audio_info(&c, &apis, 777).await.unwrap();
    assert_eq!(fetched.parser_type, "audio");
    assert_eq!(fetched.media_type, MediaType::M4a);
    assert_eq!(fetched.info_data["format"], "m4a");
    let streams = build_available_streams("audio", MediaType::M4a, &fetched.info_data);
    assert_eq!(streams.audio_ids, vec![30280]);
    assert_eq!(streams.audio_map[&30280]["codecs"], "mp4a.40.2");
}

#[tokio::test]
async fn drm_body_is_rejected() {
    let err = check_drm(&json!({"is_drm": true})).unwrap_err();
    assert!(err.to_string().contains("DRM"));
    assert!(check_drm(&json!({"is_drm": false})).is_ok());
    assert!(check_drm(&json!({})).is_ok());
}

#[tokio::test]
async fn paid_content_error_maps_to_message() {
    let server = MockServer::start().await;
    let apis = PlayurlApis::with_base(&server.uri());
    let c = client();

    Mock::given(method("GET"))
        .and(path("/x/player/wbi/playurl"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 87008, "message": "抱歉您所在地区不可观看"
        })))
        .mount(&server)
        .await;

    let err = fetch_video_info(&c, &apis, "BV1xx", 42, 127).await.unwrap_err();
    assert!(err.to_string().contains("Paid content requires purchasing"));
}

// ---------------------------------------------------------------------------
// Preview query flow (preview.rs) — cache, size short-circuit, supplement
// ---------------------------------------------------------------------------

async fn head_mock(server: &MockServer, path_and_query: &str, size: u64) {
    Mock::given(method("HEAD"))
        .and(wiremock::matchers::path(path_and_query))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "video/mp4")
                .insert_header("content-length", size.to_string()),
        )
        .mount(server)
        .await;
}

#[tokio::test]
async fn preview_query_video_supplements_missing_quality_then_probes_size() {
    let api = MockServer::start().await;
    let cdn = MockServer::start().await;
    let apis = PlayurlApis::with_base(&api.uri());
    let c = client();
    let cdn_conf = CdnConfig::default();

    let mut base = dash_body();
    // The old-video gap: qn=127 response carries q127+q64 only; q80 is
    // declared (below the dash ceiling) but its stream is absent → supplement.
    base["data"]["dash"]["video"]
        .as_array_mut()
        .unwrap()
        .retain(|v| v["id"] == 64 || v["id"] == 127);
    base["data"]["dash"]["video"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": 127, "codecid": 7, "baseUrl": "https://cdn/v127-avc.m4s",
            "backupUrl": [], "bandwidth": 8000000, "frame_rate": "30"
        }));
    Mock::given(method("GET"))
        .and(path("/x/player/wbi/playurl"))
        .and(query_param("qn", "127"))
        .respond_with(ResponseTemplate::new(200).set_body_json(base.clone()))
        .expect(1)
        .mount(&api)
        .await;
    let supplemental = json!({
        "code": 0,
        "data": {"dash": {"video": [
            {"id": 80, "codecid": 12, "baseUrl": format!("{}/v80-hevc.m4s", cdn.uri()),
             "backupUrl": [], "bandwidth": 1800000, "frame_rate": "25"}
        ]}}
    });
    Mock::given(method("GET"))
        .and(path("/x/player/wbi/playurl"))
        .and(query_param("qn", "80"))
        .respond_with(ResponseTemplate::new(200).set_body_json(supplemental))
        .expect(1)
        .mount(&api)
        .await;
    head_mock(&cdn, "/v80-hevc.m4s", 1_048_576).await;

    let mut preview = Preview::new(Priorities::default());
    let mut it = item(Attribute::VIDEO_BIT);
    it.bvid = "BV1xx".into();
    it.cid = 42;
    let fetched = parsers::playurl::fetch_media_info(&c, &apis, &it, None).await.unwrap();
    preview.start(Attribute::VIDEO_BIT, "BV1xx", 42, async { Ok(fetched) }).await.unwrap();

    // Declared list keeps q80 selectable even though its stream was missing.
    assert_eq!(preview.info.quality_ids, vec![127, 80, 64]);

    // Explicit q80: stream absent → supplement fetch, then probe.
    let info = preview
        .query_video(&c, &apis, 80, CODEC_AUTO)
        .await
        .unwrap()
        .expect("selection resolves");
    // Declared q80 has codecs [7,12]; priority 7 wins after the supplement
    // response carries only the HEVC node — merge fills 12, resolve stays 7
    // because the declared list drives the choice (video_info.py:80).
    assert_eq!((info.quality_id, info.codec_id), (80, 7));
    assert_eq!(info.file_size, 1_048_576);
    assert!(info.is_full_video);

    // Second query hits the cache: no further API hits.
    let cached = preview
        .query_video(&c, &apis, 80, 12)
        .await
        .unwrap()
        .expect("cached");
    assert_eq!(cached.file_size, info.file_size);
}

#[tokio::test]
async fn preview_query_audio_uses_priority_and_size_short_circuit() {
    let data = json!({
        "dash": {
            "audio": [
                {"id": 30280, "size": 4096, "bandwidth": 320000, "codecs": "mp4a.40.2"}
            ],
            "flac": {"audio": {"id": 30251, "baseUrl": "https://cdn/flac.m4s", "bandwidth": 2000000}},
            "dolby": null
        }
    });
    let mut preview = Preview::new(Priorities::default());
    preview.adopt(parsers::playurl::FetchedMedia {
        parser_type: "video",
        media_type: MediaType::Dash,
        info_data: data,
    })
    .unwrap();

    // Auto: flac (30251) is first in the audio priority list.
    let info = preview.query_audio(AUDIO_QUALITY_AUTO).unwrap();
    assert_eq!(info.quality_id, 30251);
    // Explicit 30280 carries its declared size — no probe needed.
    let info = preview.query_audio(30280).unwrap();
    assert_eq!(info.file_size, 4096);
    assert_eq!(info.codec, "mp4a.40.2");
}

#[tokio::test]
async fn preview_query_video_short_circuits_on_declared_size() {
    let mut data = dash_body()["data"].clone();
    for v in data["dash"]["video"].as_array_mut().unwrap() {
        v["size"] = json!(2048);
    }
    let mut preview = Preview::new(Priorities::default());
    preview.adopt(parsers::playurl::FetchedMedia {
        parser_type: "video",
        media_type: MediaType::Dash,
        info_data: data,
    })
    .unwrap();

    let info = preview.query_video(&client(), &PlayurlApis::default(), 80, 7).await.unwrap().unwrap();
    assert_eq!(info.file_size, 2048);
}

#[tokio::test]
async fn preview_falls_back_to_best_when_supplement_returns_nothing() {
    // Only q64 exists at all: the supplement request comes back with no
    // matching id, so auto lands on the one available quality.
    let api = MockServer::start().await;
    let cdn = MockServer::start().await;
    let apis = PlayurlApis::with_base(&api.uri());
    let c = client();

    let mut base = dash_body();
    base["data"]["dash"]["video"]
        .as_array_mut()
        .unwrap()
        .retain(|v| v["id"] == 64 || v["id"] == 127);
    base["data"]["dash"]["video"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": 127, "codecid": 7, "baseUrl": format!("{}/v127-avc.m4s", cdn.uri()),
            "backupUrl": [], "bandwidth": 8000000, "frame_rate": "30"
        }));
    // Supplement: q80 asked, response carries nothing at q80.
    let empty = json!({"code": 0, "data": {"dash": {"video": []}}});
    Mock::given(method("GET"))
        .and(path("/x/player/wbi/playurl"))
        .and(query_param("qn", "80"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty))
        .expect(1)
        .mount(&api)
        .await;
    Mock::given(method("GET"))
        .and(path("/x/player/wbi/playurl"))
        .and(query_param("qn", "127"))
        .respond_with(ResponseTemplate::new(200).set_body_json(base))
        .expect(1)
        .mount(&api)
        .await;

    let fetched = fetch_video_info(&c, &apis, "BV1xx", 42, 127).await.unwrap();
    let mut preview = Preview::new(Priorities::default());
    preview.start(Attribute::VIDEO_BIT, "BV1xx", 42, async { Ok(fetched) }).await.unwrap();

    // q80 supplement yields no stream → error surfaces (worker.py message).
    let err = preview.query_video(&c, &apis, 80, CODEC_AUTO).await.unwrap_err();
    assert!(err.to_string().contains("无法获取该清晰度的视频流"));
}
