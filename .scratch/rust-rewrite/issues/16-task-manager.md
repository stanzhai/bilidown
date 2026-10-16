# 16: Download Task manager + duplicate detection

**What to build:** The Download Task lifecycle: create tasks from Episode
+ options (Task Info frozen at creation), schedule across 1-10 parallel
tasks, persist active/completed transitions to the task DB, Hash Id
computation, duplicate detection across both tables with ask/skip/
continue Duplicate Resolution.

**Blocked by:** 03, 05, 13, 15.

**Status:** resolved

- [x] Task create → run → complete moves row between tables; Task Info sections identical on read-back (test_task_completion_durability semantics)
- [x] Hash Id stable across both tables; duplicate detection offers ask/skip/continue
- [x] Parallel task slots enforced; pause/resume/cancel reflected in persisted state
- [x] Crash-safe: interrupted mid-download task resumes from its offset table on restart

## Answer

Implemented `crates/tasks` (manager/writer-thread durability, scheduler 1-10 slots, Task Info freeze, Hash Id duplicates ask/skip/continue, offset-store crash-safe resume); `database::query_task_json` added for offset seeding. 26 tests green. Commit 100424c4, merged integration b2fdd000.

## Comments
