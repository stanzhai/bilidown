# 02: Config store + config_version migrations

**What to build:** A Rust config crate that reads/writes the Python
build's `config.json` in the same User Data Directory, atomically, running
the same `config_version` migration chain the Python code runs, with
defaults equal to the Python defaults. First Rust run silently upgrades an
old Python config in place.

**Blocked by:** 01.

**Status:** in-progress

- [ ] Reads real Python config.json (golden sample fixtures from test_config_defaults/test_config_migration expectations)
- [ ] Every Python migration step ported; old versions migrate to current
- [ ] Atomic write (temp file + rename) round-trips identical shape Python writes
- [ ] Golden-sample tests: old real configs parse; write round-trip stable

## Comments
