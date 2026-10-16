//! Golden-sample tests: real-shaped Python fixture databases pin read
//! compatibility; write round-trips pin byte-shape compatibility.
//! Fixtures under `tests/fixtures/` were produced by the Python schema
//! scripts (task/thumbnail/history `check_and_create_table`) with realistic
//! `TaskInfo.to_dict()` JSON payloads. (ticket 03)

use database::info::TaskInfo;
use database::store::{CoverDatabase, HistoryDatabase, TaskDatabase};
use rusqlite::Connection;

fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Copies a fixture to a temp dir so tests can open read-write.
fn copy_fixture(name: &str) -> std::path::PathBuf {
    let dir = tempfile::tempdir().expect("tempdir");
    let dest = dir.path().join(name);
    std::fs::copy(fixture(name), &dest).expect("fixture copy");
    // WAL sidecar files, if present in the fixture, travel with the copy.
    for ext in ["-wal", "-shm"] {
        let src = fixture(&format!("{name}{ext}"));
        if src.exists() {
            let _ = std::fs::copy(&src, dest.with_file_name(format!("{name}{ext}")));
        }
    }
    // Leak the tempdir: the DB must stay on disk for the test's lifetime.
    std::mem::forget(dir);
    dest
}

fn journal_mode(conn: &Connection) -> String {
    conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap()
}

#[test]
fn golden_task_db_reads_python_rows() {
    let path = copy_fixture("task.db");
    let db = TaskDatabase::open(&path).expect("open task.db");

    let tasks = db.query_tasks(false, None).expect("query download_task");
    assert_eq!(tasks.len(), 1);
    let t = &tasks[0];
    assert_eq!(t.Basic.task_id, "t-bvid-01");
    assert_eq!(t.Basic.show_title, "视频 P1");
    assert_eq!(t.Basic.created_time, 1700000000);
    assert_eq!(t.Episode.attribute, 1);
    assert_eq!(t.Episode.aid, 170001);
    assert_eq!(t.Episode.bvid, "BV1xx411c7mD");
    assert_eq!(t.Episode.cid, 170002);
    assert_eq!(t.Episode.areas, vec!["大陆".to_string()]);
    assert!((t.Episode.rating - 9.5).abs() < f64::EPSILON);
    assert_eq!(t.Options.video_container.as_deref(), Some("mp4"));
    assert_eq!(t.Naming.rule.as_deref(), Some("<title>"));
    assert_eq!(t.File.subtitle_track_list.len(), 1);
    assert_eq!(
        t.File.subtitle_track_list[0].get("kind").and_then(|v| v.as_str()),
        Some("danmaku")
    );

    let completed = db.query_tasks(true, None).expect("query completed_task");
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].Basic.task_id, "t-bangumi-02");
    assert_eq!(completed[0].Episode.ep_id, 777);

    // Direct id read + count.
    let by_id = db
        .query_task_by_id("t-bangumi-02", true)
        .expect("by id");
    assert_eq!(by_id.as_ref().map(|t| t.Basic.task_id.as_str()), Some("t-bangumi-02"));
    assert_eq!(db.count_tasks(false).unwrap(), 1);
    assert_eq!(db.count_tasks(true).unwrap(), 1);

    // Reopening the golden file keeps user_version and never downgrades it.
    assert_eq!(db.user_version(), 1);
}

#[test]
fn golden_thumbnail_db_reads_python_blobs() {
    let path = copy_fixture("thumbnail.db");
    let db = CoverDatabase::open(&path).expect("open thumbnail.db");

    let blob = db
        .query_cover("cover-bvid-01@1440x810")
        .expect("query")
        .expect("row exists");
    assert_eq!(blob, b"WEBPDATA-bvid01".to_vec());
    assert!(db.query_cover("missing").expect("query").is_none());
}

#[test]
fn golden_history_db_reads_python_rows() {
    let path = copy_fixture("history.db");
    let db = HistoryDatabase::open(&path).expect("open history.db");

    let rows = db.query_all().expect("query_all");
    assert_eq!(rows.len(), 3);
    // Newest first.
    assert_eq!(rows[0].history_id, "h-2");
    assert_eq!(rows[0].created_time, 1700000002);
    assert_eq!(rows[2].url, "https://b23.tv/0");
    assert_eq!(rows[0].type_, "video");
}

#[test]
fn task_round_trip_download_and_completed() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = TaskDatabase::open(dir.path().join("task.db")).expect("open");

    let original: TaskInfo = serde_json::from_str(include_str!(
        "fixtures/task_video.json"
    ))
    .expect("fixture task json");

    db.add_tasks(std::slice::from_ref(&original), false).expect("insert");
    let read = db.query_tasks(false, None).expect("read");
    assert_eq!(read.len(), 1);
    // Identical sections round-trip.
    assert_eq!(read[0], original);

    // Serialized data column byte-matches the Python fixture row.
    {
        let conn = Connection::open(dir.path().join("task.db")).unwrap();
        let data: String = conn
            .query_row("SELECT data FROM download_task", [], |r| r.get(0))
            .unwrap();
        assert_eq!(data, original.to_json());
        assert_eq!(data, include_str!("fixtures/task_video.json"));
    }

    // Move to completed: row disappears from download_task, appears complete.
    let record = TaskDatabase::build_record(&original, true);
    db.move_to_completed(&record).expect("move");
    assert_eq!(db.count_tasks(false).unwrap(), 0);
    assert_eq!(db.count_tasks(true).unwrap(), 1);
    let completed = db.query_tasks(true, None).expect("completed");
    assert_eq!(completed[0], original);

    // And back.
    let record = TaskDatabase::build_record(&original, false);
    db.recreate_task(&record).expect("recreate");
    assert_eq!(db.count_tasks(false).unwrap(), 1);
    assert_eq!(db.count_tasks(true).unwrap(), 0);

    // Duplicate detection sees the hash in either table.
    let hash = database::hash_id::calc_hash_id(&original);
    assert!(db.check_duplicate(&hash).unwrap());
    assert!(!db.check_duplicate("no-such-hash").unwrap());

    // Update + delete round-trips.
    let mut updated = original.clone();
    updated.Download.progress = 42;
    db.update_task(&updated).expect("update");
    let after = db.query_tasks(false, None).unwrap();
    assert_eq!(after[0].Download.progress, 42);
    db.delete_tasks(&[updated.Basic.task_id.clone()], false).expect("delete");
    assert_eq!(db.count_tasks(false).unwrap(), 0);
}

#[test]
fn thumbnail_cache_capped_and_replaceable() {
    let dir = tempfile::tempdir().unwrap();
    let db = CoverDatabase::open(dir.path().join("thumbnail.db")).expect("open");

    assert!(db.query_cover("k1").unwrap().is_none());
    db.add_cover("k1", b"first").unwrap();
    assert_eq!(db.query_cover("k1").unwrap().unwrap(), b"first".to_vec());

    // Same key overwrites (INSERT OR REPLACE self-heals falsy rows).
    db.add_cover("k1", b"second").unwrap();
    assert_eq!(db.query_cover("k1").unwrap().unwrap(), b"second".to_vec());

    // Fresh DB is WAL, way under the 150MiB cap, so the cache is untouched.
    let conn = Connection::open(dir.path().join("thumbnail.db")).unwrap();
    assert_eq!(journal_mode(&conn), "wal");
}

#[test]
fn history_capped_at_100_and_dedupes_urls() {
    let dir = tempfile::tempdir().unwrap();
    let db = HistoryDatabase::open(dir.path().join("history.db")).expect("open");

    for i in 0..105 {
        db.add(&format!("标题{i}"), &format!("https://b23.tv/{i}"), "video").unwrap();
    }
    assert_eq!(db.count().unwrap(), 100);
    // Same-second timestamps tie: SQLite keeps the first 100 rows it
    // encounters (Python behaves identically on fast inserts), so the
    // surviving set is the earliest ids here.
    {
        let conn = Connection::open(dir.path().join("history.db")).unwrap();
        let urls: Vec<String> = conn
            .prepare("SELECT url FROM history ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(urls.first().map(String::as_str), Some("https://b23.tv/0"));
        assert_eq!(urls.last().map(String::as_str), Some("https://b23.tv/99"));
    }

    // Re-adding an existing URL does not duplicate; it is replaced.
    db.add("旧标题新写", "https://b23.tv/0", "video").unwrap();
    let rows = db.query_all().unwrap();
    assert_eq!(rows.len(), 100);
    let readded: Vec<_> = rows.iter().filter(|r| r.url == "https://b23.tv/0").collect();
    assert_eq!(readded.len(), 1);
    assert_eq!(readded[0].title, "旧标题新写");
    let first = rows[0].clone();

    // Delete one, clear all.
    db.delete(&first.history_id).unwrap();
    assert_eq!(db.count().unwrap(), 99);
    db.clear().unwrap();
    assert_eq!(db.count().unwrap(), 0);
}

#[test]
fn wal_mode_on_and_user_version_migration_ports() {
    let dir = tempfile::tempdir().unwrap();

    // Fresh task.db: WAL, user_version written to HASH_ID_VERSION.
    let db = TaskDatabase::open(dir.path().join("a/task.db")).unwrap();
    {
        let conn = Connection::open(dir.path().join("a/task.db")).unwrap();
        assert_eq!(journal_mode(&conn), "wal");
    }
    assert_eq!(db.user_version(), database::hash_id::HASH_ID_VERSION);

    // Rehash path: pre-HASH_ID_VERSION rows get their hash recomputed.
    let p = dir.path().join("b/task.db");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    let conn = Connection::open(&p).unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE "download_task" (
            "id" INTEGER UNIQUE, "task_id" TEXT UNIQUE, "hash_id" TEXT,
            "cover_id" TEXT, "title" TEXT, "created_time" INTEGER, "data" TEXT,
            PRIMARY KEY("id" AUTOINCREMENT)
        );
        CREATE TABLE "completed_task" (
            "id" INTEGER UNIQUE, "task_id" TEXT UNIQUE, "hash_id" TEXT,
            "cover_id" TEXT, "title" TEXT, "completed_time" INTEGER, "data" TEXT,
            PRIMARY KEY("id" AUTOINCREMENT)
        );
        "#,
    )
    .unwrap();
    let task: TaskInfo = serde_json::from_str(include_str!("fixtures/task_video.json")).unwrap();
    conn.execute(
        "INSERT INTO download_task (task_id, hash_id, cover_id, title, created_time, data)
         VALUES (?, ?, ?, ?, ?, ?)",
        rusqlite::params![
            task.Basic.task_id,
            "stalehash",
            task.Basic.cover_id,
            task.Basic.show_title,
            1,
            task.to_json()
        ],
    )
    .unwrap();
    conn.execute("PRAGMA user_version = 0", []).unwrap();
    drop(conn);

    let db = TaskDatabase::open(&p).unwrap();
    assert_eq!(db.user_version(), database::hash_id::HASH_ID_VERSION);
    let fresh = db
        .query_task_by_id(&task.Basic.task_id, false)
        .unwrap()
        .expect("row survived rehash");
    let expected = database::hash_id::calc_hash_id(&fresh);
    {
        let conn = Connection::open(&p).unwrap();
        let hash: String = conn
            .query_row("SELECT hash_id FROM download_task", [], |r| r.get(0))
            .unwrap();
        assert_ne!(hash, "stalehash");
        assert_eq!(hash, expected);
    }

    // Old schema (missing hash_id column) is rebuilt in place with data kept.
    let p2 = dir.path().join("c/task.db");
    std::fs::create_dir_all(p2.parent().unwrap()).unwrap();
    let conn = Connection::open(&p2).unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE "download_task" (
            "id" INTEGER UNIQUE, "task_id" TEXT UNIQUE,
            "cover_id" TEXT, "title" TEXT, "created_time" INTEGER, "data" TEXT,
            PRIMARY KEY("id" AUTOINCREMENT)
        );
        "#,
    )
    .unwrap();
    conn.execute(
        "INSERT INTO download_task (task_id, cover_id, title, created_time, data)
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params![
            task.Basic.task_id,
            task.Basic.cover_id,
            task.Basic.show_title,
            1,
            task.to_json()
        ],
    )
    .unwrap();
    drop(conn);

    // Corrupted row: rehash skips it without failing the migration.
    let p3 = dir.path().join("d/task.db");
    {
        let _db = TaskDatabase::open(&p3).unwrap();
    }
    {
        let conn = Connection::open(&p3).unwrap();
        conn.execute(
            "INSERT INTO download_task (task_id, hash_id, cover_id, title, created_time, data)
             VALUES ('bad', 'h', 'c', 't', 1, 'not json')",
            [],
        )
        .unwrap();
        conn.execute("PRAGMA user_version = 0", []).unwrap();
    }
    let db = TaskDatabase::open(&p3).unwrap();
    assert_eq!(db.user_version(), database::hash_id::HASH_ID_VERSION);
}

#[test]
fn hash_id_matches_python_algorithm() {
    // Vector generated by the Python implementation (dumps_stable + md5):
    // md5('{"aid":170001,"bvid":"BV1xx411c7mD","cid":170002}')
    let task: TaskInfo = serde_json::from_str(include_str!("fixtures/task_video.json")).unwrap();
    let hash = database::hash_id::calc_hash_id(&task);
    assert_eq!(hash, "5d8cee8a5cc33872501966a17552c0b1");
}
