//! `options.py resolve()` for the extras keys: task snapshot wins, unknown
//! enum values fall back to global config.

mod common;

use common::*;

#[test]
fn task_snapshot_wins() {
    let mut task = database::TaskInfo::default();
    task.Options.subtitle_type = Some("srt".to_owned());
    task.Options.embed_subtitle = Some(true);
    let store = empty_store(); // 全局 subtitle_type = ass, embed_subtitle = false

    assert_eq!(
        extras::options::resolve(&task, &store, "subtitle_type"),
        serde_json::json!("srt")
    );
    assert_eq!(
        extras::options::resolve(&task, &store, "embed_subtitle"),
        serde_json::json!(true)
    );
}

#[test]
fn none_falls_back_to_global() {
    let task = database::TaskInfo::default();
    let store = empty_store();
    assert_eq!(
        extras::options::resolve(&task, &store, "subtitle_type"),
        serde_json::json!("ass")
    );
    assert_eq!(
        extras::options::resolve(&task, &store, "cover_type"),
        serde_json::json!("jpg")
    );
    assert_eq!(
        extras::options::resolve(&task, &store, "metadata_type"),
        serde_json::json!("nfo")
    );
    assert_eq!(
        extras::options::resolve(&task, &store, "embed_subtitle"),
        serde_json::json!(false)
    );
    assert_eq!(
        extras::options::resolve(&task, &store, "subtitle_language"),
        serde_json::json!({"download_specified": false, "specified_language": []})
    );
}

#[test]
fn unknown_enum_string_falls_back_to_global() {
    let mut task = database::TaskInfo::default();
    task.Options.subtitle_type = Some("bogus".to_owned());
    task.Options.cover_type = Some("tiff".to_owned());
    let store = empty_store();
    assert_eq!(
        extras::options::resolve(&task, &store, "subtitle_type"),
        serde_json::json!("ass")
    );
    assert_eq!(
        extras::options::resolve(&task, &store, "cover_type"),
        serde_json::json!("jpg")
    );
}

#[test]
fn every_known_enum_value_passes_through() {
    let mut task = database::TaskInfo::default();
    let store = empty_store();
    for v in ["srt", "lrc", "txt", "ass", "json"] {
        task.Options.subtitle_type = Some(v.to_owned());
        assert_eq!(
            extras::options::resolve(&task, &store, "subtitle_type"),
            serde_json::json!(v)
        );
    }
    for v in ["jpg", "png", "avif", "webp"] {
        task.Options.cover_type = Some(v.to_owned());
        assert_eq!(
            extras::options::resolve(&task, &store, "cover_type"),
            serde_json::json!(v)
        );
    }
    for v in ["nfo", "json"] {
        task.Options.metadata_type = Some(v.to_owned());
        assert_eq!(
            extras::options::resolve(&task, &store, "metadata_type"),
            serde_json::json!(v)
        );
    }
}
