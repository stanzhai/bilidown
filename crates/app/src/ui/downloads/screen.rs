//! Downloads screen entity (port of `src/gui/interface/download.py`):
//! Downloading/Completed pivot tabs, sort + bulk-action bar, task rows
//! (cover / title / info / size / progress / status / action), empty
//! states, and the QueryWorker-equivalent background refresh.

use gpui_kit::base::Disableable;
use gpui_kit::component::button::Button;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::{div, prelude::*, px, IntoElement, SharedString};
use std::sync::Arc;

use super::dialogs::{error_message, open_duplicate_dialog, open_log_viewer, tip_message};
use super::model::{
    batch_cancel, batch_pause, batch_start, cancel_download, info_text, redownload, row_action,
    size_text, status_text, toggle_pause_resume, RowAction, SortKey, TaskListState,
};
use super::screen_state::{DownloadsTab, ScreenState};
use crate::ui::Shell;
use config::ConfigStore;
use database::TaskInfo;
use tasks::status::download_status as st;
use tasks::TaskManager;

pub struct DownloadsScreen {
    pub model: ScreenState,
    pub manager: Option<Arc<TaskManager>>,
    pub config: Arc<ConfigStore>,
}

impl DownloadsScreen {
    pub fn new(
        config: Arc<ConfigStore>,
        manager: Option<Arc<TaskManager>>,
        _window: &mut gpui_kit::Window,
        _cx: &mut gpui_kit::Context<Shell>,
    ) -> Self {
        let mut model = ScreenState::new();
        model.load_sort_prefs(&config);
        Self {
            model,
            manager,
            config,
        }
    }

    /// QueryWorker equivalent: query both tables off-thread, then swap the
    /// rows in one notification.
    pub fn refresh(&mut self, cx: &mut gpui_kit::Context<Shell>) {
        let Some(manager) = self.manager.clone() else {
            return;
        };
        cx.spawn(async move |shell, cx| {
            let downloading = manager.query(false, None);
            let failed = downloading.is_err();
            let downloading = downloading.unwrap_or_default();
            let completed = manager.query(true, None).unwrap_or_default();
            shell
                .update(cx, |shell, cx| {
                    let screen = &mut shell.downloads;
                    screen.model.downloading.tasks = downloading;
                    screen.model.completed.tasks = completed;
                    screen.model.query_failed = failed;
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// Poll-refresh used while the downloads page is on screen: skip the
    /// query when the active list has nothing in flight (idle cost ~0).
    pub fn refresh_if_active(&mut self, cx: &mut gpui_kit::Context<Shell>) {
        let busy = self.model.downloading.tasks.iter().any(|t| {
            matches!(
                t.Download.status,
                st::QUEUED
                    | st::DOWNLOADING
                    | st::PARSING
                    | st::MERGING
                    | st::CONVERTING
                    | st::FFMPEG_QUEUED
                    | st::ADDITIONAL_PROCESSING
            )
        });
        if busy {
            self.refresh(cx);
        }
    }

    /// Live in-frame tick (the 500ms poll's fast lane): fold the running
    /// engines' snapshots straight into the rows — progress and the pause
    /// the DB write may still be coalescing. Speed is NOT folded: the
    /// engine never computes it (its snapshot speed stays 0), so folding
    /// it here blanked the row between polls — the speed-text flicker
    /// (issue 42). The pipeline sampler owns speed via the DB lane.
    /// Merge stages progress through the writer thread, which the poll
    /// picks up.
    pub fn fold_engine_snapshots(&mut self, manager: &tasks::TaskManager) {
        let mut changed = false;
        for task in self.model.downloading.tasks.iter_mut() {
            if task.Download.status != st::DOWNLOADING {
                continue;
            }
            let Some(state) = manager.scheduler.snapshot(&task.Basic.task_id) else {
                continue;
            };
            if state.status != downloader::TaskStatus::Downloading
                && state.status != downloader::TaskStatus::Paused
            {
                continue; // terminal states land via the DB poll
            }
            let downloaded = state.downloaded_size as i64;
            let progress = if state.status == downloader::TaskStatus::Paused {
                task.Download.progress
            } else {
                state.progress() as i64
            };
            let paused = state.status == downloader::TaskStatus::Paused;
            if task.Download.downloaded_size != downloaded
                || task.Download.progress != progress
                || (paused && task.Download.status != st::PAUSED)
            {
                task.Download.downloaded_size = downloaded;
                task.Download.progress = progress;
                if paused {
                    task.Download.status = st::PAUSED;
                }
                changed = true;
            }
        }
        let _ = changed; // caller notifies from the poll's cadence
    }

    /// Persist a sort change (Python saved on every flyout change).
    pub fn set_sort(&mut self, tab: DownloadsTab, key: SortKey, ascending: bool) {
        let (item, asc_item) = tab.sort_items();
        if let Ok(mut cfg) = config::ConfigStore::open() {
            cfg.set("Behavior", item, serde_json::json!(key.as_config()));
            cfg.set("Behavior", asc_item, serde_json::json!(ascending));
            let _ = cfg.save();
        }
        let list = match tab {
            DownloadsTab::Downloading => &mut self.model.downloading,
            DownloadsTab::Completed => &mut self.model.completed,
        };
        list.sort_by = key;
        list.ascending = ascending;
    }

    pub fn toggle_tab(&mut self, tab: DownloadsTab) {
        self.model.tab = tab;
    }

    pub fn batch_action(&mut self, action: &str, cx: &mut gpui_kit::Context<Shell>) {
        let Some(manager) = self.manager.clone() else {
            return;
        };
        let tab = self.model.tab;
        let list = match tab {
            DownloadsTab::Downloading => &mut self.model.downloading,
            DownloadsTab::Completed => &mut self.model.completed,
        };
        let mut tasks = std::mem::take(&mut list.tasks);
        match action {
            "start" => batch_start(&manager, &mut tasks),
            "pause" => batch_pause(&manager, &mut tasks),
            "delete" => {
                let removed = batch_cancel(&manager, &tasks);
                tasks.retain(|t| !removed.contains(&t.Basic.task_id));
            }
            _ => {}
        }
        list.tasks = tasks;
        cx.notify();
    }

    pub fn row_action(&mut self, task_id: &str, cx: &mut gpui_kit::Context<Shell>) {
        let Some(manager) = self.manager.clone() else {
            return;
        };
        let tab = self.model.tab;
        let list = match tab {
            DownloadsTab::Downloading => &mut self.model.downloading,
            DownloadsTab::Completed => &mut self.model.completed,
        };
        let Some(ix) = list.tasks.iter().position(|t| t.Basic.task_id == task_id) else {
            return;
        };
        let mut task = list.tasks[ix].clone();
        match row_action(&task) {
            RowAction::Open => {
                let dir =
                    std::path::PathBuf::from(&task.File.download_path).join(&task.File.folder);
                let _ = open_directory(&dir);
            }
            _ => {
                toggle_pause_resume(&manager, &mut task);
                list.upsert(task);
            }
        }
        cx.notify();
    }

    /// Per-row "Open Directory": each task's own target folder
    /// (`download_path/folder`), which can differ per task (44 → user
    /// request: the toolbar button opened the global config path).
    pub fn row_open_directory(&mut self, task_id: &str, cx: &mut gpui_kit::Context<Shell>) {
        let task = [
            self.model.downloading.tasks.iter(),
            self.model.completed.tasks.iter(),
        ]
        .into_iter()
        .flatten()
        .find(|t| t.Basic.task_id == task_id);
        if let Some(task) = task {
            let dir = std::path::PathBuf::from(&task.File.download_path).join(&task.File.folder);
            let _ = open_directory(&dir);
        }
        cx.notify();
    }

    pub fn row_delete(&mut self, task_id: &str, cx: &mut gpui_kit::Context<Shell>) {
        let Some(manager) = self.manager.clone() else {
            return;
        };
        let tab = self.model.tab;
        let list = match tab {
            DownloadsTab::Downloading => &mut self.model.downloading,
            DownloadsTab::Completed => &mut self.model.completed,
        };
        let Some(task) = list
            .tasks
            .iter()
            .find(|t| t.Basic.task_id == task_id)
            .cloned()
        else {
            return;
        };
        if cancel_download(&manager, &task) {
            list.remove(task_id);
        }
        cx.notify();
    }

    pub fn row_redownload(&mut self, task_id: &str, cx: &mut gpui_kit::Context<Shell>) {
        let Some(manager) = self.manager.clone() else {
            return;
        };
        let tab = self.model.tab;
        let list = match tab {
            DownloadsTab::Downloading => &mut self.model.downloading,
            DownloadsTab::Completed => &mut self.model.completed,
        };
        let Some(ix) = list.tasks.iter().position(|t| t.Basic.task_id == task_id) else {
            return;
        };
        let mut task = list.tasks[ix].clone();
        if !redownload(&manager, &mut task) {
            return; // FFmpeg-processing rows refuse, mirroring the toast guard
        }
        if task.Download.status == st::QUEUED {
            // Completed rows move back to the downloading table.
            list.remove(task_id);
            self.refresh(cx);
        } else {
            list.upsert(task);
        }
        cx.notify();
    }

    /// Duplicate-resolution entry for ALWAYS_ASK tasks: ask via the dialog
    /// and hand `(skip, remember)` to the caller.
    pub fn ask_duplicate(
        &mut self,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Shell>,
        task_title: &str,
        on_decision: impl FnOnce(bool, bool) + 'static,
    ) {
        self.model.pending_duplicate_ask = Some(task_title.to_owned());
        open_duplicate_dialog(window, cx, task_title, on_decision);
    }

    pub fn open_logs(&mut self, window: &mut gpui_kit::Window, cx: &mut gpui_kit::Context<Shell>) {
        open_log_viewer(window, cx);
    }

    /// Render the screen body inside the shell.
    pub fn render(
        &mut self,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Shell>,
    ) -> impl IntoElement {
        let tab = self.model.tab;
        let downloading = self.model.downloading.tasks.len();
        let completed = self.model.completed.tasks.len();

        div()
            .id("downloads-screen")
            .flex()
            .flex_col()
            .size_full()
            .p_4()
            .gap_3()
            .child(
                TabBar::new("downloads-tabs")
                    .pill()
                    .selected_index(match tab {
                        DownloadsTab::Downloading => 0,
                        DownloadsTab::Completed => 1,
                    })
                    .child(
                        Tab::new()
                            .label(format!(
                                "{} ({})",
                                t("DownloadInterface", "Downloading"),
                                downloading
                            ))
                            .on_click(cx.listener(|shell, _, _, cx| {
                                shell.downloads.toggle_tab(DownloadsTab::Downloading);
                                cx.notify();
                            })),
                    )
                    .child(
                        Tab::new()
                            .label(format!(
                                "{} ({})",
                                t("DownloadInterface", "Completed"),
                                completed
                            ))
                            .on_click(cx.listener(|shell, _, _, cx| {
                                shell.downloads.toggle_tab(DownloadsTab::Completed);
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_toolbar(tab, cx))
            .child(self.render_list(tab, window, cx))
    }

    fn render_toolbar(
        &mut self,
        tab: DownloadsTab,
        cx: &mut gpui_kit::Context<Shell>,
    ) -> impl IntoElement {
        let mut bar = div().flex().gap_2().items_center();
        if tab == DownloadsTab::Downloading {
            bar = bar
                .child(
                    Button::new("downloads-start-all")
                        .label(t("TopStackedWidget", "Start All"))
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.downloads.batch_action("start", cx);
                        })),
                )
                .child(
                    Button::new("downloads-pause-all")
                        .label(t("TopStackedWidget", "Pause All"))
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.downloads.batch_action("pause", cx);
                        })),
                );
        }
        let delete_caption = match tab {
            DownloadsTab::Downloading => t("TopStackedWidget", "Delete All"),
            DownloadsTab::Completed => t("TopStackedWidget", "Clear All"),
        };
        bar.child(div().flex_1())
            .child(
                Button::new("downloads-sort")
                    .label(t("TopStackedWidget", "Sort"))
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.downloads.cycle_sort(cx);
                    })),
            )
            .child(
                Button::new("downloads-logs")
                    .label(t("LogViewerDialog", "Logs"))
                    .on_click(cx.listener(|shell, _, window, cx| {
                        shell.downloads.open_logs(window, cx);
                    })),
            )
            .child(
                Button::new("downloads-delete-all")
                    .label(delete_caption)
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.downloads.batch_action("delete", cx);
                    })),
            )
    }

    /// Toggle sort direction; at the end of a direction cycle, advance to
    /// the tab's next sort key (SortFlyout behaviour condensed to a button
    /// until a flyout widget lands).
    fn cycle_sort(&mut self, cx: &mut gpui_kit::Context<Shell>) {
        let tab = self.model.tab;
        let keys = match tab {
            DownloadsTab::Downloading => SortKey::downloading_keys(),
            DownloadsTab::Completed => SortKey::completed_keys(),
        };
        let (current, ascending) = {
            let list = match tab {
                DownloadsTab::Downloading => &self.model.downloading,
                DownloadsTab::Completed => &self.model.completed,
            };
            (list.sort_by, list.ascending)
        };
        let next = if ascending {
            current
        } else {
            let ix = keys.iter().position(|k| *k == current).unwrap_or(0);
            keys[(ix + 1) % keys.len()]
        };
        self.set_sort(tab, next, !ascending);
        cx.notify();
    }

    fn render_list(
        &mut self,
        tab: DownloadsTab,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Shell>,
    ) -> gpui_kit::AnyElement {
        let list = match tab {
            DownloadsTab::Downloading => &self.model.downloading,
            DownloadsTab::Completed => &self.model.completed,
        };
        let empty_text = match tab {
            DownloadsTab::Downloading => t("DownloadInterface", "No downloads in progress"),
            DownloadsTab::Completed => t("DownloadInterface", "No completed downloads"),
        };
        // Snapshot the visible rows' render inputs first (borrow ends),
        // then mutate covers state per row in the render pass.
        type RowInputs = (String, String, String, String, String, f32, String, String);
        let row_data: Vec<RowInputs> = list
            .visible()
            .iter()
            .map(|task| {
                (
                    task.Basic.task_id.clone(),
                    task.Basic.show_title.clone(),
                    info_text(task),
                    status_text(task, &tip_message, &error_message),
                    size_text(task),
                    task.Download.progress as f32,
                    task.Episode.cover.clone(),
                    match row_action(task) {
                        RowAction::Open => t("DownloadListView", "Pause"),
                        RowAction::Start => t("DownloadListView", "Resume"),
                        RowAction::Retry => t("DownloadListView", "Re-download"),
                        RowAction::Pause => t("DownloadListView", "Pause"),
                    },
                )
            })
            .collect();
        let rows: Vec<_> = row_data
            .into_iter()
            .map(
                |(id, title, info, status, size, progress, cover_url, action_caption)| {
                    let cover_slot = self.render_cover(&cover_url, window, cx);
                    div()
                        .id(SharedString::from(format!("task-row-{id}")))
                        .flex()
                        .gap_3()
                        .items_center()
                        .py_2()
                        .child(cover_slot)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .flex_1()
                                .min_w_0()
                                .child(div().child(title).truncate())
                                .child(div().opacity(0.6).child(info))
                                .child(div().opacity(0.6).child(size))
                                .child(
                                    Progress::new(SharedString::from(format!(
                                        "task-progress-{id}"
                                    )))
                                    .value(progress),
                                ),
                        )
                        .child(div().w(px(140.)).child(status))
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    Button::new(SharedString::from(format!("task-action-{id}")))
                                        .label(action_caption.clone())
                                        .on_click(cx.listener({
                                            let id = id.clone();
                                            move |shell, _, _, cx| {
                                                shell.downloads.row_action(&id, cx);
                                            }
                                        })),
                                )
                                .child(
                                    Button::new(SharedString::from(format!("task-delete-{id}")))
                                        .label(t("DownloadListView", "Delete"))
                                        .on_click(cx.listener({
                                            let id = id.clone();
                                            move |shell, _, _, cx| {
                                                shell.downloads.row_delete(&id, cx);
                                            }
                                        })),
                                )
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "task-redownload-{id}"
                                    )))
                                    .label(t("DownloadListView", "Re-download"))
                                    .disabled(tab == DownloadsTab::Downloading)
                                    .on_click(cx.listener({
                                        let id = id.clone();
                                        move |shell, _, _, cx| {
                                            shell.downloads.row_redownload(&id, cx);
                                        }
                                    })),
                                )
                                .child(
                                    Button::new(SharedString::from(format!("task-open-dir-{id}")))
                                        .label(t("DownloadListView", "Open Directory"))
                                        .on_click(cx.listener({
                                            let id = id.clone();
                                            move |shell, _, _, cx| {
                                                shell.downloads.row_open_directory(&id, cx);
                                            }
                                        })),
                                ),
                        )
                },
            )
            .collect();

        if rows.is_empty() {
            return div().flex_1().child(empty_text).into_any_element();
        }
        div()
            .id("downloads-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex_col()
            .children(rows)
            .into_any_element()
    }

    /// One row's cover slot: cached image when decoded, a one-shot
    /// async fetch when missing, muted placeholder otherwise. Failed
    /// fetches cache `None` so they don't retry every frame.
    fn render_cover(
        &mut self,
        url: &str,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Shell>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::gpui::{img, Image, ImageSource, ObjectFit, StyledImage};

        fn placeholder() -> gpui_kit::AnyElement {
            div()
                .w(px(72.))
                .h(px(60.))
                .rounded_md()
                .bg(gpui_kit::rgb(0x000000))
                .opacity(0.1)
                .into_any_element()
        }

        if url.is_empty() {
            return placeholder();
        }
        let fetching = self.model.covers_fetching.contains(url);
        match self.model.covers.get(url) {
            Some(Some(image)) => {
                return img(ImageSource::from(image.clone()))
                    .w(px(72.))
                    .h(px(60.))
                    .rounded_md()
                    .object_fit(ObjectFit::Cover)
                    .into_any_element();
            }
            Some(None) => return placeholder(),
            None => {
                if fetching {
                    return placeholder();
                }
            }
        }
        // Spawn the one-shot fetch, then redraw.
        self.model.covers_fetching.insert(url.to_owned());
        let client = crate::ui::parse::dispatch::ParseService::new(&self.config.clone()).client;
        let url_owned_fetch = url.to_owned();
        let url_key = url.to_owned();
        window
            .spawn(cx, async move |cx: &mut gpui_kit::AsyncWindowContext| {
                // gpui's executor is not a tokio context; run the fetch
                // on the shared runtime and await the handle (never
                // hold an EnterGuard across an await on this thread).
                let fetched: anyhow::Result<Arc<Image>> = crate::bootstrap::on_runtime(move || {
                    let url = url_owned_fetch.clone();
                    let client = client.clone();
                    async move {
                        let bytes = client
                            .inner()
                            .get(&url)
                            .send()
                            .await?
                            .error_for_status()?
                            .bytes()
                            .await?
                            .to_vec();
                        let format = image::guess_format(&bytes)
                            .ok()
                            .and_then(Self::gpui_cover_format)
                            .ok_or_else(|| anyhow::anyhow!("unsupported cover image format"))?;
                        Ok(Arc::new(Image::from_bytes(format, bytes)))
                    }
                })
                .await
                .map_err(anyhow::Error::from)
                .and_then(|r| r);
                let _ = cx.update(|window, cx| {
                    if let Some(Some(shell)) = window.root::<Shell>() {
                        shell.update(cx, |shell, cx| {
                            let screen = &mut shell.downloads;
                            screen.model.covers_fetching.remove(&url_key);
                            screen.model.covers.insert(url_key, fetched.ok());
                            cx.notify();
                        });
                    }
                    window.refresh();
                });
            })
            .detach();
        placeholder()
    }

    fn gpui_cover_format(format: image::ImageFormat) -> Option<gpui_kit::gpui::ImageFormat> {
        use gpui_kit::gpui::ImageFormat as G;
        use image::ImageFormat as F;
        Some(match format {
            F::Jpeg => G::Jpeg,
            F::Png => G::Png,
            F::WebP => G::Webp,
            F::Gif => G::Gif,
            F::Bmp => G::Bmp,
            _ => return None,
        })
    }

    /// True while a duplicate ask is on screen (`_process_next_duplicate_download` head).
    pub fn duplicate_ask_pending(&self) -> bool {
        self.model.pending_duplicate_ask.is_some()
    }
}

fn t(context: &str, source: &str) -> String {
    super::dialogs::tr(context, source)
}

fn open_directory(dir: &std::path::Path) -> std::io::Result<()> {
    let dir = dir.to_string_lossy().into_owned();
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("explorer").arg(&dir).spawn();
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(&dir).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let status = std::process::Command::new("xdg-open").arg(&dir).spawn();
    status.map(|_| ())
}

/// Kept for the row-loop signature (TaskInfo rows are owned by the model).
#[allow(dead_code)]
fn rows_use(_: &[TaskInfo]) {}

#[allow(dead_code)]
fn list_state_use(_: &TaskListState) {}
