//! App UI modules: navigation shell + screens.

pub mod dialogs;
pub mod downloads;
pub mod fmt;
pub mod login;
pub mod parse;
pub mod screens;

use gpui_kit::base::Disableable;
use gpui_kit::component::sidebar::{Sidebar, SidebarMenuItem};
use gpui_kit::{div, prelude::*, Context, Global, Render, Window};
use std::sync::Arc;

use downloads::screen::DownloadsScreen;
use login::LoginState;
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

/// App-global i18n language (resolved once per launch; the language
/// setting is applied at startup like the Python build).
pub struct ShellI18n {
    pub lang: services::i18n::Language,
}

impl Global for ShellI18n {}

/// The main window navigation shell: sidebar + current screen
/// (port of `src/gui/interface/main_window.py`).
pub struct Shell {
    pub screen: Screen,
    pub parse: ParseScreen,
    pub downloads: DownloadsScreen,
    pub screens: screens::Screens,
    /// Login state (auth stack, ticket 23): dialog handles + models.
    pub login: LoginState,
    /// Auth state loaded from config.json; login flows capture into it
    /// and persist back (`UserManager`/`CookieManager` equivalent).
    pub auth_state: auth::AuthState,
    /// The shared auth client: cookie jar survives across the login
    /// dialogs so captured logins stick.
    pub auth_client: Arc<network::HttpClient>,
    pub auth_wbi: Arc<auth::WbiKeyStore>,
    pub user_agent: String,
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // i18n global: `General.language` (auto → system)
        let cfg = Arc::new(
            config::ConfigStore::open().unwrap_or_else(|_| config::ConfigStore::with_paths("", "")),
        );
        let lang = services::i18n::Language::from_config(
            cfg.get("General", "language")
                .and_then(|v| v.as_str())
                .unwrap_or("auto"),
        );
        cx.set_global(ShellI18n { lang });

        let user_agent = cfg
            .get("Advanced", "user_agent")
            .and_then(|v| v.as_str())
            .unwrap_or("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36")
            .to_string();
        let client = Arc::new(
            network::HttpClient::new(network::ClientOptions {
                user_agent: user_agent.clone(),
                proxy: network::ProxyConfig::none(),
                cookies_from_persistence: Vec::new(),
                signer: None,
            })
            .expect("rustls client builds with a valid UA"),
        );
        let wbi = Arc::new(auth::WbiKeyStore::new());

        // config.json Cookie section (auth stack's persistence point)
        let dir = cfg
            .path()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let auth_state = auth::AuthState::from_config_file(&auth::session::config_path(&dir))
            .unwrap_or_default();
        let is_login = auth_state.is_login;

        // TaskManager 不可用（无 DB / 只读目录）时下载页降级为空表；
        // 其余屏幕不受影响。
        let config = cfg.clone();
        let manager = task_db_path()
            .and_then(|p| tasks::TaskManager::open(p, config.clone()).ok())
            .map(Arc::new);
        dialogs::update::run_startup_check(window, cx);
        Self {
            screen: Screen::Parse,
            parse: ParseScreen::new(window, cx),
            downloads: DownloadsScreen::new(config, manager, window, cx),
            screens: screens::Screens::default(),
            login: LoginState::new(window, cx, is_login),
            auth_state,
            auth_client: client,
            auth_wbi: wbi,
            user_agent,
        }
    }

    /// Python `on_avatar_click` with `!is_login` → open the login dialog.
    pub fn open_login(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.login.open(window, cx);
    }

    /// Any login success: flip the shell flag (sidebar button reflects it).
    pub fn on_login_success(&mut self, cx: &mut Context<Self>) {
        self.login.is_login = true;
        cx.notify();
    }

    /// Capture a login-updated auth state: keep the in-memory copy in
    /// sync and persist the `Cookie` section back to config.json.
    pub fn set_auth_state(&mut self, state: auth::AuthState, cx: &mut Context<Self>) {
        self.auth_state = state.clone();
        let dir = config::ConfigStore::open()
            .ok()
            .and_then(|c| c.path().parent().map(|p| p.to_path_buf()))
            .unwrap_or_default();
        let _ = login::controller::persist(&state, &dir);
        self.login.is_login = state.is_login;
        cx.notify();
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::assets::IconName;

        let screen = self.screen;
        let is_login = self.login.is_login;
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
                Screen::About => screens::about::render_about(cx).into_any_element(),
                Screen::Settings => self.screens.settings.render(window, cx).into_any_element(),
            })
            .child(
                // login entry (Python: the avatar button on the main page)
                div()
                    .id("shell-login")
                    .absolute()
                    .bottom_4()
                    .left_4()
                    .child(
                        gpui_kit::component::button::Button::new("shell-login-button")
                            .label(if is_login { "Logged in" } else { "Log In" })
                            .disabled(is_login)
                            .on_click(cx.listener(|shell, _, window, cx| {
                                if !shell.login.is_login {
                                    shell.open_login(window, cx);
                                }
                            })),
                    ),
            )
            .child(self.login.render(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Headless smoke: the login model starts logged-out and idle.
    #[test]
    fn login_state_defaults_to_logged_out() {
        let state = login::model::LoginModel::new();
        assert_eq!(state.sms, login::model::SmsPhase::Idle);
        assert_eq!(state.qr, login::model::QrPhase::Loading);
        assert!(state.qr_key.is_empty());
    }
}

#[cfg(test)]
mod task_path_tests {
    /// `task_db_path` stays inside the user-data dir (never the repo).
    #[test]
    fn task_db_path_lives_in_appdata() {
        let p = super::task_db_path().expect("data_dir available on macOS");
        assert!(p.ends_with("task.db"));
        assert!(p.to_string_lossy().contains(config::APP_NAME));
    }
}
