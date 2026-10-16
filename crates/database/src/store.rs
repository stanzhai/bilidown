//! The three SQLite stores, ported from `src/util/common/database.py` and
//! its subclasses: task.db (download_task/completed_task), thumbnail.db
//! (blob cache with size cap), history.db (capped at 100 rows).
//!
//! One long-lived connection per store with `synchronous = NORMAL` and a
//! 30s busy timeout, mirroring the Python base class; WAL is set at table
//! creation and re-checked per connection. (ticket 03)

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::hash_id::{calc_hash_id, HASH_ID_VERSION};
use crate::info::TaskInfo;

const BUSY_TIMEOUT_MS: u32 = 30_000;

fn open_connection(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    // Switching journal_mode needs a brief exclusive lock; skip if already WAL.
    let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        conn.pragma_update(None, "journal_mode", "WAL")?;
    }
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS as u64))?;
    Ok(conn)
}

fn user_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap_or(0)
}

fn set_user_version(conn: &Connection, version: i64) -> rusqlite::Result<()> {
    conn.pragma_update(None, "user_version", version)
}

/// A raw task row as written to either task table. Callers may assemble
/// records up front (on their own thread) and hand them to the write path.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskRecord {
    pub task_id: String,
    pub hash_id: String,
    pub cover_id: String,
    pub title: String,
    pub timestamp: i64,
    pub data: String,
}

/// `task.db` store.
pub struct TaskDatabase {
    conn: Connection,
    pub path: PathBuf,
}

impl TaskDatabase {
    /// Opens (creating if needed) `task.db`, ensures tables/indexes, then
    /// runs the upgrade/rehash migration chain.
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = open_connection(&path)?;
        let mut db = Self { conn, path };
        db.check_and_create_table()?;
        db.check_should_upgrade()?;
        Ok(db)
    }

    pub fn check_and_create_table(&mut self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS "download_task" (
                "id"	INTEGER UNIQUE,
                "task_id"	TEXT UNIQUE,
                "hash_id"   TEXT,
                "cover_id"	TEXT,
                "title"	TEXT,
                "created_time"	INTEGER,
                "data"	TEXT,
                PRIMARY KEY("id" AUTOINCREMENT)
            );
            CREATE TABLE IF NOT EXISTS "completed_task" (
                "id"	INTEGER UNIQUE,
                "task_id"	TEXT UNIQUE,
                "hash_id"   TEXT,
                "cover_id"	TEXT,
                "title"	TEXT,
                "completed_time"	INTEGER,
                "data"	TEXT,
                PRIMARY KEY("id" AUTOINCREMENT)
            );
            "#,
        )?;

        // Old tables may lack hash_id; only create the index once present.
        if self.has_column("download_task", "hash_id")
            && self.has_column("completed_task", "hash_id")
        {
            self.conn.execute_batch(
                r#"
                CREATE INDEX IF NOT EXISTS "idx_download_task_hash_id" ON "download_task" ("hash_id");
                CREATE INDEX IF NOT EXISTS "idx_completed_task_hash_id" ON "completed_task" ("hash_id");
                "#,
            )?;
        }

        self.conn.execute_batch(
            r#"
            CREATE INDEX IF NOT EXISTS "idx_download_task_created_time" ON "download_task" ("created_time");
            CREATE INDEX IF NOT EXISTS "idx_completed_task_completed_time" ON "completed_task" ("completed_time");
            "#,
        )
    }

    fn has_column(&self, table: &str, column: &str) -> bool {
        let sql = format!("PRAGMA table_info({table});");
        let mut stmt = match self.conn.prepare(&sql) {
            Ok(stmt) => stmt,
            Err(_) => return false,
        };
        let mut rows = stmt.query([]).expect("pragma query is infallible");
        while let Ok(Some(row)) = rows.next() {
            let name: String = row.get(1).expect("table_info column 1 is TEXT");
            if name == column {
                return true;
            }
        }
        false
    }

    fn needs_upgrade(&self) -> bool {
        let required = ["task_id", "hash_id", "cover_id", "title", "data"];
        for table in ["download_task", "completed_task"] {
            for column in required {
                if !self.has_column(table, column) {
                    return true;
                }
            }
        }
        false
    }

    fn check_should_upgrade(&mut self) -> anyhow::Result<()> {
        // Config version and task-db version are independent; always check
        // the actual table shape so a failed migration is retried.
        if self.needs_upgrade() {
            self.upgrade()?;
            set_user_version(&self.conn, HASH_ID_VERSION)?;
            return Ok(());
        }
        self.check_should_rehash()
    }

    fn check_should_rehash(&mut self) -> anyhow::Result<()> {
        let current = user_version(&self.conn);
        if current >= HASH_ID_VERSION {
            return Ok(());
        }
        // On failure the version is left alone; the next startup retries.
        self.rehash_all()?;
        set_user_version(&self.conn, HASH_ID_VERSION)?;
        Ok(())
    }

    fn rehash_all(&mut self) -> anyhow::Result<()> {
        // The data column holds the full task_info; hash ids can be
        // recomputed without re-downloading anything.
        for table in ["download_task", "completed_task"] {
            let rows: Vec<(String, String)> = {
                let mut stmt = self
                    .conn
                    .prepare(&format!("SELECT task_id, data FROM {table}"))?;
                let rows = stmt
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows
            };

            let mut updates = Vec::new();
            for (task_id, data) in rows {
                // Skip corrupted rows; the rest still get rehashed.
                let Ok(task_info) = TaskInfo::from_json(&data) else {
                    continue;
                };
                updates.push((calc_hash_id(&task_info), task_id));
            }

            if !updates.is_empty() {
                let mut stmt = self.conn.prepare(&format!(
                    "UPDATE {table} SET hash_id = ? WHERE task_id = ?"
                ))?;
                for (hash_id, task_id) in &updates {
                    stmt.execute(params![hash_id, task_id])?;
                }
            }
        }
        Ok(())
    }

    fn upgrade(&mut self) -> anyhow::Result<()> {
        if !self.needs_upgrade() {
            return Ok(());
        }

        let download_records = self.collect_records("download_task", false)?;
        let completed_records = self.collect_records("completed_task", true)?;

        // Rebuild both tables in one transaction so a mid-migration failure
        // cannot leave empty or half-built tables behind.
        let conn = &mut self.conn;
        let tx = conn.transaction()?;
        tx.execute_batch(
            "DROP TABLE IF EXISTS download_task;
             DROP TABLE IF EXISTS completed_task;",
        )?;
        tx.execute_batch(
            r#"CREATE TABLE download_task (
                id INTEGER UNIQUE,
                task_id TEXT UNIQUE,
                hash_id TEXT,
                cover_id TEXT,
                title TEXT,
                created_time INTEGER,
                data TEXT,
                PRIMARY KEY(id AUTOINCREMENT)
            )"#,
        )?;
        tx.execute_batch(
            r#"CREATE TABLE completed_task (
                id INTEGER UNIQUE,
                task_id TEXT UNIQUE,
                hash_id TEXT,
                cover_id TEXT,
                title TEXT,
                completed_time INTEGER,
                data TEXT,
                PRIMARY KEY(id AUTOINCREMENT)
            )"#,
        )?;
        tx.execute_batch(
            "CREATE INDEX idx_download_task_hash_id ON download_task (hash_id);
             CREATE INDEX idx_completed_task_hash_id ON completed_task (hash_id);",
        )?;
        insert_records(&tx, false, &download_records)?;
        insert_records(&tx, true, &completed_records)?;
        Ok(tx.commit()?)
    }

    fn collect_records(&self, table: &str, completed: bool) -> anyhow::Result<Vec<TaskRecord>> {
        let mut stmt = self.conn.prepare(&format!("SELECT data FROM \"{table}\""))?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut records = Vec::new();
        for data in rows {
            let task_info = TaskInfo::from_json(&data)?;
            records.push(Self::build_record(&task_info, completed));
        }
        Ok(records)
    }

    /// Reads task rows. `limit = None` returns everything (startup list
    /// restore); otherwise the most recent `limit` rows by time desc.
    pub fn query_tasks(&self, completed: bool, limit: Option<i64>) -> rusqlite::Result<Vec<TaskInfo>> {
        let (table, order_column) = if completed {
            ("completed_task", "completed_time")
        } else {
            ("download_task", "created_time")
        };
        let sql = match limit {
            None => format!(r#"SELECT data FROM "{table}""#),
            Some(_) => format!(
                r#"SELECT data FROM "{table}" ORDER BY "{order_column}" DESC LIMIT ?"#,
            ),
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = match limit {
            None => stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?,
            Some(limit) => stmt
                .query_map([limit], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?,
        };

        // Corrupted rows are skipped, matching the Python read tolerance.
        Ok(rows
            .iter()
            .filter_map(|data| TaskInfo::from_json(data).ok())
            .collect())
    }

    /// Reads one row by `task_id` (UNIQUE column; no full scan).
    pub fn query_task_by_id(&self, task_id: &str, completed: bool) -> rusqlite::Result<Option<TaskInfo>> {
        let table = if completed { "completed_task" } else { "download_task" };
        let mut stmt = self
            .conn
            .prepare(&format!(r#"SELECT data FROM "{table}" WHERE "task_id" = ?"#))?;
        let mut rows = stmt.query(params![task_id])?;
        match rows.next()? {
            Some(row) => {
                let data: String = row.get(0)?;
                Ok(TaskInfo::from_json(&data).ok())
            }
            None => Ok(None),
        }
    }

    pub fn count_tasks(&self, completed: bool) -> rusqlite::Result<i64> {
        let table = if completed { "completed_task" } else { "download_task" };
        self.conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
    }

    /// Assembles a record from a task, recomputing the hash id.
    pub fn build_record(task_info: &TaskInfo, completed: bool) -> TaskRecord {
        let timestamp = if completed {
            task_info.Basic.completed_time
        } else {
            task_info.Basic.created_time
        };
        TaskRecord {
            task_id: task_info.Basic.task_id.clone(),
            hash_id: calc_hash_id(task_info),
            cover_id: task_info.Basic.cover_id.clone(),
            title: task_info.Basic.show_title.clone(),
            timestamp: if timestamp != 0 {
                timestamp
            } else {
                now_timestamp()
            },
            data: task_info.to_json(),
        }
    }

    /// Inserts tasks into either table (batch of pre-built records).
    pub fn add_task_records(&mut self, records: &[TaskRecord], completed: bool) -> rusqlite::Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let table = if completed { "completed_task" } else { "download_task" };
        let mut stmt = self.conn.prepare(&format!(
            "INSERT INTO {table} (task_id, hash_id, cover_id, title, created_time, data)
             VALUES (?, ?, ?, ?, ?, ?)"
        ))?;
        for record in records {
            stmt.execute(params![
                record.task_id,
                record.hash_id,
                record.cover_id,
                record.title,
                record.timestamp,
                record.data
            ])?;
        }
        Ok(())
    }

    /// Convenience: build + insert whole `TaskInfo`s.
    pub fn add_tasks(&mut self, tasks: &[TaskInfo], completed: bool) -> rusqlite::Result<()> {
        let records = tasks
            .iter()
            .map(|t| Self::build_record(t, completed))
            .collect::<Vec<_>>();
        self.add_task_records(&records, completed)
    }

    pub fn update_task(&self, task_info: &TaskInfo) -> rusqlite::Result<usize> {
        self.conn.execute(
            "UPDATE download_task SET data = ? WHERE task_id = ?",
            params![task_info.to_json(), task_info.Basic.task_id],
        )
    }

    pub fn update_task_json(&self, task_id: &str, data: &str) -> rusqlite::Result<usize> {
        self.conn.execute(
            "UPDATE download_task SET data = ? WHERE task_id = ?",
            params![data, task_id],
        )
    }

    pub fn delete_tasks(&self, task_ids: &[String], completed: bool) -> rusqlite::Result<()> {
        if task_ids.is_empty() {
            return Ok(());
        }
        let table = if completed { "completed_task" } else { "download_task" };
        // SQLITE_MAX_VARIABLE_NUMBER defaults to 999; batch to stay safe.
        for batch in task_ids.chunks(500) {
            let placeholders = vec!["?"; batch.len()].join(", ");
            self.conn.execute(
                &format!("DELETE FROM {table} WHERE task_id IN ({placeholders})"),
                rusqlite::params_from_iter(batch.iter()),
            )?;
        }
        Ok(())
    }

    /// Moves a record from download_task to completed_task in one
    /// transaction so a mid-flight failure cannot lose the task.
    pub fn move_to_completed(&mut self, record: &TaskRecord) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM download_task WHERE task_id = ?", params![record.task_id])?;
        insert_records(&tx, true, std::slice::from_ref(record))?;
        tx.commit()
    }

    /// Moves a record from completed_task back to download_task.
    pub fn recreate_task(&mut self, record: &TaskRecord) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM completed_task WHERE task_id = ?", params![record.task_id])?;
        insert_records(&tx, false, std::slice::from_ref(record))?;
        tx.commit()
    }

    /// Duplicate detection across both tables in one round-trip.
    pub fn check_duplicate(&self, hash_id: &str) -> rusqlite::Result<bool> {
        self.conn.query_row(
            "SELECT
                EXISTS(SELECT 1 FROM completed_task WHERE hash_id = ?)
                OR EXISTS(SELECT 1 FROM download_task WHERE hash_id = ?)",
            params![hash_id, hash_id],
            |r| r.get::<_, i64>(0),
        )
        .map(|v| v != 0)
        .or(Ok(false))
    }

    pub fn user_version(&self) -> i64 {
        user_version(&self.conn)
    }
}

fn insert_records(
    conn: &Connection,
    completed: bool,
    records: &[TaskRecord],
) -> rusqlite::Result<()> {
    let sql = if completed {
        "INSERT OR REPLACE INTO completed_task (task_id, hash_id, cover_id, title, completed_time, data)
         VALUES (?, ?, ?, ?, ?, ?)"
    } else {
        "INSERT OR REPLACE INTO download_task (task_id, hash_id, cover_id, title, created_time, data)
         VALUES (?, ?, ?, ?, ?, ?)"
    };
    let mut stmt = conn.prepare_cached(sql)?;
    for record in records {
        stmt.execute(params![
            record.task_id,
            record.hash_id,
            record.cover_id,
            record.title,
            record.timestamp,
            record.data
        ])?;
    }
    Ok(())
}

/// `thumbnail.db` store: blob cache keyed by a composite cover key.
pub struct CoverDatabase {
    conn: Connection,
    pub path: PathBuf,
}

/// Capacity cap in bytes. Exceeding it clears the whole cache (covers are
/// regenerable); aligned with the Python threshold.
const COVER_DB_SIZE_THRESHOLD: u64 = 150 * 1024 * 1024;

impl CoverDatabase {
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = open_connection(&path)?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS "thumbnail" (
                "id"	INTEGER UNIQUE,
                "cover_id"	TEXT UNIQUE,
                "created_time"	INTEGER,
                "cover"	BLOB,
                PRIMARY KEY("id" AUTOINCREMENT)
            );
            "#,
        )?;
        let db = Self { conn, path };
        db.check_database_size()?;
        Ok(db)
    }

    /// Over threshold: clear and VACUUM (DELETE alone does not shrink the
    /// file, which would re-trigger the cleanup every startup).
    fn check_database_size(&self) -> anyhow::Result<()> {
        let Ok(meta) = std::fs::metadata(&self.path) else {
            return Ok(());
        };
        if meta.len() > COVER_DB_SIZE_THRESHOLD {
            self.conn.execute("DELETE FROM thumbnail", [])?;
            self.conn.execute("VACUUM", [])?;
        }
        Ok(())
    }

    pub fn query_cover(&self, cover_key: &str) -> rusqlite::Result<Option<Vec<u8>>> {
        self.conn
            .query_row(
                "SELECT cover FROM thumbnail WHERE cover_id = ?",
                params![cover_key],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
    }

    /// INSERT OR REPLACE (not a bare INSERT): a stored falsy value would
    /// otherwise force a re-download every query and collide on UNIQUE
    /// forever; REPLACE lets every successful download self-heal the row.
    pub fn add_cover(&self, cover_key: &str, cover_data: &[u8]) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO thumbnail (cover_id, created_time, cover) VALUES (?, ?, ?)",
            params![cover_key, now_timestamp(), cover_data],
        )?;
        Ok(())
    }
}

/// One history row (parse history entry).
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub history_id: String,
    pub title: String,
    pub url: String,
    pub type_: String,
    pub created_time: i64,
}

/// `history.db` store: parse history capped at the most recent 100 rows.
pub struct HistoryDatabase {
    conn: Connection,
    pub path: PathBuf,
    max_length: i64,
}

impl HistoryDatabase {
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = open_connection(&path)?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS "history" (
                "id"	INTEGER UNIQUE,
                "history_id"	TEXT UNIQUE,
                "title" TEXT,
                "url"	TEXT,
                "type"	TEXT,
                "created_time"	INTEGER,
                PRIMARY KEY("id" AUTOINCREMENT)
            );
            "#,
        )?;
        Ok(Self { conn, path, max_length: 100 })
    }

    /// All rows, newest first.
    pub fn query_all(&self) -> rusqlite::Result<Vec<HistoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT history_id, title, url, type, created_time FROM history ORDER BY created_time DESC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(HistoryEntry {
                    history_id: r.get(0)?,
                    title: r.get(1)?,
                    url: r.get(2)?,
                    type_: r.get(3)?,
                    created_time: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Adds an entry, de-duplicating by URL and trimming to the newest
    /// `max_length` rows, all in one transaction.
    pub fn add(&self, title: &str, url: &str, type_: &str) -> rusqlite::Result<()> {
        let history_id = uuid::Uuid::new_v4().to_string();
        self.conn.execute_batch("BEGIN")?;
        let result = (|| {
            self.conn
                .execute("DELETE FROM history WHERE url = ?", params![url])?;
            self.conn.execute(
                "INSERT INTO history (history_id, title, url, type, created_time) VALUES (?, ?, ?, ?, ?)",
                params![history_id, title, url, type_, now_timestamp()],
            )?;
            self.conn.execute(
                "DELETE FROM history WHERE id NOT IN (SELECT id FROM history ORDER BY created_time DESC LIMIT ?)",
                params![self.max_length],
            )?;
            Ok(())
        })();
        match result {
            Ok(()) => self.conn.execute_batch("COMMIT").map(|_| ()),
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    pub fn delete(&self, history_id: &str) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM history WHERE history_id = ?", params![history_id])?;
        Ok(())
    }

    pub fn clear(&self) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM history", [])?;
        Ok(())
    }

    /// Row count (tests and callers that just need "how full is history").
    pub fn count(&self) -> rusqlite::Result<i64> {
        self.conn.query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
    }
}

/// Current unix seconds, matching `get_timestamp()`.
pub fn now_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
