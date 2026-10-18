//! App UI modules: navigation shell + screens.

pub mod dialogs;
pub mod download_options;
pub mod downloads;
pub mod fmt;
pub mod login;
pub mod parse;
pub mod screens;
pub mod settings;

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

/// One clipboard read handed to the monitor from a UI-thread poll.
struct PolledClipboard(Option<String>);

impl services::clipboard::ClipboardSource for PolledClipboard {
    fn text(&mut self) -> Option<String> {
        self.0.take()
    }
}

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
    pub auth_client: Arc<network::HttpClient>,
    pub auth_wbi: Arc<auth::WbiKeyStore>,
    pub user_agent: String,
    /// Task manager handle (None when the DB is unavailable): task
    /// creation from the parse screen routes through it.
    pub manager: Option<Arc<tasks::TaskManager>>,
    /// Clipboard monitor (F2): one per window; checked on interval.
    pub clipboard: services::clipboard::ClipboardMonitor,
    /// Tick counter driving the clipboard poll (see `Shell::render`).
    pub clipboard_ticks: u32,
    /// Transient shell-level note (tips, area result).
    pub status_note: Option<String>,
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // ONE file, opened once: the primary Arc feeds TaskManager, the
        // download driver and every UI read; MCP gets its own Arc<Mutex>
        // store re-opened from the same path (its tools take that shape,
        // so their writes stay visible inside the MCP server).
        let cfg = Arc::new(
            config::ConfigStore::open().unwrap_or_else(|_| config::ConfigStore::with_paths("", "")),
        );
        let cfg_dir = cfg
            .path()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        // i18n global: `General.language` (auto → system)
        let lang = services::i18n::Language::from_config(
            cfg.get("General", "language")
                .and_then(|v| v.as_str())
                .unwrap_or("auto"),
        );
        cx.set_global(ShellI18n { lang });

        let user_agent = cfg
            .get("Advanced", "user_agent")
            .and_then(|v| v.as_str())
            .unwrap_or(config::DEFAULT_USER_AGENT)
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
        let auth_state = auth::AuthState::from_config_file(&auth::session::config_path(&cfg_dir))
            .unwrap_or_default();
        let is_login = auth_state.is_login;

        // TaskManager 不可用（无 DB / 只读目录）时下载页降级为空表；
        // 其余屏幕不受影响。
        let config = cfg.clone();
        let manager = task_db_path()
            .and_then(|p| tasks::TaskManager::open(p, config.clone()).ok())
            .map(Arc::new);
        // Bootstrap pass (ticket 31 F8/F2): download driver + MCP server
        // start with the shell. Python runs these after the first frame;
        // a same-turn start is equivalent — neither touches the UI.
        if let Some(manager) = manager.clone() {
            crate::bootstrap::spawn_download_driver(config.clone(), manager.clone());
            // MCP tools hold their config as Arc<Mutex<ConfigStore>>;
            // re-open from the same file (writes land on disk, the
            // shell's copy keeps its in-memory view as before).
            let mcp_cfg = config::ConfigStore::with_paths(cfg_dir.clone(), "");
            crate::bootstrap::start_mcp_server(Arc::new(parking_lot::Mutex::new(mcp_cfg)), manager);
        }

        // Post-terms pass (F2), mirroring Python's `run_post_terms_checks`
        // / `QTimer.singleShot(0, show_terms_of_use)`: the gate runs after
        // the first frame; tips follow only once accepted.
        let shell_for_gate = cx.entity();
        // The gate takes an owned store (its accept re-opens + persists
        // `Application/accepted_terms`); re-open from the same file.
        let gate_cfg = config::ConfigStore::with_paths(cfg_dir, "");

        window.on_next_frame(move |window, cx| {
            if services::terms::gate(&gate_cfg) == services::terms::TermsGate::Pending {
                dialogs::terms::open_terms_gate(
                    gate_cfg,
                    // accepted → run the tip pass
                    {
                        let shell = shell_for_gate.clone();
                        move |window, cx| {
                            shell.update(cx, |shell, cx| shell.run_tip_pass(window, cx));
                        }
                    },
                    // rejected → close the app (Python `self.close()`)
                    {
                        let shell = shell_for_gate.clone();
                        move |_, cx| {
                            shell.update(cx, |_, cx| cx.quit());
                        }
                    },
                    window,
                    cx,
                );
            } else {
                shell_for_gate.update(cx, |shell, cx| shell.run_tip_pass(window, cx));
            }
        });

        // Clipboard monitor (F2): poll on the foreground executor; the
        // startup check mirrors Python's `check_clipboard_on_startup`.
        cx.spawn(async move |shell, cx| {
            shell
                .update(cx, |shell, cx| {
                    shell.check_clipboard_once(cx);
                })
                .ok();
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                let Ok(()) = shell.update(cx, |shell, cx| shell.check_clipboard_once(cx)) else {
                    return; // shell dropped; stop polling
                };
            }
        })
        .detach();
        dialogs::update::run_startup_check(window, cx);
        let settings = settings::SettingsScreen::new(window, cx);
        // Settings writes notify their entity; observe it so the shell
        // redraws (the subscription lives as long as the shell).
        cx.observe(&settings.ui, |_, _, cx| cx.notify()).detach();
        Self {
            screen: Screen::Parse,
            parse: ParseScreen::new(window, cx),
            downloads: DownloadsScreen::new(config, manager.clone(), window, cx),
            screens: screens::Screens {
                favorites: Default::default(),
                settings,
            },
            login: LoginState::new(window, cx, is_login),
            auth_state,
            auth_client: client,
            auth_wbi: wbi,
            manager,
            user_agent,
            clipboard: services::clipboard::ClipboardMonitor::new(),
            clipboard_ticks: 0,
            status_note: None,
        }
    }

    /// One clipboard poll (Python `_parse_clipboard_url`). A parseable URL
    /// fills the parse input and starts the parse.
    pub fn check_clipboard_once(&mut self, cx: &mut Context<Self>) {
        let Ok(cfg) = config::ConfigStore::open() else {
            return;
        };
        let text = cx.read_from_clipboard().and_then(|item| item.text());
        let mut source = PolledClipboard(text);
        let clock = services::clipboard::SystemClock::new();
        if let services::clipboard::ClipboardDecision::Parse(url) =
            self.clipboard.check(&cfg, &mut source, &clock)
        {
            let auth = self.auth_state.clone();
            self.parse.start_parse_from_clipboard(url, &auth, cx);
            cx.notify();
        }
    }

    /// Post-terms tip pass (Python `run_post_terms_checks`): tutorial
    /// dialog, area selection, login teaching tip — in that order, each
    /// gated by its once-flag.
    pub fn run_tip_pass(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut cfg =
            config::ConfigStore::open().unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
        match services::tips::next_tip(&cfg) {
            services::tips::PendingTip::TutorialDialog => {
                services::tips::mark_tutorial_shown(&mut cfg);
                let _ = cfg.save();
                // The dialog itself is the doc link; the shell has no
                // browser abstraction, so mark-and-continue keeps the
                // once-flag behavior identical.
                self.status_note =
                    Some("Welcome! See https://github.com/stanzhai/bilidown".to_owned());
            }
            services::tips::PendingTip::SelectAreaDialog => {
                services::tips::mark_area_dialog_shown(&mut cfg);
                let _ = cfg.save();
                // Python then shows the select-area dialog; its detected
                // default pre-selects the radio row.
                dialogs::select_area::open_select_area(cfg, window, cx);
                // Area auto-detection (Python `detect_area` worker): zone
                // probe on the detection client; failure keeps the
                // default `cn` list, matching the silent fallback.
                let user_agent = self.user_agent.clone();
                std::thread::spawn(move || {
                    let area = parsers::area::detection_client(parsers::area::DetectOptions {
                        user_agent,
                        ..Default::default()
                    })
                    .ok()
                    .and_then(|client| {
                        crate::bootstrap::runtime()
                            .block_on(parsers::area::detect_area(&client, ""))
                            .ok()
                    });
                    if let Some(area) = area {
                        if let Ok(mut cfg) = config::ConfigStore::open() {
                            cfg.set(
                                "Advanced",
                                "area",
                                serde_json::json!(if area == network::cdn::Area::Cn {
                                    "cn"
                                } else {
                                    "ov"
                                }),
                            );
                            let _ = cfg.save();
                        }
                    }
                });
            }
            services::tips::PendingTip::LoginTeachingTip => {
                // The login tip repeats while logged out (no flag), so
                // surface it as a shell note instead of a modal.
                self.status_note =
                    Some("Click the avatar to log in for full functionality.".to_owned());
                cx.notify();
            }
            _ => {}
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

/// 任务数据库路径：User Data/bilidown/task.db（Python appdata 推导）。
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

    /// Regression (double-lease panic): clicking "Log In" opens the login
    /// dialog from inside the Shell's update lease. Reading the shell
    /// synchronously there used to panic with "cannot read Shell while it
    /// is already being updated" (gpui entity_map.rs). The QR kickoff now
    /// reads via the async context instead.
    // Network-real: start_qr hits the live QR API on the app's tokio
    // runtime, which gpui's deterministic test scheduler flags as
    // "not deterministic" when other tests run in parallel. Run on
    // demand with: cargo test -p app open_login -- --ignored
    #[ignore]
    #[gpui::test]
    async fn open_login_does_not_double_lease_shell(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_kit::init);
        let handle = cx.add_window(Shell::new);
        let window = gpui::AnyWindowHandle::from(handle);
        cx.update_window(window, |root, window, cx| {
            let shell = root.downcast::<Shell>().expect("root is Shell");
            shell.update(cx, |shell, cx| {
                // Even with a stale logged-in config the dialog must open
                // without the double-lease panic; the button is disabled
                // when logged in, so force the flag off first.
                shell.login.is_login = false;
                shell.open_login(window, cx);
                assert!(shell.login.handle.is_open());
            });
        })
        .unwrap();
        cx.run_until_parked();
        let open = handle
            .read_with(cx, |shell: &Shell, _| shell.login.handle.is_open())
            .expect("window alive");
        assert!(open, "login dialog must stay open");
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
