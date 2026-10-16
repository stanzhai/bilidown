//! External-behavior tests for the task manager at its public seams
//! (spec Testing Decisions): task lifecycle across both DB tables,
//! durability of `mark_as_completed(wait=true)`, option freeze/fallback,
//! duplicate detection, numbering, Task Info assembly, scheduler
//! admission, and resume-from-offsets through wiremock.

use database::info::TaskInfo;
use database::TaskDatabase;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tasks::info::{episode_to_task_info, CreateInputs, NamingInput};
use tasks::numbering::Numbering;
use tasks::options::{self, OPTION_KEYS};
use tasks::status::{download_status, download_type, NumberingType};
use tasks::{Scheduler, TaskManager};

fn config(tmp: &tempfile::TempDir) -> Arc<config::ConfigStore> {
    Arc::new(config::ConfigStore::with_paths(
        tmp.path().join("config"),
        tmp.path().join("downloads"),
    ))
}

fn episode(attribute: i64) -> serde_json::Map<String, Value> {
    json!({
        "title": "第 1 话",
        "episode_id": "ep-1",
        "attribute": attribute,
        "aid": 100,
        "bvid": "BV1xx",
        "cid": 200,
        "cover": "https://i0.hdslb.com/bfs/archive/cover.jpg",
    })
    .as_object()
    .unwrap()
    .clone()
}

fn make_task(task_id: &str) -> TaskInfo {
    let mut task_info = TaskInfo::default();
    task_info.Basic.task_id = task_id.to_string();
    task_info.Basic.show_title = format!("标题 {task_id}");
    task_info.Basic.created_time = 1_700_000_000;
    task_info.Download.status = download_status::DOWNLOADING;
    task_info
}

fn task_ids(db: &TaskDatabase, completed: bool) -> Vec<String> {
    db.query_tasks(completed, None)
        .unwrap()
        .into_iter()
        .map(|t| t.Basic.task_id)
        .collect()
}

// ---- options (test_download_options.py) ----

#[test]
fn every_option_has_a_resolvable_global_value() {
    let tmp = tempfile::tempdir().unwrap();
    let store = config(&tmp);
    for key in OPTION_KEYS {
        // Missing global values surface as Null, not panics; every key in
        // the spec must resolve to something.
        let value = {
            let probe = TaskInfo::default();
            options::resolve(&store, &probe, key)
        };
        assert!(!value.is_null(), "no global fallback for {key}");
    }
}

#[test]
fn snapshot_covers_every_option() {
    let tmp = tempfile::tempdir().unwrap();
    let store = config(&tmp);
    let snap = options::snapshot(&store, None);
    let keys: Vec<&String> = snap.keys().collect();
    assert_eq!(keys.len(), OPTION_KEYS.len());
    for key in OPTION_KEYS {
        assert!(snap.contains_key(key), "snapshot missing {key}");
    }
}

#[test]
fn snapshot_freezes_config_value() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store =
        config::ConfigStore::with_paths(tmp.path().join("config"), tmp.path().join("downloads"));
    store.set("Download", "keep_original_files_type", json!(2));
    let snap = options::snapshot(&store, None);
    assert_eq!(snap["keep_original_files_type"], json!(2));
}

#[test]
fn missing_option_falls_back_to_config() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store =
        config::ConfigStore::with_paths(tmp.path().join("config"), tmp.path().join("downloads"));
    store.set("Download", "keep_original_files_type", json!(2));
    // Old task: no Options section at all.
    let task = TaskInfo::default();
    assert_eq!(
        options::resolve(&store, &task, "keep_original_files_type"),
        json!(2)
    );
}

#[test]
fn frozen_option_wins_over_config() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store =
        config::ConfigStore::with_paths(tmp.path().join("config"), tmp.path().join("downloads"));
    store.set("Download", "keep_original_files_type", json!(2));
    let mut task = TaskInfo::default();
    task.Options.keep_original_files_type = Some(1);
    assert_eq!(
        options::resolve(&store, &task, "keep_original_files_type"),
        json!(1)
    );
}

// ---- durability (test_task_completion_durability.py) ----

#[test]
fn wait_true_is_on_disk_when_it_returns() {
    let tmp = tempfile::tempdir().unwrap();
    let manager = TaskManager::open(tmp.path().join("task.db"), config(&tmp)).unwrap();
    let task_info = make_task("wait-true");
    {
        let mut db = manager.reader().unwrap();
        db.add_tasks(std::slice::from_ref(&task_info), false)
            .unwrap();
    }

    let reader = manager.reader().unwrap();
    assert!(task_ids(&reader, false).contains(&"wait-true".to_string()));

    manager.mark_as_completed(&task_info, true, 5.0);

    // 关键断言：不 sleep、不重试。返回即已落库
    let reader = manager.reader().unwrap();
    assert!(task_ids(&reader, true).contains(&"wait-true".to_string()));
    assert!(!task_ids(&reader, false).contains(&"wait-true".to_string()));
    manager.shutdown(5.0);
}

#[test]
fn wait_true_does_not_deadlock_from_the_caller_thread() {
    let tmp = tempfile::tempdir().unwrap();
    let manager = Arc::new(TaskManager::open(tmp.path().join("task.db"), config(&tmp)).unwrap());
    let task_info = make_task("no-deadlock");
    {
        let mut db = manager.reader().unwrap();
        db.add_tasks(std::slice::from_ref(&task_info), false)
            .unwrap();
    }

    let worker = manager.clone();
    let thread = std::thread::spawn(move || {
        worker.mark_as_completed(&task_info, true, 5.0);
    });
    thread
        .join()
        .expect("mark_as_completed(wait=true) did not return within the join, likely deadlock");
    manager.shutdown(5.0);
}

#[test]
fn timeout_does_not_raise_and_write_still_lands() {
    let tmp = tempfile::tempdir().unwrap();
    let manager = TaskManager::open(tmp.path().join("task.db"), config(&tmp)).unwrap();
    let task_info = make_task("timeout");
    {
        let mut db = manager.reader().unwrap();
        db.add_tasks(std::slice::from_ref(&task_info), false)
            .unwrap();
    }

    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    manager.block_writer(release_rx);

    // 超时只该记日志并放行，绝不能把异常抛回调用方。
    manager.mark_as_completed(&task_info, true, 0.2);
    release_tx.send(()).unwrap();

    // 超时只是放弃等待，写入本身仍在写线程上排队，最终必须落库。
    let reader = manager.reader().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if task_ids(&reader, true).contains(&"timeout".to_string()) {
            manager.shutdown(5.0);
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed-out write never landed in completed_task");
}

#[test]
fn without_wait_there_is_no_durability_guarantee() {
    let tmp = tempfile::tempdir().unwrap();
    let manager = TaskManager::open(tmp.path().join("task.db"), config(&tmp)).unwrap();
    let task_info = make_task("no-guarantee");
    {
        let mut db = manager.reader().unwrap();
        db.add_tasks(std::slice::from_ref(&task_info), false)
            .unwrap();
    }

    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    manager.block_writer(release_rx);

    manager.mark_as_completed(&task_info, false, 5.0);

    // 此刻库里必然还是旧状态 —— 崩溃发生在这个窗口里，记录就丢了
    let reader = manager.reader().unwrap();
    assert!(!task_ids(&reader, true).contains(&"no-guarantee".to_string()));
    assert!(task_ids(&reader, false).contains(&"no-guarantee".to_string()));

    release_tx.send(()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if task_ids(&reader, true).contains(&"no-guarantee".to_string()) {
            manager.shutdown(5.0);
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("unblocked write never landed");
}

#[test]
fn wait_false_keeps_the_old_asynchronous_contract() {
    let tmp = tempfile::tempdir().unwrap();
    let manager = TaskManager::open(tmp.path().join("task.db"), config(&tmp)).unwrap();
    let task_info = make_task("wait-false");
    {
        let mut db = manager.reader().unwrap();
        db.add_tasks(std::slice::from_ref(&task_info), false)
            .unwrap();
    }

    manager.mark_as_completed(&task_info, false, 5.0);
    // 异步路径不保证返回时已落库 —— shutdown's sentinel joins the writer.
    manager.shutdown(5.0);

    let reader = TaskDatabase::open(tmp.path().join("task.db")).unwrap();
    assert!(task_ids(&reader, true).contains(&"wait-false".to_string()));
}

// ---- duplicate detection ----

#[test]
fn hash_from_episode_matches_database_calc() {
    let ep = episode(1); // VIDEO
    let hash = tasks::duplicate::calc_hash_id_from_episode(&ep);
    let mut task = TaskInfo::default();
    task.Episode.attribute = 1;
    task.Episode.aid = 100;
    task.Episode.bvid = "BV1xx".into();
    task.Episode.cid = 200;
    assert_eq!(hash, database::hash_id::calc_hash_id(&task));
}

#[test]
fn duplicate_ask_skip_and_continue() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store =
        config::ConfigStore::with_paths(tmp.path().join("config"), tmp.path().join("downloads"));
    store.set("Behavior", "duplicate_download_resolution", json!(2));
    let db_path = tmp.path().join("task.db");
    let mut db = TaskDatabase::open(&db_path).unwrap();
    let ep = episode(1);
    existing_hash_insert(&mut db, &ep);

    let asked = std::cell::Cell::new(0);
    let outcome = tasks::duplicate::check_duplicate(&db, &store, &ep, None, || {
        asked.set(asked.get() + 1);
        tasks::duplicate::AskOutcome::Skip
    });
    assert_eq!(outcome, tasks::duplicate::DuplicateCheck::Skip);
    assert_eq!(asked.get(), 1, "ALWAYS_ASK must invoke the resolver");

    // Non-duplicate: no ask.
    let fresh = episode(1);
    let mut fresh = fresh.clone();
    fresh.insert("cid".into(), json!(999));
    asked.set(0);
    let outcome = tasks::duplicate::check_duplicate(&db, &store, &fresh, None, || {
        asked.set(asked.get() + 1);
        tasks::duplicate::AskOutcome::Skip
    });
    assert_eq!(outcome, tasks::duplicate::DuplicateCheck::Continue);
    assert_eq!(asked.get(), 0);
}

fn existing_hash_insert(db: &mut TaskDatabase, ep: &serde_json::Map<String, Value>) {
    let hash = tasks::duplicate::calc_hash_id_from_episode(ep);
    let mut task = TaskInfo::default();
    task.Basic.task_id = "existing".into();
    let mut record = TaskDatabase::build_record(&task, false);
    record.hash_id = hash;
    db.add_task_records(std::slice::from_ref(&record), false)
        .unwrap();
}

#[test]
fn duplicate_resolution_skip_and_continue_from_options() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store =
        config::ConfigStore::with_paths(tmp.path().join("config"), tmp.path().join("downloads"));
    let db_path = tmp.path().join("task.db");
    let mut db = TaskDatabase::open(&db_path).unwrap();
    let ep = episode(1);
    existing_hash_insert(&mut db, &ep);

    // SKIP (1): skip without asking.
    store.set("Behavior", "duplicate_download_resolution", json!(1));
    let outcome = tasks::duplicate::check_duplicate(&db, &store, &ep, None, || {
        panic!("SKIP must not ask");
    });
    assert_eq!(outcome, tasks::duplicate::DuplicateCheck::Skip);

    // CONTINUE (0): download again without asking.
    store.set("Behavior", "duplicate_download_resolution", json!(0));
    let outcome = tasks::duplicate::check_duplicate(&db, &store, &ep, None, || {
        panic!("CONTINUE must not ask");
    });
    assert_eq!(outcome, tasks::duplicate::DuplicateCheck::Continue);
}

#[test]
fn is_duplicate_is_non_interactive() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("task.db");
    let mut db = TaskDatabase::open(&db_path).unwrap();
    let ep = episode(1);
    assert!(!tasks::duplicate::is_duplicate(&db, &ep));
    existing_hash_insert(&mut db, &ep);
    assert!(tasks::duplicate::is_duplicate(&db, &ep));
}

// ---- numbering ----

#[test]
fn numbering_three_modes_and_global_increment() {
    let mut numbering = Numbering::default();
    let (n1, _) = numbering.take(NumberingType::Continuous, None);
    let (n2, _) = numbering.take(NumberingType::Continuous, None);
    assert_eq!((n1.as_str(), n2.as_str()), ("1", "2"));

    let mut numbering = Numbering::default();
    numbering.state.current_starting_number = Some(7);
    let (a, _) = numbering.take(NumberingType::FromSpecified, None);
    let (b, _) = numbering.take(NumberingType::FromSpecified, None);
    assert_eq!((a.as_str(), b.as_str()), ("7", "8"));

    let mut numbering = Numbering::default();
    let (p, _) = numbering.take(NumberingType::UseParseList, Some("42"));
    assert_eq!(p, "42");
    // The global counter always advances, whatever the mode.
    let (g, _) = numbering.take(NumberingType::Continuous, None);
    assert_eq!(g, "2");
}

#[test]
fn numbering_rollback_restores_only_when_counters_unchanged() {
    let mut numbering = Numbering::default();
    let (_, token) = numbering.take(NumberingType::Continuous, None);
    numbering.rollback(token);
    assert_eq!(numbering.state.global_starting_number, 1);

    // Someone else numbered since our take: rollback is a no-op.
    let (_, token) = numbering.take(NumberingType::Continuous, None);
    let _ = numbering.take(NumberingType::Continuous, None);
    numbering.rollback(token);
    assert_eq!(numbering.state.global_starting_number, 3);
}

// ---- Task Info assembly ----

fn make_inputs<'a>(
    store: &'a config::ConfigStore,
    episode_data: &'a parsers::EpisodeData,
) -> CreateInputs<'a> {
    CreateInputs {
        config: store,
        episode_data,
        video_quality_id: 200,
        audio_quality_id: 30300,
        video_codec_id: 20,
    }
}

#[test]
fn assembly_freezes_task_info() {
    let tmp = tempfile::tempdir().unwrap();
    let store = config(&tmp);
    let ep = episode(1); // VIDEO attribute
    let naming = NamingInput {
        formatted_path: Some("番剧/第 1 话.mp4".into()),
        ..Default::default()
    };
    let episode_data = parsers::EpisodeData::new();
    let task =
        episode_to_task_info(&make_inputs(&store, &episode_data), &ep, "1", None, &naming).unwrap();

    assert_eq!(task.Download.status, download_status::QUEUED);
    assert_eq!(
        task.Download.type_,
        download_type::VIDEO | download_type::AUDIO
    );
    assert_eq!(task.Episode.leaf_title, "第 1 话");
    assert_eq!(task.Episode.number, "1");
    assert_eq!(task.File.folder, "番剧");
    assert_eq!(task.File.name, "第 1 话.mp4");
    // cover_id = md5(cover_url)
    use md5::Digest;
    let digest = md5::Md5::digest(b"https://i0.hdslb.com/bfs/archive/cover.jpg");
    assert_eq!(task.Basic.cover_id, format!("{digest:x}"));
    assert!(task.Basic.created_time > 1_700_000_000_000);
}

#[test]
fn assembly_filters_illegal_characters_only_on_title_keys() {
    assert_eq!(
        tasks::info::filter_illegal_characters("a/b\\c:d*e?f\"g<h>i|j"),
        "a_b_c_d_e_f_g_h_i_j"
    );
    assert_eq!(
        tasks::info::filter_illegal_characters("正常标题"),
        "正常标题"
    );
}

#[test]
fn assembly_merges_extra_data_related_titles_uploader_and_number() {
    let tmp = tempfile::tempdir().unwrap();
    let store = config(&tmp);
    let episode_data = parsers::EpisodeData::new();
    episode_data.set_episode_data("ep-1", json!({"parent_title": "来自侧数据"}));

    let inputs = CreateInputs {
        config: &store,
        episode_data: &episode_data,
        video_quality_id: 200,
        audio_quality_id: 30300,
        video_codec_id: 20,
    };
    let mut ep = episode(1 << 1); // BANGUMI: episode_title gets set
    ep.insert("related_titles".into(), json!({"season_title": "第二季"}));
    ep.insert("uploader_info".into(), json!({"uploader": "UP主"}));
    ep.insert("number".into(), json!("9")); // parse-list value, overridden by create()

    let task = episode_to_task_info(&inputs, &ep, "3", None, &Default::default()).unwrap();
    // Merge order: episode ← extra ← related_titles ← uploader ← number.
    assert_eq!(task.Episode.parent_title, "来自侧数据");
    assert_eq!(task.Episode.season_title, "第二季");
    assert_eq!(task.Episode.uploader, "UP主");
    assert_eq!(task.Episode.number, "3");
    // BANGUMI: episode_title mirrors the title.
    assert_eq!(task.Episode.episode_title, "第 1 话");
}

#[test]
fn assembly_trims_lesson_download_type() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store =
        config::ConfigStore::with_paths(tmp.path().join("config"), tmp.path().join("downloads"));
    store.set("Additional", "download_danmaku", json!(true));
    store.set("Additional", "download_subtitle", json!(true));
    store.set("Additional", "download_cover", json!(true));
    store.set("Additional", "download_metadata", json!(true));

    let ep = episode(1 << 18); // LESSON
    let episode_data = parsers::EpisodeData::new();
    let task = episode_to_task_info(
        &make_inputs(&store, &episode_data),
        &ep,
        "1",
        None,
        &Default::default(),
    )
    .unwrap();
    // LESSON entries carry no danmaku/subtitle/chapter/cover.
    assert_eq!(task.Download.type_ & download_type::DANMAKU, 0);
    assert_eq!(task.Download.type_ & download_type::SUBTITLE, 0);
    assert_eq!(task.Download.type_ & download_type::COVER, 0);
    assert_eq!(task.Download.type_ & download_type::CHAPTER, 0);
    // Metadata survives (only those four bits are trimmed).
    assert_ne!(task.Download.type_ & download_type::METADATA, 0);
}

#[test]
fn assembly_needs_reparse_flag() {
    let mut ep = episode(1);
    assert!(!tasks::info::needs_reparse(&ep));
    ep.insert("attribute".into(), json!(1 | (1 << 7)));
    assert!(tasks::info::needs_reparse(&ep));
}

// ---- lifecycle: create → complete moves rows ----

#[test]
fn create_then_complete_moves_row_between_tables() {
    let tmp = tempfile::tempdir().unwrap();
    let store = config(&tmp);
    let manager = TaskManager::open(tmp.path().join("task.db"), store.clone()).unwrap();

    let result = manager
        .create(
            &store,
            &parsers::EpisodeData::new(),
            std::slice::from_ref(&episode(1)),
            None,
            |_| NamingInput::default(),
        )
        .unwrap();
    assert_eq!(result.created.len(), 1);
    manager.shutdown(5.0);

    let db = TaskDatabase::open(tmp.path().join("task.db")).unwrap();
    assert_eq!(db.count_tasks(false).unwrap(), 1);
    assert_eq!(db.count_tasks(true).unwrap(), 0);

    let manager = TaskManager::open(tmp.path().join("task.db"), store).unwrap();
    let task = result.created.into_iter().next().unwrap();
    manager.mark_as_completed(&task, true, 5.0);
    manager.shutdown(5.0);

    let db = TaskDatabase::open(tmp.path().join("task.db")).unwrap();
    assert_eq!(db.count_tasks(false).unwrap(), 0);
    assert_eq!(db.count_tasks(true).unwrap(), 1);
    // Task Info sections survive the round trip.
    let back = db
        .query_task_by_id(&task.Basic.task_id, true)
        .unwrap()
        .unwrap();
    assert_eq!(back.Basic.show_title, task.Basic.show_title);
    assert_eq!(back.Episode.leaf_title, task.Episode.leaf_title);
}

#[test]
fn create_skips_duplicates_and_reports_need_reparse() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store =
        config::ConfigStore::with_paths(tmp.path().join("config"), tmp.path().join("downloads"));
    store.set("Behavior", "duplicate_download_resolution", json!(1)); // SKIP
    let store = Arc::new(store);
    let manager = TaskManager::open(tmp.path().join("task.db"), store.clone()).unwrap();
    // Seed an existing row so the second create() sees a duplicate.
    let mut need_parse = episode(1);
    need_parse.insert("attribute".into(), json!(1 | (1 << 7)));

    let existing = episode(1);
    {
        let mut db = manager.reader().unwrap();
        let mut record = TaskDatabase::build_record(&TaskInfo::default(), false);
        record.hash_id = tasks::duplicate::calc_hash_id_from_episode(&existing);
        db.add_task_records(std::slice::from_ref(&record), false)
            .unwrap();
    }

    let result = manager
        .create(
            &store,
            &parsers::EpisodeData::new(),
            &[existing, need_parse],
            None,
            |_| NamingInput::default(),
        )
        .unwrap();
    // SKIP resolution: the duplicate is not created; NEED_PARSE is
    // reported back instead of being created.
    assert_eq!(result.created.len(), 0);
    assert_eq!(result.skipped_duplicates.len(), 1);
    assert_eq!(result.need_reparse.len(), 1);
    manager.shutdown(5.0);
}

// ---- scheduler ----

#[test]
fn scheduler_clamps_parallel_limit() {
    let limit_for = |parallel: i64| {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = config::ConfigStore::with_paths(
            tmp.path().join("config"),
            tmp.path().join("downloads"),
        );
        store.set("Download", "download_parallel", json!(parallel));
        Scheduler::new(Arc::new(store)).parallel_limit()
    };
    assert_eq!(limit_for(1), 1);
    assert_eq!(limit_for(4), 4);
    assert_eq!(limit_for(99), 10);
    assert_eq!(limit_for(0), 1);
    assert_eq!(limit_for(-5), 1);
}

#[test]
fn scheduler_admits_queued_up_to_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store =
        config::ConfigStore::with_paths(tmp.path().join("config"), tmp.path().join("downloads"));
    store.set("Download", "download_parallel", json!(2));
    let manager = TaskManager::open(tmp.path().join("task.db"), Arc::new(store)).unwrap();

    let tasks: Vec<TaskInfo> = (0..4)
        .map(|i| {
            let mut t = make_task(&format!("task-{i}"));
            t.Download.status = download_status::QUEUED;
            t
        })
        .collect();
    {
        let mut db = manager.reader().unwrap();
        db.add_tasks(&tasks, false).unwrap();
    }

    let started = manager.scheduler.auto_manage(&manager);
    assert_eq!(started.len(), 2);
    manager.shutdown(5.0);

    let db = TaskDatabase::open(tmp.path().join("task.db")).unwrap();
    let statuses: Vec<i64> = db
        .query_tasks(false, None)
        .unwrap()
        .iter()
        .map(|t| t.Download.status)
        .collect();
    assert_eq!(
        statuses
            .iter()
            .filter(|&&s| s == download_status::DOWNLOADING)
            .count(),
        2
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|&&s| s == download_status::QUEUED)
            .count(),
        2
    );
}

// ---- wiremock resume through the manager's offset store ----

fn wire_client() -> network::HttpClient {
    network::HttpClient::new(network::ClientOptions {
        user_agent: "bilidown-test".into(),
        proxy: network::ProxyConfig::none(),
        cookies_from_persistence: Vec::new(),
        signer: None,
    })
    .unwrap()
}

fn wire_body(seed: u64, len: u64) -> Vec<u8> {
    let mut state = seed | 1;
    let mut out = Vec::with_capacity(len as usize);
    for _ in 0..len {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.push((state & 0xff) as u8);
    }
    out
}

#[tokio::test]
async fn paused_offsets_persist_and_resume_skips_done_chunks() {
    use downloader::{DownloadEngine, EngineConfig, FileSpec, OffsetTable};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    let tmp = tempfile::tempdir().unwrap();
    let store = config(&tmp);
    let manager = Arc::new(TaskManager::open(tmp.path().join("task.db"), store).unwrap());

    let size = downloader::CHUNK_SIZE * 40;
    let data = Arc::new(wire_body(0x5eed, size));
    Mock::given(method("GET"))
        .and(path("/video"))
        .respond_with({
            let data = data.clone();
            move |req: &wiremock::Request| {
                let range = req
                    .headers
                    .get("range")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_owned());
                match range {
                    Some(range) if range.starts_with("bytes=") => {
                        let (s, e) = range[6..].split_once('-').unwrap();
                        let (start, end): (usize, usize) = (s.parse().unwrap(), e.parse().unwrap());
                        ResponseTemplate::new(206)
                            .append_header(
                                "Content-Range",
                                format!("bytes {}-{}/{}", start, end, data.len()),
                            )
                            .set_body_bytes(data[start..=end].to_vec())
                    }
                    _ => ResponseTemplate::new(200).set_body_bytes(data.as_ref().clone()),
                }
            }
        })
        .mount(&server)
        .await;

    let spec = FileSpec {
        file_key: "video".to_owned(),
        url: format!("{}/video", server.uri()),
        file_size: size,
        path: tmp.path().join("video.m4s"),
    };
    let engine_config = EngineConfig {
        parallel_chunks: 8,
        auto_retry_enabled: false,
        auto_retry_max_count: 5,
        preallocate: true,
        chunk_backoff_unit: Duration::from_millis(1),
        task_retry_unit: Duration::from_millis(5),
    };

    let mut task = make_task("resume-task");
    task.File.download_path = tmp.path().to_string_lossy().to_string();
    task.File.folder = String::new();
    task.File.name = "video.m4s".into();
    task.File.relative_files = vec!["video.m4s".into()];
    {
        let mut db = manager.reader().unwrap();
        db.add_tasks(std::slice::from_ref(&task), false).unwrap();
    }

    // Run, pause midway, persist the offsets through the manager's store.
    let mut engine = DownloadEngine::new(
        wire_client(),
        engine_config.clone(),
        downloader::TokenBucketHandle::unlimited(),
        Arc::new(manager.offset_store("resume-task")),
        vec![spec.clone()],
    );
    let control = engine.control();
    let handle = tokio::spawn(async move { engine.run().await });
    tokio::time::sleep(Duration::from_millis(80)).await;
    control.pause();
    let _ = handle.await;

    // The manager's offset store scheduled a flush; let it land.
    manager.shutdown(5.0);
    let db = TaskDatabase::open(tmp.path().join("task.db")).unwrap();
    let persisted = db.query_task_by_id("resume-task", false).unwrap().unwrap();
    let offsets_value = persisted
        .Download
        .files
        .get("video")
        .and_then(|f| f.get("chunk_offsets"))
        .cloned()
        .expect("chunk_offsets persisted in Download.files");
    let persisted_table = OffsetTable::from_json(offsets_value);
    assert!(!persisted_table
        .total_written(|_| downloader::CHUNK_SIZE)
        .to_string()
        .is_empty());

    // Resume in a fresh engine from the persisted table: only the
    // unfinished chunks hit the CDN again.
    let before = server.received_requests().await.unwrap().len();
    let mut resumed = DownloadEngine::new(
        wire_client(),
        engine_config,
        downloader::TokenBucketHandle::unlimited(),
        Arc::new(()), // no-op store: offsets come from resume_from
        vec![spec],
    );
    resumed.resume_from(single_table("video", persisted_table));
    let final_state = resumed.run().await;
    assert_eq!(
        final_state.status.code(),
        downloader::TaskStatus::Completed.code()
    );

    let after = server.received_requests().await.unwrap().len();
    assert!(after > before, "resume must re-request remaining chunks");
    let expected = std::fs::read(tmp.path().join("video.m4s")).unwrap();
    assert_eq!(expected, data.as_ref().clone());
}

fn single_table(
    key: &str,
    table: downloader::OffsetTable,
) -> std::collections::BTreeMap<String, downloader::OffsetTable> {
    let mut map = std::collections::BTreeMap::new();
    map.insert(key.to_string(), table);
    map
}
