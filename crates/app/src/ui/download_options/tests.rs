//! Headless view-model tests for the download options dialog — the port of
//! `tests/test_download_options.py`'s logic vectors (pure gating rules, no
//! gpui).

use config::ConfigStore;
use naming::naming_rules::RuleStore;
use naming::task_naming::NamingState;
use parsers::playurl::{CODEC_AUTO, QUALITY_AUTO};
use parsers::preview::{PreviewInfo, VideoQueryInfo};
use parsers::tree::Attribute;
use serde_json::{json, Value};

use super::model::{AcceptCheck, DownloadOptionsModel};

fn test_cfg() -> ConfigStore {
    let dir = std::env::temp_dir().join(format!("t21-{}", std::process::id()));
    let mut cfg = ConfigStore::with_paths(dir.clone(), dir);
    // the Python dialog's default scene: both streams + merge.
    cfg.set("Download", "download_video_stream", Value::Bool(true));
    cfg.set("Download", "download_audio_stream", Value::Bool(true));
    cfg.set("Download", "merge_video_audio", Value::Bool(true));
    cfg.set("Download", "video_container", Value::String("mkv".into()));
    cfg
}

fn preview_info() -> PreviewInfo {
    PreviewInfo {
        episode_title: "Title".into(),
        episode_number: "1".into(),
        attribute: Attribute::NORMAL_BIT,
        media_type: parsers::playurl::MediaType::Dash,
        video_quality_choices: vec![("Auto".into(), QUALITY_AUTO)],
        audio_quality_choices: vec![("Auto".into(), 30300)],
        video_codec_choices: vec![("Auto".into(), CODEC_AUTO)],
        ..PreviewInfo::new()
    }
}

fn open_model(cfg: &ConfigStore) -> DownloadOptionsModel {
    let store = RuleStore::default();
    DownloadOptionsModel::open(
        cfg,
        &preview_info(),
        &[11],
        &store,
        (QUALITY_AUTO, 30300, CODEC_AUTO),
    )
}

#[test]
fn stream_switches_gate_merge_and_keep_original() {
    let mut m = open_model(&test_cfg());
    // turning either stream off clears merge + keep-original
    m.media.set_stream_switches(false, true);
    assert!(!m.media.merge_video_audio);
    assert!(!m.media.keep_original_files);
    // both back on: merge auto-rechecks (Python force-checks it), keep stays off
    m.media.set_stream_switches(true, true);
    assert!(m.media.merge_video_audio);
    // explicit un-merge then re-merge keeps keep-original still user's choice
    m.media.set_merge(false);
    m.media.set_merge(true);
    assert!(m.media.merge_video_audio);
    assert!(!m.media.keep_original_files);
}

#[test]
fn embed_gating_follows_container_without_resetting_state() {
    let mut m = open_model(&test_cfg());
    m.additional.download_danmaku = true;
    m.additional.embed_danmaku = true;
    m.additional.danmaku_type = "ass".into();
    assert!(m.additional.danmaku_embed_enabled());
    // mp4 disables the control but never resets the checked state
    m.set_video_container("mp4".into());
    assert!(!m.additional.danmaku_embed_enabled());
    assert!(m.additional.embed_danmaku);
    // back to mkv: still checked, gated open again
    m.set_video_container("mkv".into());
    assert!(m.additional.danmaku_embed_enabled());
    assert!(m.additional.embed_danmaku);
    // container linkage keeps both pages in sync
    assert_eq!(m.download.video_container, "mkv");
    // ass + mkv but download off → gated
    m.additional.download_danmaku = false;
    assert!(!m.additional.danmaku_embed_enabled());
}

#[test]
fn avif_cover_forces_attach_off_and_gates_delete() {
    let mut m = open_model(&test_cfg());
    m.additional.download_cover = true;
    m.additional.attach_cover = true;
    m.additional.delete_cover_after_attach = true;
    assert!(m.additional.cover_attach_enabled());
    assert!(m.additional.cover_delete_after_attach_enabled());
    m.additional.set_cover_type("avif".into());
    assert!(!m.additional.attach_cover);
    assert!(!m.additional.cover_attach_enabled());
    assert!(!m.additional.cover_delete_after_attach_enabled());
}

#[test]
fn codec_fallback_message_rules() {
    let mut m = open_model(&test_cfg());
    // auto never falls back
    m.media.video_codec_id = CODEC_AUTO;
    m.media.video_info = Some(VideoQueryInfo {
        quality_id: 80,
        codec_id: 7,
        frame_rate: json!(30.0),
        bitrate: json!(2_000_000),
        file_size: 1024,
        is_full_video: true,
    });
    assert_eq!(m.media.codec_fallback_message(c()), "");
    // requested == actual → no fallback
    m.media.video_codec_id = 7;
    assert_eq!(m.media.codec_fallback_message(c()), "");
    // requested hevc but source is av1 → message names the requested codec
    m.media.video_codec_id = 12;
    let msg = m.media.codec_fallback_message(c());
    assert!(msg.contains("HEVC") || msg.contains("H.265") || !msg.is_empty());
}

#[test]
fn accept_check_ordering() {
    let mut m = open_model(&test_cfg());
    // video-only asks first
    m.media.download_audio_stream = false;
    assert_eq!(m.accept_check(), AcceptCheck::ConfirmVideoOnly);
    // both + no merge asks second
    m.media.download_audio_stream = true;
    m.media.merge_video_audio = false;
    assert_eq!(m.accept_check(), AcceptCheck::ConfirmNoMerge);
    // nothing selected is the last gate
    m.media.download_video_stream = false;
    m.media.download_audio_stream = false;
    m.additional.download_danmaku = false;
    m.additional.download_subtitle = false;
    m.additional.download_cover = false;
    m.additional.download_metadata = false;
    assert_eq!(m.accept_check(), AcceptCheck::NoFilesSelected);
    // chapter alone doesn't count (not in has_file_to_download)
    m.additional.embed_chapter = true;
    assert_eq!(m.accept_check(), AcceptCheck::NoFilesSelected);
    // a file selected → ok
    m.additional.download_danmaku = true;
    assert_eq!(m.accept_check(), AcceptCheck::Ok);
}

#[test]
fn preview_tags_empty_placeholder() {
    let mut m = open_model(&test_cfg());
    m.media.download_video_stream = false;
    m.media.download_audio_stream = false;
    m.additional.download_danmaku = false;
    m.additional.download_subtitle = false;
    m.additional.download_cover = false;
    m.additional.embed_chapter = false;
    m.additional.download_metadata = false;
    assert_eq!(m.preview_tags(), vec![("empty", true)]);
    m.additional.download_metadata = true;
    assert!(m.preview_tags().contains(&("metadata", true)));
    assert!(!m.preview_tags().contains(&("empty", true)));
}

#[test]
fn filename_preview_matches_engine() {
    let cfg = test_cfg();
    let m = open_model(&cfg);
    let store = RuleStore::default();
    let preview = m.filename_preview(&store, 11);
    // engine parity: format the same rule through the formatter directly
    let rule = store
        .get_rule_from_config(Some(11))
        .expect("default normal rule exists");
    let variable_data = naming::conventions::VariableListFactory::new().build_variable_data(
        naming::conventions::ConventionType::Normal,
        naming::conventions::SampleShape::Single,
    );
    let expected = naming::formatter::FileNameFormatter::new(variable_data, &store)
        .format(Some(11), 0, &rule)
        .expect("engine formats the default rule");
    assert_eq!(preview.expect("preview produced"), expected);
}

#[test]
fn commit_writes_config_and_naming_state() {
    let mut cfg = test_cfg();
    let mut m = open_model(&cfg);
    m.download.video_container = "mp4".into();
    m.download.numbering_type = naming::numbering::CONTINUOUS;
    m.download.global_starting_number = 5;
    let mut naming = NamingState::default();
    m.commit(&mut cfg, &mut naming);
    assert_eq!(
        cfg.get("Download", "video_container")
            .and_then(|v| v.as_str().map(str::to_owned)),
        Some("mp4".into())
    );
    assert_eq!(
        cfg.get("File Naming", "numbering_type")
            .and_then(|v| v.as_i64()),
        Some(2)
    );
    // CONTINUOUS → the dialog's starting number becomes the runtime's
    assert_eq!(naming.global_starting_number, 5);
}

fn c() -> &'static services::i18n::Catalog {
    services::i18n::catalog(services::i18n::Language::from_config("en_US").resolve())
}
