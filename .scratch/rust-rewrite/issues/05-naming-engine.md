# 05: Naming engine (rule compiler, 12 conventions, numbering modes)

**What to build:** The Naming Rule engine in Rust: template compiler with
variables, `<>` optional segments, date/number formats; all 12 built-in
Convention Types; numbering modes (continuous / specified /
use-parse-list). Produces identical filenames to the Python engine for the
same Task Info + rule inputs.

**Blocked by:** 01.

**Status:** ready-for-agent

- [ ] Rule template compiler ports test_rule_template vectors verbatim
- [ ] Rule model ports test_rule_model vectors
- [ ] All 12 conventions match test_naming_convention expectations
- [ ] Task naming snapshot tests (test_task_naming_snapshot, test_file_name) pass with identical expected filenames
- [ ] Numbering modes produce the Python build's numbering for each mode

## Comments
