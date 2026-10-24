# 47: Per-row open directory in the downloads list

**What to fix:** The downloads toolbar's "Open Download Directory" button
opened the global config `Download/download_path` — wrong whenever a task
was created with its own save path. Remove it; open each task's own
directory from its row instead (paths can differ per task).

**Status:** resolved

## Answer
Deleted the toolbar button and `DownloadsScreen::open_download_directory`
(config-path based). Each row now has an "Open Directory" button wired to
`DownloadsScreen::row_open_directory`, which opens that task's own target
`File.download_path/File.folder` (searches both tabs so it works from
Downloading and Completed). i18n: `DownloadListView`/`Open Directory`
(打开目录/開啟目錄) added; catalog total pinned at 1053 in
`services/tests/i18n.rs`. Toolbar otherwise unchanged (Sort / Logs /
Delete All).
