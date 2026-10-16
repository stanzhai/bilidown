# 03: Three SQLite databases with drop-in schemas

**What to build:** Task/thumbnail/history SQLite persistence in Rust with
identical schemas, table/column names, PRAGMA user_version rehash
migration semantics and WAL mode as the Python build; reads the Python
databases in place. Task rows serialize Task Info JSON in the same shape.

**Blocked by:** 01.

**Status:** resolved

- [x] Golden-sample tests pin read of real Python task.db / thumbnail.db / history.db fixtures
- [x] download_task + completed_task round-trip: insert a Task Info, read back identical sections
- [x] Thumbnail blob cache with size cap; history capped at 100
- [x] WAL on; PRAGMA user_version migration path ported; write round-trip stable

## Comments

## Answer

Implemented `crates/database`: `TaskInfo` JSON model with Python-identical
section shapes and byte-stable serialization, `hash_id` algorithm pinned
against a Python-generated MD5 vector, and `TaskDatabase` /
`CoverDatabase` / `HistoryDatabase` with the exact Python schemas, WAL,
`PRAGMA user_version` upgrade+rehash migrations, thumbnail 150MiB cap, and
history capped at 100. Golden fixtures under `tests/fixtures/` pin read
compatibility; 8 tests green. Commit: f09d63da
