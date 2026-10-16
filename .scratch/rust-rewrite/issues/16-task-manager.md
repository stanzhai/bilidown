# 16: Download Task manager + duplicate detection

**What to build:** The Download Task lifecycle: create tasks from Episode
+ options (Task Info frozen at creation), schedule across 1-10 parallel
tasks, persist active/completed transitions to the task DB, Hash Id
computation, duplicate detection across both tables with ask/skip/
continue Duplicate Resolution.

**Blocked by:** 03, 05, 13, 15.

**Status:** ready-for-agent

- [ ] Task create → run → complete moves row between tables; Task Info sections identical on read-back (test_task_completion_durability semantics)
- [ ] Hash Id stable across both tables; duplicate detection offers ask/skip/continue
- [ ] Parallel task slots enforced; pause/resume/cancel reflected in persisted state
- [ ] Crash-safe: interrupted mid-download task resumes from its offset table on restart

## Comments
