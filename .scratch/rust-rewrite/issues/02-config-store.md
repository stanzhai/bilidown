# 02: Config store + config_version migrations

**What to build:** A Rust config crate that reads/writes the Python
build's `config.json` in the same User Data Directory, atomically, running
the same `config_version` migration chain the Python code runs, with
defaults equal to the Python defaults. First Rust run silently upgrades an
old Python config in place.

**Blocked by:** 01.

**Status:** resolved

- [x] Reads real Python config.json (golden sample fixtures from test_config_defaults/test_config_migration expectations)
- [x] Every Python migration step ported; old versions migrate to current
- [x] Atomic write (temp file + rename) round-trips identical shape Python writes
- [x] Golden-sample tests: old real configs parse; write round-trip stable

## Answer

`crates/config`：嵌入由 Python 真实保存路径生成的 defaults.json 作默认值与键序，加载语义对齐 qfluentwidgets（未知键忽略、校验器纠偏、枚举反序列化失败中止 load），迁移链逐步等价 patch_config（2.13.0 proxy→manual、2.14.0 补画质 122、2.20.0 命名规则重置+去 Akamai、版本写 2200），保存为临时文件+rename 的原子写且字节与 Python 一致（金样比对）。commit fe582d8b（merge 后 tip 8b42cfcb）。

## Comments
