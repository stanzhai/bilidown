//! App UI modules: navigation shell + screens.

pub mod dialogs;
pub mod download_options;
pub mod downloads;
pub mod fmt;
pub mod login;
pub mod parse;
pub mod screens;
pub mod settings;

use gpui_kit::component::label::Label;
use gpui_kit::component::theme::ActiveTheme;
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, px, Context, Global, Render, Window};
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
    /// Render language the login placeholders were built for (refreshed
    /// in `render` when the language setting flips).
    pub login_placeholders_lang: services::i18n::Language,
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
        // i18n global: `Interface.language` (auto → system)
        let lang = services::i18n::Language::from_config(
            cfg.get("Interface", "language")
                .and_then(|v| v.as_str())
                .unwrap_or("auto"),
        );
        cx.set_global(ShellI18n { lang });
        crate::ui::download_options::set_shell_lang(lang);

        let user_agent = cfg
            .get("Advanced", "user_agent")
            .and_then(|v| v.as_str())
            .unwrap_or(config::DEFAULT_USER_AGENT)
            .to_string();
        // config.json Cookie section (auth stack's persistence point);
        // read before the auth client so the visitor + login sets are
        // seeded the moment it exists (ticket 34).
        let auth_state = auth::AuthState::from_config_file(&auth::session::config_path(&cfg_dir))
            .unwrap_or_default();
        let is_login = auth_state.is_login;
        let client = Arc::new(
            network::HttpClient::new(network::ClientOptions {
                user_agent: user_agent.clone(),
                proxy: network::ProxyConfig::none(),
                cookies_from_persistence: Vec::new(),
                signer: None,
            })
            .expect("rustls client builds with a valid UA"),
        );
        auth::seed_client_cookies(&client, &auth_state);
        let wbi = Arc::new(auth::WbiKeyStore::new());

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
        // Auto theme: follow OS appearance changes live. The observer
        // fires on every appearance flip; only Auto mode re-syncs (Light/
        // Dark are pinned by the user's setting). Shares the Arc'd config
        // store the shell already opened.
        let theme_cfg = cfg.clone();
        window
            .observe_window_appearance(move |window, cx| {
                let mode = theme_cfg
                    .get("QFluentWidgets", "ThemeMode")
                    .and_then(|v| v.as_str())
                    .map(services::theme::ThemeModeSetting::from_config)
                    .unwrap_or(services::theme::ThemeModeSetting::Auto);
                if matches!(mode, services::theme::ThemeModeSetting::Auto) {
                    gpui_kit::component::theme::Theme::sync_system_appearance(Some(window), cx);
                }
            })
            .detach();
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
            login_placeholders_lang: lang,
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
        let c = dialogs::active_catalog(&cfg);
        match services::tips::next_tip(&cfg) {
            services::tips::PendingTip::TutorialDialog => {
                services::tips::mark_tutorial_shown(&mut cfg);
                let _ = cfg.save();
                // The dialog itself is the doc link; the shell has no
                // browser abstraction, so mark-and-continue keeps the
                // once-flag behavior identical.
                self.status_note = Some(
                    c.tr(
                        "MainWindow",
                        "Welcome! See https://github.com/stanzhai/bilidown",
                    )
                    .to_owned(),
                );
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
                self.status_note = Some(
                    c.tr(
                        "MainWindow",
                        "Click the avatar to log in for full functionality.",
                    )
                    .to_owned(),
                );
                cx.notify();
            }
            _ => {}
        }
    }

    /// Python `on_avatar_click` with `!is_login` → open the login dialog.
    pub fn open_login(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.login.open(window, cx);
    }

    /// Any login success: flip the shell flag (sidebar button reflects it)
    /// and refresh the account uname for the footer tooltip.
    pub fn on_login_success(&mut self, cx: &mut Context<Self>) {
        self.login.is_login = true;
        self.refresh_uname(cx);
        cx.notify();
    }

    /// Fetch `uname` from nav and persist it — the login footer's tooltip
    /// data source. No-op while logged out; a failed fetch keeps the
    /// persisted value.
    pub fn refresh_uname(&mut self, cx: &mut Context<Self>) {
        if !self.auth_state.is_login {
            return;
        }
        cx.spawn(async move |shell, cx| {
            let _tokio = crate::bootstrap::tokio_context();
            let Ok(client) = shell.read_with(cx, |shell, _| shell.auth_client.clone()) else {
                return;
            };
            let Ok(uname) = auth::login::fetch_uname(client.as_ref(), auth::wbi::NAV_API).await
            else {
                return; // offline startup keeps the persisted uname
            };
            shell
                .update(cx, |shell, cx| {
                    if uname.is_empty() {
                        return;
                    }
                    shell.auth_state.uname = uname;
                    let dir = config::ConfigStore::open()
                        .ok()
                        .and_then(|c| c.path().parent().map(|p| p.to_path_buf()))
                        .unwrap_or_default();
                    let _ = login::controller::persist(&shell.auth_state, &dir);
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// Startup pass piped in `main.rs` right after `open_window`: with a
    /// persisted session, refresh the uname so the footer shows the
    /// account name instead of a stale "Log In".
    pub fn boot(shell: &gpui_kit::Entity<Self>, cx: &mut gpui_kit::App) {
        shell.update(cx, |shell, cx| shell.refresh_uname(cx));
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

    /// Logout (Python ProfileCard): confirm-dialog OK calls this. Runs the
    /// logout API on the tokio runtime, then on the UI thread clears the
    /// login cookies the jar still holds (the API only clears them
    /// server-side), re-seeds visitor cookies, persists the anonymous
    /// state and pushes a notification.
    pub fn logout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let lang = cx
            .read_global::<ShellI18n, _>(|i18n, _| i18n.lang)
            .resolve();
        let cat = services::i18n::catalog(lang);
        let ok_msg = cat.tr("ProfileCard", "Successfully logged out").to_owned();
        let err_msg = cat.tr("ERROR_MESSAGES", "Logout failed").to_owned();

        let shell = cx.entity();
        window
            .spawn(cx, async move |cx: &mut gpui_kit::AsyncWindowContext| {
                let _tokio = crate::bootstrap::tokio_context();
                let Ok((mut state, client, user_agent)) = cx.update(|_, cx| {
                    shell.update(cx, |shell, _| {
                        (
                            shell.auth_state.clone(),
                            shell.auth_client.clone(),
                            shell.user_agent.clone(),
                        )
                    })
                }) else {
                    return;
                };
                let result = {
                    let mut session =
                        login::controller::session_for(client.as_ref(), &mut state, &user_agent);
                    auth::login::logout(session.client, &mut session).await
                };
                let _ = cx.update(move |window, cx| {
                    shell.update(cx, |shell, cx| match result {
                        Ok(()) => {
                            for name in ["SESSDATA", "bili_jct", "DedeUserID", "DedeUserID__ckMd5"]
                            {
                                client.set_cookie(name, "", ".bilibili.com", "/");
                            }
                            auth::seed_client_cookies(client.as_ref(), &state);
                            shell.set_auth_state(state, cx);
                            window.push_notification(
                                gpui_kit::component::notification::Notification::success(
                                    ok_msg.clone(),
                                ),
                                cx,
                            );
                        }
                        Err(_) => {
                            window.push_notification(
                                gpui_kit::component::notification::Notification::error(
                                    err_msg.clone(),
                                ),
                                cx,
                            );
                        }
                    });
                });
            })
            .detach();
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

        // Login placeholders are baked into `InputState` at construction;
        // re-derive them whenever the render language moves (ticket 36).
        let render_lang = cx.read_global::<ShellI18n, _>(|i18n, _| i18n.lang);
        if render_lang != self.login_placeholders_lang {
            self.login_placeholders_lang = render_lang;
            self.login.refresh_placeholders(window, cx);
        }

        let screen = self.screen;
        let is_login = self.login.is_login;
        let uname = self.auth_state.uname.clone();
        let lang = cx
            .read_global::<ShellI18n, _>(|i18n, _| i18n.lang)
            .resolve();
        let c = services::i18n::catalog(lang);
        let login_icon = if is_login {
            IconName::CircleUser
        } else {
            IconName::LogIn
        };
        div()
            .size_full()
            .flex()
            .child(
                // Hand-drawn icon rail (ticket 5): kit `Sidebar` pins a
                // 48px collapsed rail with horizontal items — too small
                // for icons stacked above their labels, so the app draws
                // its own. Compact 60px column. Nav entries above, login
                // entry below.
                div()
                    .id("nav-rail")
                    .w(px(60.))
                    .h_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_1()
                    .py_2()
                    .bg(cx.theme().sidebar)
                    .border_r_1()
                    .border_color(cx.theme().border)
                    .children(
                        [
                            (Screen::Parse, "Parser", IconName::Search),
                            (Screen::Downloads, "Downloads", IconName::CloudDownload),
                            (Screen::Favorites, "Favorites", IconName::Star),
                            (Screen::About, "About", IconName::Info),
                            (Screen::Settings, "Settings", IconName::Settings),
                        ]
                        .map(|(s, label, icon)| {
                            let active = screen == s;
                            div()
                                .id(format!("nav-item-{label}"))
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap_0p5()
                                .py_1()
                                .w(px(56.))
                                .rounded_md()
                                .when(active, |this| {
                                    this.bg(cx.theme().sidebar_accent)
                                })
                                .text_color(if active {
                                    cx.theme().sidebar_accent_foreground
                                } else {
                                    cx.theme().muted_foreground
                                })
                                .hover(|this| this.bg(cx.theme().accent))
                                .child(
                                    gpui_kit::component::Icon::new(icon)
                                        .size_5()
                                        .text_color(if active {
                                            cx.theme().sidebar_accent_foreground
                                        } else {
                                            cx.theme().muted_foreground
                                        }),
                                )
                                .child(
                                    Label::new(c.tr("MainWindow", label)).text_size(px(10.)),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.screen = s;
                                    if s == Screen::Downloads {
                                        this.downloads.refresh(cx);
                                    }
                                    cx.notify();
                                }))
                        }),
                    )
                    .child(
                        div().flex_1(), // spacer pushes the login entry down
                    )
                    .child(
                        // login entry (Python: the avatar button on the
                        // main page) — logged out opens the login dialog;
                        // logged in shows the uname and confirms logout.
                        div()
                            .id("shell-login-button")
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_0p5()
                            .py_1()
                            .w(px(56.))
                            .rounded_md()
                            .text_color(cx.theme().muted_foreground)
                            .hover(|this| this.bg(cx.theme().accent))
                            .child(
                                gpui_kit::component::Icon::new(login_icon)
                                    .size_5()
                                    .text_color(cx.theme().muted_foreground),
                            )
                            .child(
                                Label::new(
                                    if is_login && !uname.is_empty() {
                                        uname.clone()
                                    } else {
                                        c.tr("LoginDialog", "Log In").to_owned()
                                    },
                                )
                                .text_size(px(10.))
                                .w(px(52.))
                                .truncate(),
                            )
                            .on_click(cx.listener(|shell, _, window, cx| {
                                if !shell.login.is_login {
                                    shell.open_login(window, cx);
                                    return;
                                }
                                let shell_for_ok = std::rc::Rc::new(cx.entity());
                                let lang = cx
                                    .read_global::<ShellI18n, _>(|i18n, _| i18n.lang)
                                    .resolve();
                                let cat = services::i18n::catalog(lang);
                                let title = cat
                                    .tr("ProfileCard", "Are you sure you want to log out? This will also clear locally stored cookies.")
                                    .to_owned();
                                let ok = cat.tr("ProfileCard", "Logout").to_owned();
                                let cancel = cat.tr("RuleListDialog", "Cancel").to_owned();
                                window.open_alert_dialog(cx, move |alert, _, _| {
                                    alert
                                        .title(title.clone())
                                        .confirm()
                                        .ok_text(ok.clone())
                                        .cancel_text(cancel.clone())
                                        .on_ok({
                                            let shell_for_ok = shell_for_ok.clone();
                                            move |_, window, cx| {
                                                shell_for_ok.update(cx, |shell, cx| {
                                                    shell.logout(window, cx)
                                                });
                                                true
                                            }
                                        })
                                });
                            })),
                    ),
            )
            .child(match screen {
                Screen::Parse => self.parse.render(cx).into_any_element(),
                Screen::Downloads => self.downloads.render(cx).into_any_element(),
                Screen::Favorites => self.screens.favorites.render(cx).into_any_element(),
                Screen::About => screens::about::render_about(cx).into_any_element(),
                Screen::Settings => self.screens.settings.render(window, cx).into_any_element(),
            })
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

    /// Completeness: every source string routed through the catalog in
    /// this i18n sweep must exist — `tr()`'s source fallback would
    /// otherwise silently mask a typo. (The En catalog is the identity
    /// map, so lookups run against zh-CN, which carries every entry.)
    #[test]
    fn i18n_sweep_sources_resolve_in_catalog() {
        let c = services::i18n::catalog(services::i18n::ResolvedLanguage::ZhCn);
        for (ctx, source) in [
            // Shell / screens
            ("MainWindow", "Parser"),
            ("MainWindow", "Downloads"),
            ("MainWindow", "Favorites"),
            ("MainWindow", "About"),
            ("MainWindow", "Settings"),
            ("MainWindow", "Logged in"),
            (
                "MainWindow",
                "Welcome! See https://github.com/stanzhai/bilidown",
            ),
            (
                "MainWindow",
                "Click the avatar to log in for full functionality.",
            ),
            ("LoginDialog", "Log In"),
            ("LoginDialog", "Enter phone number"),
            ("LoginDialog", "Enter verification code"),
            (
                "CookieLoginDialog",
                "SESSDATA=xxx;bili_jct=xxx;DedeUserID=xxx;DedeUserID__ckMd5=xxx",
            ),
            // Parse screen + pager
            ("ParseInterface", "Paste a Bilibili link"),
            ("ParseInterface", "Search…"),
            ("ParseInterface", "Search"),
            ("ParseInterface", "Parse"),
            ("ParseInterface", "Batch Parse"),
            ("ParseInterface", "Parsing History"),
            ("ParseInterface", "Auto-parse Pagination"),
            ("ParseInterface", "Download Selected Items"),
            ("ParseInterface", "Parsing…"),
            ("ParseInterface", "Preparing…"),
            ("ParseInterface", "Parse failed: {error}"),
            ("ParseInterface", "Batch parse failed: {error}"),
            ("ParseInterface", "Auto-parse failed: {error}"),
            ("ParseInterface", "Task creation failed: {error}"),
            ("ParseInterface", "Queued {count} task(s)"),
            ("ParseInterface", " ({count} duplicate(s) skipped)"),
            ("ParseInterface", "Invalid page number: {body}"),
            ("ParseInterface", "Downloads unavailable (no task database)"),
            ("Pager", "{total_pages} total / {total_items} items"),
            ("Pager", "Please enter a number between 1 and {total_pages}"),
            ("SearchWidget", "{index} of {count}"),
            ("SearchWidget", "No matches found"),
            (
                "ParseInterface",
                "{category_name} ({selected_count} selected, {total_count} total)",
            ),
            ("ParseInterface", "{category_name} ({total_count} total)"),
            // Download options dialog
            ("DownloadOptionsDialog", "Download Options"),
            ("DownloadOptionsDialog", "Media Settings"),
            ("DownloadOptionsDialog", "Additional Files"),
            ("DownloadOptionsDialog", "Download Settings"),
            ("DownloadOptionsDialog", "Rule Builder"),
            ("DownloadOptionsDialog", "Container"),
            ("DownloadPathSettingCard", "Download Path"),
            ("DownloadFormatCard", "Output Container Format"),
            ("DownloadFormatCard", "Convert M4A to MP3"),
            ("DownloadOptionsDialog", "Metadata format"),
            ("DownloadOptionsDialog", "Naming rules"),
            ("DownloadOptionsDialog", "Filename preview"),
            ("DownloadOptionsDialog", "Cancel"),
            ("DownloadOptionsDialog", "Download"),
            ("DownloadSettingsPage", "Automatically show this dialog"),
            ("MediaOptionsCard", "Download standalone video stream"),
            ("MediaOptionsCard", "Download standalone audio stream"),
            ("MediaOptionsCard", "Merge video and audio"),
            ("MediaOptionsCard", "Keep original files"),
            ("MediaInfoCard", "Video Quality"),
            ("MediaInfoCard", "Audio Quality"),
            ("MediaInfoCard", "Video Codec"),
            ("MediaInfoCard", "Unknown"),
            ("DanmakuSettingCard", "Download Danmaku"),
            ("DanmakuSettingCard", "Embed Danmaku"),
            ("DanmakuSettingCard", "Delete Danmaku After Embedding"),
            ("SubtitleSettingCard", "Download Subtitles"),
            ("SubtitleSettingCard", "Embed Subtitles"),
            ("SubtitleSettingCard", "Delete Subtitles After Embedding"),
            ("CoverSettingCard", "Cover Format"),
            ("CoverSettingCard", "Embed Cover"),
            ("CoverSettingCard", "Delete Cover After Embedding"),
            ("ChapterSettingCard", "Embed Chapters"),
            ("MetadataSettingCard", "Download Metadata"),
            ("NamingConventionCard", "Not available"),
            ("RuleBuilderWidget", "File name"),
            ("RuleBuilderWidget", "directory {index}"),
            ("RuleBuilderWidget", "text fragment"),
            ("RuleBuilderWidget", "format ({variable})"),
            ("RuleBuilderWidget", "default"),
            ("RuleBuilderWidget", "Insert title variable"),
            ("RuleBuilderWidget", "Add directory level"),
            ("RuleBuilderWidget", "rule not editable: {reason}"),
            ("RuleLevelRow", "Insert text"),
            ("FragmentChip", "Remove"),
            ("DownloadPreviewBar", "Video"),
            ("DownloadPreviewBar", "Audio"),
            ("DownloadPreviewBar", "Danmaku"),
            ("DownloadPreviewBar", "Cover"),
            ("DownloadPreviewBar", "Chapters"),
            ("DownloadPreviewBar", "Metadata"),
            ("DownloadPreviewBar", "Nothing selected"),
            // Dialogs
            ("AutoParseDialog", "Cancel"),
            ("CDNServerDialog", "Edit"),
            ("CDNServerDialog", "Remove"),
            ("CDNServerDialog", "OK"),
            ("ViewCoverDialog", "Close"),
            ("ViewCoverDialog", "Loading..."),
            ("BatchSelectDialog", "OK"),
            ("BatchSelectDialog", "Enter line numbers (e.g. 1,3,5-10)"),
            ("JumpToPageDialog", "OK"),
            ("AlignmentGroup", "Alignment"),
            // i18n coverage wave: settings card contexts + scattered sites
            ("ADDITIONAL_FILES_QUALIFIER", "Danmaku"),
            ("ADDITIONAL_FILES_QUALIFIER", "Subtitles"),
            ("AUDIO_QUALITY", "Auto"),
            ("CDNSettingCard", "Configure…"),
            ("CheckUpdateSettingCard", "Check for Updates"),
            ("CheckUpdateSettingCard", "Include Prerelease Versions"),
            ("ContentWidget", "Customize Danmaku Style"),
            ("ContentWidget", "Customize Subtitle Style"),
            ("CoverSettingCard", "Download Cover"),
            ("DanmakuSettingCard", "Danmaku Format"),
            (
                "DownloadConcurrencySettingCard",
                "Number of Parallel Downloads",
            ),
            ("DownloadConcurrencySettingCard", "Number of Threads"),
            ("DownloadFormatCard", "Download Format"),
            ("DownloadHandlingSettingCard", "Auto-rename"),
            ("DownloadHandlingSettingCard", "Continue"),
            ("DownloadHandlingSettingCard", "Download Handling"),
            (
                "DownloadHandlingSettingCard",
                "Duplicate Download Resolution",
            ),
            ("DownloadHandlingSettingCard", "File Conflict Resolution"),
            ("DownloadHandlingSettingCard", "Maximum Retry Attempts"),
            ("DownloadHandlingSettingCard", "Overwrite"),
            ("DownloadHandlingSettingCard", "Preallocate File Space"),
            (
                "DownloadHandlingSettingCard",
                "Retry Failed Downloads Automatically",
            ),
            (
                "DownloadHandlingSettingCard",
                "Show Download Options Dialog",
            ),
            ("DownloadHandlingSettingCard", "Show Notifications"),
            ("DownloadHandlingSettingCard", "Skip"),
            ("DownloadListView", "Delete"),
            ("EditRuleDialog", "Naming Rule"),
            ("EditRuleDialog", "Rule Name"),
            ("EditRuleDialog", "Set as default rule for this type"),
            ("EditRuleDialog", "Variable reference"),
            ("FFmpegSettingCard", "Custom path"),
            ("FFmpegSettingCard", "System PATH"),
            ("MCPSettingCard", "Client Configuration"),
            ("MCPSettingCard", "Client configuration copied"),
            ("MCPSettingCard", "Copy"),
            ("MCPSettingCard", "Enable MCP Server"),
            ("MCPSettingCard", "MCP Server"),
            ("MCPSettingCard", "Port"),
            ("MCPSettingCard", "Regenerate"),
            ("MediaOptionsCard", "Audio Only"),
            ("MediaOptionsCard", "Both"),
            ("MediaOptionsCard", "Media Options"),
            ("MediaOptionsCard", "Original file type to keep"),
            ("MediaOptionsCard", "Video Only"),
            ("MetadataSettingCard", "Metadata Format"),
            ("NamingConventionCard", "Naming Convention"),
            ("NumberSettingCard", "Global Sequential Starting Number"),
            ("NumberSettingCard", "Global sequential numbering"),
            ("NumberSettingCard", "Numbering Mode"),
            (
                "NumberSettingCard",
                "Sequential numbering starting from 1 per batch",
            ),
            ("NumberSettingCard", "Use the index from the parse list"),
            ("ParsingSettingCard", "Save Parse History"),
            ("PersonalizationCard", "Dark"),
            ("PersonalizationCard", "Light"),
            ("PersonalizationCard", "Personalization"),
            ("PersonalizationCard", "System default"),
            ("PersonalizationCard", "Theme"),
            ("ProxyDialog", "Password"),
            ("ProxyDialog", "Port"),
            ("ProxyDialog", "Proxy Type"),
            ("ProxyDialog", "Username"),
            ("RuleLevelRow", "Move down"),
            ("RuleLevelRow", "Move up"),
            ("RuleListDialog", "Add"),
            ("RuleListDialog", "Delete"),
            ("RuleListDialog", "Discard changes?"),
            ("RuleListDialog", "Duplicate"),
            ("RuleListDialog", "Naming Rules"),
            ("RuleListDialog", "Naming rules saved"),
            ("RuleListDialog", "Reset to Default"),
            ("RuleListDialog", "Restore to Built-in Default"),
            ("RuleListDialog", "Restored to the built-in default"),
            ("RuleListDialog", "Save"),
            ("SettingInterface", "ASS"),
            ("SettingInterface", "AVIF"),
            ("SettingInterface", "Additional"),
            ("SettingInterface", "Area"),
            ("SettingInterface", "CDN"),
            ("SettingInterface", "CDN provider"),
            ("SettingInterface", "Change…"),
            ("SettingInterface", "Chapter and metadata"),
            ("SettingInterface", "Concurrency"),
            ("SettingInterface", "Custom FFmpeg path"),
            ("SettingInterface", "Custom proxy"),
            ("SettingInterface", "Customize naming rules…"),
            ("SettingInterface", "Display scaling"),
            ("SettingInterface", "FFmpeg"),
            ("SettingInterface", "FFmpeg source"),
            ("SettingInterface", "Hong Kong & Taiwan"),
            ("SettingInterface", "JPG"),
            ("SettingInterface", "JSON"),
            ("SettingInterface", "MKV"),
            ("SettingInterface", "MP4"),
            ("SettingInterface", "Mainland China"),
            ("SettingInterface", "Mica effect (Windows 11 only)"),
            ("SettingInterface", "NFO"),
            ("SettingInterface", "No proxy"),
            ("SettingInterface", "Other"),
            ("SettingInterface", "Overseas"),
            ("SettingInterface", "PNG"),
            ("SettingInterface", "Parsing"),
            ("SettingInterface", "Priority"),
            ("SettingInterface", "Protocol"),
            ("SettingInterface", "Proxy"),
            ("SettingInterface", "Proxy mode"),
            ("SettingInterface", "Proxy server"),
            ("SettingInterface", "SRT"),
            ("SettingInterface", "System proxy"),
            ("SettingInterface", "Update"),
            ("SettingInterface", "User agent"),
            ("SettingInterface", "WebP"),
            ("SettingInterface", "XML"),
            ("SubtitleSettingCard", "Subtitle Format"),
            ("TermsOfUseDialog", "Terms of Use"),
            ("TopNavigationDialogBase", "OK"),
            ("UpdateDialog", "Exit"),
            ("WindowBehaviorSettingCard", "Always ask"),
            ("WindowBehaviorSettingCard", "Close the Main Window"),
            ("WindowBehaviorSettingCard", "Exit the program"),
            ("WindowBehaviorSettingCard", "Minimize to system tray"),
            ("WindowBehaviorSettingCard", "Remember Window State"),
            ("WindowBehaviorSettingCard", "Silent Start"),
            ("WindowBehaviorSettingCard", "Stay on Top"),
            ("WindowBehaviorSettingCard", "Window Behavior"),
        ] {
            assert!(c.lookup(ctx, source).is_some(), "missing {ctx}/{source}");
        }
    }

    /// The shell language flip resolves a different catalog: zh-CN turns
    /// an English source into a Chinese string, English echoes the
    /// source.
    #[test]
    fn shell_language_flip_resolves_zh_catalog() {
        let zh = services::i18n::catalog(services::i18n::ResolvedLanguage::ZhCn);
        let zh_hit = zh.tr("ParseInterface", "Paste a Bilibili link");
        assert_ne!(zh_hit, "Paste a Bilibili link", "zh must translate");

        let en = services::i18n::catalog(services::i18n::ResolvedLanguage::En);
        assert_eq!(
            en.tr("ParseInterface", "Paste a Bilibili link"),
            "Paste a Bilibili link"
        );
        // Same flip through the same global the shell render path reads.
        assert_ne!(zh.tr("MainWindow", "Parser"), en.tr("MainWindow", "Parser"));
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
