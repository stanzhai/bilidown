# 48: Windows 打开下载目录落到"文档"

**What to fix:** Windows 上下载列表行内"打开目录"打开的是文档（Documents），
不是任务实际下载路径。怀疑平台路径处理问题，统一修复。

**Status:** resolved

## Answer
根因不是路径拼接（`download_path/folder` 一直正确），而是 Windows
explorer 的兜底行为：给它一个**解析不了的路径**（相对路径或尚不存在），
explorer 不会报错，而是**静默打开"文档"**。下载任务的
`download_path/folder` 只在首次写文件时才创建（download_pipeline
`create_dir_all`），所以排队中/刚失败的任务点"打开目录"必然命中这个坑。
另外旧实现 `to_string_lossy()` 会把非 UTF-8 的中文路径弄脏。

统一修复（两处重复实现收敛为一处 `ui/downloads/mod.rs::open_directory`）：

- `std::path::absolute()` 转绝对路径——不用 `canonicalize()`，它会加
  `\\?\` UNC 前缀，explorer 打不开；`absolute()` 自 1.79 稳定，不加前缀。
- `nearest_existing()`：目录不存在时向上走到最近存在的祖先（如
  download_path 本身），绝不把不存在的路径交给 explorer。
- 参数保持 `&Path`（OsStr 语义），中文路径不再有 lossy 转码。
- macOS/Linux 同走该函数：`open`/`xdg-open` 同样需要绝对路径，行为统一。

调用点：`screen.rs` 两处（row_action 的 Open 分支、row_open_directory）
与 `log_view.rs::open_logs_directory`（打开日志目录）全部改走共享实现。

测试：`nearest_existing_walks_up_to_real_dir`（深层不存在尾解析到存在
祖先 + 存在目录返回自身）。workspace 全测试 + clippy `-D warnings` 通过。
