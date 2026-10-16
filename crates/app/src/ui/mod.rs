//! App UI modules: navigation shell + screens.

pub mod downloads;
pub mod fmt;
pub mod parse;
pub mod screens;

use gpui_kit::component::sidebar::{Sidebar, SidebarMenuItem};
use gpui_kit::{Context, Render, Window, div, prelude::*};

use downloads::screen::DownloadsScreen;
use parse::ParseScreen;

/// Which screen the shell shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Parse,
    Downloads,
    Favorites,
    About,
    Settings,
}

/// The main window navigation shell: sidebar + current screen
/// (port of `src/gui/interface/main_window.py`).
pub struct Shell {
    pub screen: Screen,
    pub parse: ParseScreen,
    pub downloads: DownloadsScreen,
    pub screens: screens::Screens,
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // TaskManager 不可用（无 DB / 只读目录）时下载页降级为空表；
        // 其余屏幕不受影响。
        let config = std::sync::Arc::new(
            config::ConfigStore::open().unwrap_or_else(|_| config::ConfigStore::with_paths("", "")),
        );
        let manager = task_db_path()
            .and_then(|p| tasks::TaskManager::open(p, config.clone()).ok())
            .map(std::sync::Arc::new);
        Self {
            screen: Screen::Parse,
            parse: ParseScreen::new(window, cx),
            downloads: DownloadsScreen::new(config, manager, window, cx),
            screens: screens::Screens::default(),
        }
    }

    /// Window-close seam (ticket 26 owns the wiring in main.rs): show the
    /// exit dialog and hand `(exit, remember)` back.
    pub fn ask_exit(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        on_choice: impl FnOnce(bool, bool) + 'static,
    ) {
        downloads::dialogs::open_exit_dialog(window, cx, on_choice);
    }
}

/// 任务数据库路径：User Data/Bili23 Downloader/task.db（Python appdata 推导）。
fn task_db_path() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join(config::APP_NAME).join("task.db"))
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::assets::IconName;

        let screen = self.screen;
        div()
            .size_full()
            .flex()
            .child(
                Sidebar::new("nav").children(
                    [
                        (Screen::Parse, "Parse", IconName::Search),
                        (Screen::Downloads, "Downloads", IconName::CloudDownload),
                        (Screen::Favorites, "Favorites", IconName::Star),
                        (Screen::About, "About", IconName::Info),
                        (Screen::Settings, "Settings", IconName::Settings),
                    ]
                    .map(|(s, label, icon)| {
                        SidebarMenuItem::new(label)
                            .icon(icon)
                            .active(screen == s)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.screen = s;
                                if s == Screen::Downloads {
                                    this.downloads.refresh(cx);
                                }
                                cx.notify();
                            }))
                    }),
                ),
            )
            .child(match screen {
                Screen::Parse => self.parse.render(cx).into_any_element(),
                Screen::Downloads => self.downloads.render(cx).into_any_element(),
                Screen::Favorites => self.screens.favorites.render().into_any_element(),
                Screen::About => screens::about::render_about().into_any_element(),
                Screen::Settings => self.screens.settings.render().into_any_element(),
            })
    }
}

#[cfg(test)]
mod tests {
    /// `task_db_path` stays inside the user-data dir (never the repo).
    #[test]
    fn task_db_path_lives_in_appdata() {
        let p = super::task_db_path().expect("data_dir available on macOS");
        assert!(p.ends_with("task.db"));
        assert!(p.to_string_lossy().contains(config::APP_NAME));
    }
}
