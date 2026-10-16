# 03: Three SQLite databases with drop-in schemas

**What to build:** Task/thumbnail/history SQLite persistence in Rust with
identical schemas, table/column names, PRAGMA user_version rehash
migration semantics and WAL mode as the Python build; reads the Python
databases in place. Task rows serialize Task Info JSON in the same shape.

**Blocked by:** 01.

**Status:** ready-for-agent

- [ ] Golden-sample tests pin read of real Python task.db / thumbnail.db / history.db fixtures
- [ ] download_task + completed_task round-trip: insert a Task Info, read back identical sections
- [ ] Thumbnail blob cache with size cap; history capped at 100
- [ ] WAL on; PRAGMA user_version migration path ported; write round-trip stable

## Comments
