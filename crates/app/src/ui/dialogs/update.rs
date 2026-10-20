//! Update check against the Verhub API + the update dialog
//! (ports of `src/util/misc/update.py` and `src/gui/dialog/update.py`).
//!
//! The client POSTs the live `check-update` endpoint with the app's
//! current/comparable versions, prerelease flag and locale; the gate
//! honors `Application/skip_version` unless the check was manual.

use serde_json::Value;

use gpui_kit::component::button::Button;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::label::Label;
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, ParentElement, Window};

use super::{active_catalog, format_tr};

pub use services::update::{to_verhub_locale, UpdateInfo, VERHUB_BASE_URL, VERHUB_PROJECT_KEY};

/// App version shown in the dialog tip (workspace version line).
pub fn app_version() -> &'static str {
    services::update::APP_VERSION
}

/// `Updater.check`: decide what a finished check does.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateAction {
    /// Show the update dialog with this info.
    Show(UpdateInfo),
    /// Skipped: `should_update` but `skip_version` matches and not manual.
    Skip,
    /// Nothing new and a manual check: show the "already latest" toast.
    AlreadyLatest,
    /// Nothing new and automatic: silent.
    Silent,
}

/// The gate (`Updater.check(info, manual)`), delegating the
/// skip-version comparison to `services::update::suppresses_prompt`.
pub fn decide(info: &UpdateInfo, cfg: &config::ConfigStore, manual: bool) -> UpdateAction {
    let skipped = cfg
        .get("Application", "skip_version")
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
        .to_owned();
    if services::update::suppresses_prompt(info, &skipped, manual) {
        UpdateAction::Skip
    } else if info.should_update {
        UpdateAction::Show(info.clone())
    } else if manual {
        UpdateAction::AlreadyLatest
    } else {
        UpdateAction::Silent
    }
}

/// One check against Verhub (`check_for_update`), via the shared
/// `services::update` client. `base` is for tests (wiremock origin);
/// production passes `VERHUB_BASE_URL`.
pub async fn check_for_update(
    client: &network::HttpClient,
    base: &str,
    include_preview: bool,
    locale: Option<&str>,
) -> Result<UpdateInfo, services::update::UpdateError> {
    let params = services::update::CheckUpdateParams {
        include_preview,
        locale: locale.map(str::to_owned),
        ..Default::default()
    };
    services::update::VerhubClient::with_options(client, base, VERHUB_PROJECT_KEY, app_version())
        .check_update(&params)
        .await
        .map(|outcome| outcome.info)
}

/// Assemble the params from a config store (`request_update`).
pub fn check_params(cfg: &config::ConfigStore) -> (bool, Option<&'static str>) {
    let include_preview = cfg
        .get("Update", "include_prerelease")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let lang = cfg
        .get("Interface", "language")
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "Auto".into());
    (include_preview, to_verhub_locale(Some(&lang)))
}

/// The dialog's version tip line
/// (`"Version {latest_version} is available ..."`).
pub fn version_tip(c: &services::i18n::Catalog, info: &UpdateInfo) -> String {
    format_tr(
        c.tr(
            "UpdateDialog",
            "Version {latest_version} is available for download. You are currently using version {current_version}. Would you like to update now?",
        ),
        &[
            ("latest_version", &info.version),
            ("current_version", app_version()),
        ],
    )
}

/// Write the skip-version (`UpdateDialog.on_skip`).
pub fn skip_version(cfg: &mut config::ConfigStore, version: &str) {
    cfg.set(
        "Application",
        "skip_version",
        Value::String(version.to_owned()),
    );
}

/// The startup check (Python `request_update(manual=False)`): fetch,
/// gate, and open the dialog when an update is offered. Network
/// failures are silent (the Python `except` swallows them).
pub fn run_startup_check(window: &mut Window, cx: &mut gpui_kit::Context<crate::ui::Shell>) {
    // The dialog needs a live `Window` handle; the check is async, so
    // park the result in a shared slot and open the dialog from the
    // window's deferred queue on the following frame.
    let slot = std::rc::Rc::new(std::cell::RefCell::<Option<UpdateInfo>>::new(None));
    let tail = slot.clone();
    cx.spawn(async move |shell, cx| {
        let cfg =
            config::ConfigStore::open().unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
        let (include_preview, locale) = check_params(&cfg);
        let service = super::super::parse::dispatch::ParseService::new(&cfg);
        let fetched = crate::bootstrap::on_runtime(move || async move {
            check_for_update(&service.client, VERHUB_BASE_URL, include_preview, locale).await
        })
        .await
        .ok()
        .and_then(|r| r.ok());
        let Some(fetched) = fetched else { return };
        if let UpdateAction::Show(found) = decide(&fetched, &cfg, false) {
            *tail.borrow_mut() = Some(found);
            let _ = shell.update(cx, |_, cx| cx.notify());
        }
    })
    .detach();
    window.defer(cx, move |window, cx| {
        if let Some(info) = slot.borrow().clone() {
            let cfg = config::ConfigStore::open()
                .unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
            open_update_dialog(&info, cfg, window, cx);
        }
    });
}

/// Open the update dialog (port of `UpdateDialog`). Skip writes
/// `Application/skip_version`; "Update now" opens the download url;
/// forced updates offer Exit (the app is left open — closing the
/// window is the shell's business).
pub fn open_update_dialog(
    info: &UpdateInfo,
    cfg: config::ConfigStore,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    const CTX: &str = "UpdateDialog";
    let catalog = active_catalog(&cfg);
    let title = catalog.tr(CTX, "Update Available").to_owned();
    let subtitle = catalog.tr(CTX, "A new version is available").to_owned();
    let tip = version_tip(catalog, info);
    let whats_new = catalog.tr(CTX, "What's new:").to_owned();
    let skip = catalog.tr(CTX, "Skip this version").to_owned();
    let update_now = catalog.tr(CTX, "Update now").to_owned();
    let exit = catalog.tr(CTX, "Exit").to_owned();
    let required = info.required;
    let info = info.clone();

    window.open_dialog(cx, move |dialog, _, _| {
        let info = info.clone();
        dialog
            .title(title.clone())
            .w(gpui_kit::px(560.))
            .content({
                let subtitle = subtitle.clone();
                let tip = tip.clone();
                let whats_new = whats_new.clone();
                let content_line = info.content.clone();
                move |content, _, _| {
                    content.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(Label::new(subtitle.clone()))
                            .child(Label::new(tip.clone()))
                            .child(Label::new(whats_new.clone()))
                            .child(Label::new(content_line.clone())),
                    )
                }
            })
            .footer({
                let info = info.clone();
                DialogFooter::new()
                    .when(!required, |footer| {
                        footer.child(Button::new("skip").label(skip.clone()).on_click({
                            let version = info.version.clone();
                            move |_, window, cx| {
                                let mut cfg = config::ConfigStore::open()
                                    .unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
                                skip_version(&mut cfg, &version);
                                window.close_dialog(cx);
                            }
                        }))
                    })
                    .child(
                        Button::new("update-now")
                            .label(update_now.clone())
                            .on_click({
                                let url = info.update_url.clone();
                                move |_, window, cx| {
                                    window.close_dialog(cx);
                                    cx.open_url(&url);
                                }
                            }),
                    )
                    .when(required, |footer| {
                        footer.child(Button::new("exit").label(exit.clone()).on_click(
                            |_, window, cx| {
                                window.close_dialog(cx);
                            },
                        ))
                    })
            })
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use services::i18n::{catalog, tip_messages, ResolvedLanguage};
    use services::update::parse_response;

    // ---- to_verhub_locale (the Python matrix) ----

    #[test]
    fn locale_none_for_auto_and_empty() {
        assert_eq!(to_verhub_locale(None), None);
        assert_eq!(to_verhub_locale(Some("")), None);
        assert_eq!(to_verhub_locale(Some("auto")), None);
        assert_eq!(to_verhub_locale(Some("  ")), None);
    }

    #[test]
    fn locale_zh_variants() {
        assert_eq!(to_verhub_locale(Some("zh_CN")), Some("zh-CN"));
        assert_eq!(to_verhub_locale(Some("zh_TW")), Some("zh-TW"));
        assert_eq!(to_verhub_locale(Some("zh_HK")), Some("zh-TW"));
        assert_eq!(to_verhub_locale(Some("zh-Hant-TW")), Some("zh-TW"));
        assert_eq!(to_verhub_locale(Some("zh-MO")), Some("zh-TW"));
        assert_eq!(to_verhub_locale(Some("zh-SG")), Some("zh-CN"));
    }

    #[test]
    fn locale_en_fallback() {
        assert_eq!(to_verhub_locale(Some("en")), Some("en-US"));
        assert_eq!(to_verhub_locale(Some("en_US")), Some("en-US"));
        assert_eq!(to_verhub_locale(Some("ja")), Some("en-US"));
    }

    // ---- parse_response (live-verified Verhub shape) ----

    #[test]
    fn parse_response_flattens_the_live_shape() {
        let raw = serde_json::json!({
            "should_update": false,
            "required": false,
            "reason_codes": ["CURRENT_IS_LATEST"],
            "current_version": "2.20.0",
            "current_comparable_version": "2.20.0",
            "latest_version": {
                "id": 210,
                "version": "2.20.0",
                "comparable_version": "2.20.0",
                "title": "2.20.0",
                "content": "release notes",
                "download_url": "https://github.com/stanzhai/bilidown/releases",
                "forced": false,
                "is_preview": false
            },
            "latest_preview_version": null,
            "target_version": null,
            "milestone": null
        });
        let info = parse_response(&raw);
        assert_eq!(info.version, "2.20.0");
        assert_eq!(info.content, "release notes");
        assert_eq!(
            info.update_url,
            "https://github.com/stanzhai/bilidown/releases"
        );
        assert!(!info.should_update);
        assert!(!info.required);
    }

    #[test]
    fn parse_response_defaults_missing_fields() {
        let info = parse_response(&serde_json::json!({"should_update": true}));
        assert!(info.should_update);
        assert_eq!(info.version, "");
        assert_eq!(info.content, "");
        assert_eq!(info.update_url, "");
    }

    // ---- gate matrix ----

    fn info(should_update: bool, version: &str) -> UpdateInfo {
        UpdateInfo {
            should_update,
            required: false,
            version: version.to_owned(),
            title: None,
            content: String::new(),
            update_url: String::new(),
        }
    }

    #[test]
    fn gate_shows_new_version() {
        let cfg = config::ConfigStore::with_paths("", "");
        assert_eq!(
            decide(&info(true, "9.9.9"), &cfg, false),
            UpdateAction::Show(info(true, "9.9.9"))
        );
    }

    #[test]
    fn gate_honors_skip_version_for_auto_checks() {
        let mut cfg = config::ConfigStore::with_paths("", "");
        skip_version(&mut cfg, "9.9.9");
        assert_eq!(
            decide(&info(true, "9.9.9"), &cfg, false),
            UpdateAction::Skip
        );
        // manual overrides the skip
        assert!(decide(&info(true, "9.9.9"), &cfg, true).is_show());
    }

    #[test]
    fn gate_silent_when_current() {
        let cfg = config::ConfigStore::with_paths("", "");
        assert_eq!(
            decide(&info(false, "2.21.0"), &cfg, false),
            UpdateAction::Silent
        );
        assert_eq!(
            decide(&info(false, "2.21.0"), &cfg, true),
            UpdateAction::AlreadyLatest
        );
    }

    impl UpdateAction {
        fn is_show(&self) -> bool {
            matches!(self, UpdateAction::Show(_))
        }
    }

    #[test]
    fn check_params_read_config() {
        let mut cfg = config::ConfigStore::with_paths("", "");
        assert_eq!(check_params(&cfg), (false, None));
        cfg.set("Update", "include_prerelease", Value::Bool(true));
        cfg.set("Interface", "language", Value::String("zh_TW".to_owned()));
        assert_eq!(check_params(&cfg), (true, Some("zh-TW")));
    }

    #[test]
    fn version_tip_formats_both_versions() {
        let c = catalog(ResolvedLanguage::En);
        let tip = version_tip(c, &info(true, "9.9.9"));
        assert!(tip.contains("9.9.9"));
        assert!(tip.contains(app_version()));
    }

    #[test]
    fn already_latest_tip_exists() {
        let c = catalog(ResolvedLanguage::En);
        assert_eq!(
            tip_messages(c, "ALREADY_LATEST_VERSION").as_deref(),
            Some("You are already using the latest version")
        );
    }

    // ---- wiremock: the request the client actually sends ----

    #[tokio::test]
    async fn check_via_services_client_parses_the_response() {
        // Wire contract (path, dropped-unset body) is pinned by
        // services/tests/update.rs; this only checks the delegation
        // glue still hands the parsed info to the dialog side.
        use wiremock::matchers::{body_partial_json, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = serde_json::json!({
            "current_version": services::update::APP_VERSION,
        });
        Mock::given(method("POST"))
            .and(path(format!(
                "/api/v1/public/{VERHUB_PROJECT_KEY}/versions/check-update"
            )))
            .and(body_partial_json(body))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "should_update": true,
                "required": true,
                "latest_version": {
                    "version": "9.9.9",
                    "content": "notes",
                    "download_url": "https://example.com/dl"
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = network::HttpClient::new(network::ClientOptions {
            user_agent: "test-ua".to_owned(),
            proxy: network::ProxyConfig::none(),
            cookies_from_persistence: Vec::new(),
            signer: None,
        })
        .unwrap();

        let info = check_for_update(&client, &server.uri(), false, None)
            .await
            .unwrap();
        assert!(info.should_update);
        assert!(info.required);
        assert_eq!(info.version, "9.9.9");
        assert_eq!(info.update_url, "https://example.com/dl");
    }
}
