//! Settings screen (port of `src/gui/interface/setting.py`): seven pages
//! built on the gpui-component setting library, write-through config, and
//! the naming-rules window (`RuleListDialog` + `EditRuleDialog`).
//!
//! Layout mirrors the Python interface: one `SettingPage` per Python
//! settings group, one `SettingGroup` per card, card rows become
//! `SettingItem`s. Every change is written to the config store and saved
//! atomically immediately — no OK button.
//!
//! The setting library's field closures receive only `&mut App`, so all
//! mutable state lives in an [`Entity<SettingsUi>`] the closures capture.
//! Writes call `App::notify` on that entity and the shell observes it, so
//! the window re-renders. Pages rebuild every frame from fresh reads.

use crate::ui::dialogs::update::UpdateInfo;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::label::Label;
use gpui_kit::component::list::ListItem;
use gpui_kit::component::setting::{
    NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::theme::ActiveTheme;
use gpui_kit::{
    div, prelude::*, px, AnyElement, App, AppContext, ClipboardItem, Context, Entity, EntityId,
    InteractiveElement, IntoElement, ParentElement, PathPromptOptions, SharedString, Styled,
    Window,
};
use naming::conventions::{ConventionType, VariableListFactory};
use naming::naming_rules::RuleEntry;
use serde_json::Value;

use super::i18n_bridge::Lang;
use super::model::{self, EditorField, RuleListModel};
use crate::ui::Shell;

/// All mutable UI state the setting-library closures must reach through
/// `&mut App`.
pub struct SettingsUi {
    pub cfg: config::ConfigStore,
    pub lang: Lang,
    pub media: model::MediaOptions,
    /// Runtime-only global sequential starting number (not config).
    pub numbering: model::NumberingModel,
    /// Naming-rules window state.
    pub rules_open: bool,
    pub rules: RuleListModel,
    pub rule_name: Entity<InputState>,
    pub rule_body: Entity<InputState>,
    pub rules_search: Entity<InputState>,
    /// Editor error line `(field, message)`; cleared on next success.
    pub editor_error: Option<(EditorField, String)>,
    /// Toast line (save confirmations, blocked deletes, ...).
    pub notice: Option<String>,
    /// Dialog seams for sibling tickets (21/25): recorded, not opened.
    pub pending_dialog: Option<&'static str>,
}

impl SettingsUi {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let cfg =
            config::ConfigStore::open().unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
        let lang = Lang::from_config(&model::get_str(&cfg, "Interface", "language"));
        let rules = RuleListModel::new(&cfg);
        let initial = rules.editor.clone();
        let rule_name =
            cx.new(|cx| InputState::new(window, cx).default_value(initial.name_text.clone()));
        let rule_body =
            cx.new(|cx| InputState::new(window, cx).default_value(initial.rule_text.clone()));
        let rules_search = cx.new(|cx| InputState::new(window, cx));
        Self {
            media: model::MediaOptions::load(&cfg),
            numbering: model::NumberingModel::default(),
            rules_open: false,
            rules,
            rule_name,
            rule_body,
            rules_search,
            editor_error: None,
            notice: None,
            pending_dialog: None,
            lang,
            cfg,
        }
    }

    /// Write-through: set + atomic save. Save errors surface as a notice.
    fn put(&mut self, group: &str, item: &str, value: Value) {
        if let Err(e) = model::set_and_save(&mut self.cfg, group, item, value) {
            self.notice = Some(crate::ui::dialogs::format_tr(
                self.tr("SettingInterface", "Save failed: {error}").as_str(),
                &[("error", &e.to_string())],
            ));
        }
    }

    fn tr(&self, ctx: &str, source: &str) -> String {
        self.lang.tr(ctx, source).to_string()
    }

    /// Restart-required notice. `display_scaling` is the only setting that
    /// still needs a restart (theme and language apply instantly).
    fn restart_notice(&mut self) {
        self.notice = Some(self.tr(
            "SettingInterface",
            "Configuration takes effect after restart",
        ));
    }
}

/// The screen: a handle to the state entity, plain field on `Shell`
/// (mirrors `ParseScreen`). `Shell::new` creates it and observes it.
pub struct SettingsScreen {
    pub ui: Entity<SettingsUi>,
}

impl SettingsScreen {
    pub fn new(window: &mut Window, cx: &mut Context<Shell>) -> Self {
        let ui = cx.new(|cx| SettingsUi::new(window, cx));
        Self { ui }
    }

    pub fn render(&mut self, window: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
        let ui = self.ui.clone();
        let notice = ui.read(cx).notice.clone();
        let rules_open = ui.read(cx).rules_open;
        // Configure… rows only record a seam; open the real dialog here,
        // where both window and app context are live.
        let pending = ui.update(cx, |ui, _| ui.pending_dialog.take());
        if let Some(seam) = pending {
            let cfg = config::ConfigStore::open()
                .unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
            match seam {
                "danmaku_style" => crate::ui::dialogs::styles::open_danmaku_style(cfg, window, cx),
                "subtitle_style" => {
                    crate::ui::dialogs::styles::open_subtitle_style(cfg, window, cx)
                }
                "check_update" => {
                    // Python's "check for updates" row runs the check and
                    // only opens the dialog when an update exists; the
                    // dialog needs a live window, so open it on the next
                    // frame from the shell update.
                    let slot =
                        std::rc::Rc::new(std::cell::RefCell::<Option<UpdateInfo>>::new(None));
                    let tail = slot.clone();
                    let c = config::ConfigStore::open()
                        .unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
                    cx.spawn(async move |shell, cx| {
                        let (include_preview, locale) =
                            crate::ui::dialogs::update::check_params(&c);
                        let service = crate::ui::parse::dispatch::ParseService::new(&c);
                        let fetched = crate::bootstrap::on_runtime(move || async move {
                            crate::ui::dialogs::update::check_for_update(
                                &service.client,
                                crate::ui::dialogs::update::VERHUB_BASE_URL,
                                include_preview,
                                locale,
                            )
                            .await
                        })
                        .await
                        .ok()
                        .and_then(|r| r.ok());
                        let Some(fetched) = fetched else { return };
                        if let crate::ui::dialogs::update::UpdateAction::Show(found) =
                            crate::ui::dialogs::update::decide(&fetched, &c, true)
                        {
                            *tail.borrow_mut() = Some(found);
                            let _ = shell.update(cx, |_, cx| cx.notify());
                        }
                    })
                    .detach();
                    window.defer(cx, move |window, cx| {
                        if let Some(info) = slot.borrow().clone() {
                            crate::ui::dialogs::update::open_update_dialog(&info, cfg, window, cx);
                        }
                    });
                }
                "subtitle_language" => {
                    crate::ui::dialogs::subtitles_language::open_subtitles_language(cfg, window, cx)
                }
                "cn_cdn_list" => crate::ui::dialogs::edit_host::open_cdn_server_list(
                    cfg,
                    crate::ui::dialogs::edit_host::CdnListKey::Cn,
                    window,
                    cx,
                ),
                "ov_cdn_list" => crate::ui::dialogs::edit_host::open_cdn_server_list(
                    cfg,
                    crate::ui::dialogs::edit_host::CdnListKey::Ov,
                    window,
                    cx,
                ),
                "auto_select" => crate::ui::dialogs::auto_select::open_auto_select(cfg, window, cx),
                "video_priority" => crate::ui::dialogs::priority::open_priority(
                    cfg,
                    crate::ui::dialogs::priority::PriorityKind::VideoQuality,
                    window,
                    cx,
                ),
                "audio_priority" => crate::ui::dialogs::priority::open_priority(
                    cfg,
                    crate::ui::dialogs::priority::PriorityKind::AudioQuality,
                    window,
                    cx,
                ),
                "codec_priority" => crate::ui::dialogs::priority::open_priority(
                    cfg,
                    crate::ui::dialogs::priority::PriorityKind::VideoCodec,
                    window,
                    cx,
                ),
                "parse_list" => crate::ui::dialogs::parse_list::open_parse_list(cfg, window, cx),
                "monitor_clipboard" => {
                    crate::ui::dialogs::monitor_clipboard::open_monitor_clipboard(cfg, window, cx)
                }
                _ => {}
            }
        }
        div()
            .id("settings-screen")
            .size_full()
            .when_some(notice, |this, notice| {
                this.child(
                    div()
                        .id("settings-notice")
                        .px_4()
                        .py_2()
                        .child(Label::new(notice)),
                )
            })
            .child(render_pages(&ui, window, cx))
            .when(rules_open, |this| {
                this.child(render_rules_window(&ui, window, cx))
            })
            .into_any_element()
    }
}

// ---------- shared field helpers ----------

/// Notify the settings entity (its observer chain redraws the shell).
fn kick(cx: &mut App, id: EntityId) {
    cx.notify(id);
}

/// Apply a stored `QFluentWidgets.ThemeMode` string to the live theme:
/// Light/Dark load that mode, Auto re-resolves against the OS appearance.
/// Field closures only get `&mut App`, so `None` for the window argument.
fn apply_theme_now(v: &str, cx: &mut App) {
    use gpui_kit::component::theme::{Theme, ThemeMode};
    match services::theme::ThemeModeSetting::from_config(v) {
        services::theme::ThemeModeSetting::Light => Theme::change(ThemeMode::Light, None, cx),
        services::theme::ThemeModeSetting::Dark => Theme::change(ThemeMode::Dark, None, cx),
        services::theme::ThemeModeSetting::Auto => Theme::sync_system_appearance(None, cx),
    }
}

/// Instant-apply for `Interface.language`: write-through plus two
/// coordinated flips — the settings entity's resolved `Lang` (this screen
/// re-renders from it) and the app-global `ShellI18n` (shell/login render
/// read it every frame).
fn apply_language(ui: &mut SettingsUi, cx: &mut App, value: &str) {
    ui.put("Interface", "language", Value::from(value));
    ui.lang = Lang::from_config(value);
    let lang = services::i18n::Language::from_config(value);
    cx.set_global(crate::ui::ShellI18n { lang });
    crate::ui::download_options::set_shell_lang(lang);
}

/// Switch bound to a bool config key (write-through).
fn cfg_switch(
    ui: &Entity<SettingsUi>,
    group: &'static str,
    item: &'static str,
) -> SettingField<bool> {
    let get = ui.clone();
    let set = ui.clone();
    let id = ui.entity_id();
    SettingField::switch(
        move |cx| model::get_bool(&get.read(cx).cfg, group, item),
        move |v, cx| {
            set.update(cx, |ui, _| ui.put(group, item, Value::from(v)));
            kick(cx, id);
        },
    )
}

/// Dropdown over `(value, label)` pairs; the value string is stored.
fn cfg_dropdown(
    ui: &Entity<SettingsUi>,
    group: &'static str,
    item: &'static str,
    options: Vec<(SharedString, SharedString)>,
) -> SettingField<SharedString> {
    let get = ui.clone();
    let set = ui.clone();
    let id = ui.entity_id();
    SettingField::dropdown(
        options,
        move |cx| model::get_str(&get.read(cx).cfg, group, item).into(),
        move |v, cx| {
            set.update(cx, |ui, _| ui.put(group, item, Value::from(v.to_string())));
            kick(cx, id);
        },
    )
}

/// Integer-valued dropdown; the numeric value is stored.
fn cfg_int_dropdown(
    ui: &Entity<SettingsUi>,
    group: &'static str,
    item: &'static str,
    options: Vec<(i64, SharedString)>,
    fallback: i64,
) -> SettingField<SharedString> {
    let opts: Vec<(SharedString, SharedString)> = options
        .into_iter()
        .map(|(v, l)| (SharedString::from(v.to_string()), l))
        .collect();
    let get = ui.clone();
    let set = ui.clone();
    let id = ui.entity_id();
    SettingField::dropdown(
        opts,
        move |cx| {
            SharedString::from(model::get_int(&get.read(cx).cfg, group, item, fallback).to_string())
        },
        move |v, cx| {
            let parsed = v.to_string().parse::<i64>().unwrap_or(fallback);
            set.update(cx, |ui, _| ui.put(group, item, Value::from(parsed)));
            kick(cx, id);
        },
    )
}

/// Number input bound to an int config key, clamped to `[min, max]`.
fn cfg_number(
    ui: &Entity<SettingsUi>,
    group: &'static str,
    item: &'static str,
    min: f64,
    max: f64,
    step: f64,
    fallback: i64,
) -> SettingField<f64> {
    let get = ui.clone();
    let set = ui.clone();
    let id = ui.entity_id();
    SettingField::number_input(
        NumberFieldOptions { min, max, step },
        move |cx| model::get_int(&get.read(cx).cfg, group, item, fallback) as f64,
        move |v, cx| {
            set.update(cx, |ui, _| {
                ui.put(group, item, Value::from(v.round() as i64))
            });
            kick(cx, id);
        },
    )
}

/// Text input bound to a string config key.
fn cfg_input(
    ui: &Entity<SettingsUi>,
    group: &'static str,
    item: &'static str,
) -> SettingField<SharedString> {
    let get = ui.clone();
    let set = ui.clone();
    let id = ui.entity_id();
    SettingField::input(
        move |cx| model::get_str(&get.read(cx).cfg, group, item).into(),
        move |v, cx| {
            set.update(cx, |ui, _| ui.put(group, item, Value::from(v.to_string())));
            kick(cx, id);
        },
    )
}

/// `Configure…` row: a button recording a dialog seam owned by a sibling
/// ticket (21/25).
fn configure_button(ui: &Entity<SettingsUi>, seam: &'static str) -> SettingItem {
    let h = ui.clone();
    let id = ui.entity_id();
    SettingItem::render(move |_, _, cx| {
        let text = h.update(cx, |ui, _| ui.tr("CDNSettingCard", "Configure…"));
        let hh = h.clone();
        Button::new(SharedString::from(format!("cfg-{seam}")))
            .label(text)
            .outline()
            .on_click(move |_, _, cx| {
                hh.update(cx, |ui, _| ui.pending_dialog = Some(seam));
                kick(cx, id);
            })
            .into_any_element()
    })
}

// ---------- Interface page ----------

fn interface_page(ui: &Entity<SettingsUi>, lang: Lang) -> SettingPage {
    let t = move |s: &str| SharedString::from(lang.tr("SettingInterface", s).to_string());

    let theme = {
        let get = ui.clone();
        let set = ui.clone();
        let id = ui.entity_id();
        SettingField::dropdown(
            vec![
                (
                    SharedString::from("Light"),
                    lang.tr("PersonalizationCard", "Light").to_string().into(),
                ),
                (
                    SharedString::from("Dark"),
                    lang.tr("PersonalizationCard", "Dark").to_string().into(),
                ),
                (
                    SharedString::from("Auto"),
                    lang.tr("PersonalizationCard", "System default")
                        .to_string()
                        .into(),
                ),
            ],
            move |cx| model::get_str(&get.read(cx).cfg, "QFluentWidgets", "ThemeMode").into(),
            move |v, cx| {
                set.update(cx, |ui, _| {
                    ui.put("QFluentWidgets", "ThemeMode", Value::from(v.to_string()));
                });
                // Instant apply: load the mode now (Auto re-resolves
                // against the OS appearance), no restart notice.
                apply_theme_now(&v, cx);
                kick(cx, id);
            },
        )
    };

    let mica = SettingItem::new(
        t("Mica effect (Windows 11 only)"),
        cfg_switch(ui, "Interface", "mica_effect"),
    )
    .disabled(cfg!(windows));

    let scaling = {
        let get = ui.clone();
        let set = ui.clone();
        let id = ui.entity_id();
        let pct = |p: &str| SharedString::from(format!("{p}%"));
        SettingField::dropdown(
            vec![
                (SharedString::from("1"), pct("100")),
                (SharedString::from("1.25"), pct("125")),
                (SharedString::from("1.5"), pct("150")),
                (SharedString::from("1.75"), pct("175")),
                (SharedString::from("2"), pct("200")),
                (
                    SharedString::from("Auto"),
                    lang.tr("PersonalizationCard", "System default")
                        .to_string()
                        .into(),
                ),
            ],
            move |cx| model::get_str(&get.read(cx).cfg, "Interface", "display_scaling").into(),
            move |v, cx| {
                set.update(cx, |ui, _| {
                    ui.put("Interface", "display_scaling", Value::from(v.to_string()));
                    ui.restart_notice();
                });
                kick(cx, id);
            },
        )
    };

    let language = {
        let get = ui.clone();
        let set = ui.clone();
        let id = ui.entity_id();
        SettingField::dropdown(
            vec![
                (SharedString::from("zh_CN"), SharedString::from("简体中文")),
                (SharedString::from("zh_TW"), SharedString::from("繁體中文")),
                (SharedString::from("en_US"), SharedString::from("English")),
                (
                    SharedString::from("Auto"),
                    lang.tr("PersonalizationCard", "System default")
                        .to_string()
                        .into(),
                ),
            ],
            move |cx| model::get_str(&get.read(cx).cfg, "Interface", "language").into(),
            move |v, cx| {
                set.update(cx, |ui, cx| apply_language(ui, cx, &v));
                kick(cx, id);
            },
        )
    };

    SettingPage::new(t("Interface"))
        .icon(IconName::Palette)
        .group(
            SettingGroup::new()
                .title(
                    lang.tr("PersonalizationCard", "Personalization")
                        .to_string(),
                )
                .item(SettingItem::new(
                    lang.tr("PersonalizationCard", "Theme").to_string(),
                    theme,
                ))
                .item(mica)
                .item(SettingItem::new(t("Display scaling"), scaling))
                .item(SettingItem::new(t("Language"), language)),
        )
}

// ---------- Behavior page ----------

fn behavior_page(ui: &Entity<SettingsUi>, lang: Lang) -> SettingPage {
    let t = move |s: &str| SharedString::from(lang.tr("SettingInterface", s).to_string());

    SettingPage::new(t("Behavior"))
        .icon(IconName::Wand)
        .group(
            SettingGroup::new().title(t("Parsing")).items([
                configure_button(ui, "parse_list"),
                configure_button(ui, "monitor_clipboard"),
                configure_button(ui, "auto_select"),
                SettingItem::new(
                    lang.tr("ParsingSettingCard", "Save Parse History")
                        .to_string(),
                    cfg_switch(ui, "Behavior", "parse_history"),
                ),
            ]),
        )
        .group(
            SettingGroup::new()
                .title(
                    lang.tr("WindowBehaviorSettingCard", "Window Behavior")
                        .to_string(),
                )
                .item(SettingItem::new(
                    lang.tr("WindowBehaviorSettingCard", "Silent Start")
                        .to_string(),
                    cfg_switch(ui, "Behavior", "silent_start"),
                ))
                .item(SettingItem::new(
                    lang.tr("WindowBehaviorSettingCard", "Remember Window State")
                        .to_string(),
                    cfg_switch(ui, "Behavior", "remember_window_state"),
                ))
                .item(SettingItem::new(
                    lang.tr("WindowBehaviorSettingCard", "Stay on Top")
                        .to_string(),
                    cfg_switch(ui, "Behavior", "stay_on_top"),
                )),
        )
        .group(
            SettingGroup::new()
                .title(
                    lang.tr("DownloadHandlingSettingCard", "Download Handling")
                        .to_string(),
                )
                .item(SettingItem::new(
                    lang.tr(
                        "DownloadHandlingSettingCard",
                        "Show Download Options Dialog",
                    )
                    .to_string(),
                    cfg_switch(ui, "Behavior", "show_download_options_dialog"),
                ))
                .item(SettingItem::new(
                    lang.tr("DownloadHandlingSettingCard", "Show Notifications")
                        .to_string(),
                    cfg_switch(ui, "Behavior", "show_notification"),
                ))
                .item(SettingItem::new(
                    lang.tr(
                        "DownloadHandlingSettingCard",
                        "Retry Failed Downloads Automatically",
                    )
                    .to_string(),
                    cfg_switch(ui, "Behavior", "auto_retry_enabled"),
                ))
                .item(SettingItem::new(
                    lang.tr("DownloadHandlingSettingCard", "Maximum Retry Attempts")
                        .to_string(),
                    cfg_number(ui, "Behavior", "auto_retry_max_count", 1., 20., 1., 3),
                ))
                .item(SettingItem::new(
                    lang.tr(
                        "DownloadHandlingSettingCard",
                        "Duplicate Download Resolution",
                    )
                    .to_string(),
                    cfg_int_dropdown(
                        ui,
                        "Behavior",
                        "duplicate_download_resolution",
                        vec![
                            (
                                0,
                                lang.tr("DownloadHandlingSettingCard", "Continue")
                                    .to_string()
                                    .into(),
                            ),
                            (
                                1,
                                lang.tr("DownloadHandlingSettingCard", "Skip")
                                    .to_string()
                                    .into(),
                            ),
                            (
                                2,
                                lang.tr("WindowBehaviorSettingCard", "Always ask")
                                    .to_string()
                                    .into(),
                            ),
                        ],
                        2,
                    ),
                ))
                .item(SettingItem::new(
                    lang.tr("DownloadHandlingSettingCard", "File Conflict Resolution")
                        .to_string(),
                    cfg_int_dropdown(
                        ui,
                        "Behavior",
                        "file_conflict_resolution",
                        vec![
                            (
                                1,
                                lang.tr("DownloadHandlingSettingCard", "Auto-rename")
                                    .to_string()
                                    .into(),
                            ),
                            (
                                2,
                                lang.tr("DownloadHandlingSettingCard", "Overwrite")
                                    .to_string()
                                    .into(),
                            ),
                        ],
                        1,
                    ),
                ))
                .item(SettingItem::new(
                    lang.tr("DownloadHandlingSettingCard", "Preallocate File Space")
                        .to_string(),
                    cfg_switch(ui, "Behavior", "preallocate_file_space"),
                )),
        )
}

// ---------- Download page ----------

fn download_page(ui: &Entity<SettingsUi>, lang: Lang, cx: &App) -> SettingPage {
    let t = move |s: &str| SharedString::from(lang.tr("SettingInterface", s).to_string());

    // Download path row: current path and a Change… button opening the
    // native directory picker.
    let path_row = {
        let h = ui.clone();
        let id = ui.entity_id();
        SettingItem::render(move |_, _, cx| {
            let path = model::get_str(&h.read(cx).cfg, "Download", "download_path");
            let change = h.update(cx, |ui, _| ui.tr("SettingInterface", "Change…"));
            let hh = h.clone();
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(Label::new(path))
                .child(
                    Button::new("change-download-path")
                        .label(change)
                        .outline()
                        .on_click(move |_, _, cx| {
                            let rx = cx.prompt_for_paths(PathPromptOptions {
                                files: false,
                                directories: true,
                                multiple: false,
                                prompt: None,
                            });
                            let hhh = hh.clone();
                            cx.spawn(async move |cx| {
                                if let Ok(Ok(Some(paths))) = rx.await {
                                    if let Some(p) = paths.first().cloned() {
                                        cx.update(|cx: &mut App| {
                                            hhh.update(cx, |ui, _| {
                                                ui.put(
                                                    "Download",
                                                    "download_path",
                                                    Value::from(p.display().to_string()),
                                                );
                                            });
                                            kick(cx, id);
                                        });
                                    }
                                }
                            })
                            .detach();
                        }),
                )
                .into_any_element()
        })
    };

    // Media options cascade: every write flows through the model so the
    // derived states follow the Python card.
    let video_stream = {
        let get = ui.clone();
        let set = ui.clone();
        let id = ui.entity_id();
        SettingField::switch(
            move |cx| get.read(cx).media.download_video_stream,
            move |v, cx| {
                set.update(cx, |ui, _| {
                    let audio = ui.media.download_audio_stream;
                    ui.media.set_stream(v, audio);
                    let _ = ui.media.save(&mut ui.cfg);
                });
                kick(cx, id);
            },
        )
    };
    let audio_stream = {
        let get = ui.clone();
        let set = ui.clone();
        let id = ui.entity_id();
        SettingField::switch(
            move |cx| get.read(cx).media.download_audio_stream,
            move |v, cx| {
                set.update(cx, |ui, _| {
                    let video = ui.media.download_video_stream;
                    ui.media.set_stream(video, v);
                    let _ = ui.media.save(&mut ui.cfg);
                });
                kick(cx, id);
            },
        )
    };
    let merge = {
        let get = ui.clone();
        let set = ui.clone();
        let id = ui.entity_id();
        SettingField::switch(
            move |cx| get.read(cx).media.merge_video_audio,
            move |v, cx| {
                set.update(cx, |ui, _| {
                    ui.media.set_merge(v);
                    let _ = ui.media.save(&mut ui.cfg);
                });
                kick(cx, id);
            },
        )
    };
    let keep = {
        let get = ui.clone();
        let set = ui.clone();
        let id = ui.entity_id();
        SettingField::switch(
            move |cx| get.read(cx).media.keep_original_files,
            move |v, cx| {
                set.update(cx, |ui, _| {
                    ui.media.set_keep(v);
                    let _ = ui.media.save(&mut ui.cfg);
                });
                kick(cx, id);
            },
        )
    };
    let keep_type = {
        let get = ui.clone();
        let set = ui.clone();
        let id = ui.entity_id();
        SettingField::dropdown(
            vec![
                (
                    SharedString::from("0"),
                    lang.tr("MediaOptionsCard", "Video Only").to_string().into(),
                ),
                (
                    SharedString::from("1"),
                    lang.tr("MediaOptionsCard", "Audio Only").to_string().into(),
                ),
                (
                    SharedString::from("2"),
                    lang.tr("MediaOptionsCard", "Both").to_string().into(),
                ),
            ],
            move |cx| SharedString::from(get.read(cx).media.keep_original_files_type.to_string()),
            move |v, cx| {
                let parsed = v.to_string().parse::<i64>().unwrap_or(0);
                set.update(cx, |ui, _| {
                    ui.media.keep_original_files_type = parsed;
                    let _ = ui.media.save(&mut ui.cfg);
                });
                kick(cx, id);
            },
        )
    };

    let media = ui.read(cx).media;

    SettingPage::new(t("Download"))
        .icon(IconName::Download)
        .group(
            SettingGroup::new()
                .title(
                    lang.tr("DownloadPathSettingCard", "Download Path")
                        .to_string(),
                )
                .item(path_row),
        )
        .group(
            SettingGroup::new()
                .title(t("Concurrency"))
                .item(SettingItem::new(
                    lang.tr("DownloadConcurrencySettingCard", "Number of Threads")
                        .to_string(),
                    cfg_number(ui, "Download", "download_thread", 1., 10., 1., 8),
                ))
                .item(SettingItem::new(
                    lang.tr(
                        "DownloadConcurrencySettingCard",
                        "Number of Parallel Downloads",
                    )
                    .to_string(),
                    cfg_number(ui, "Download", "download_parallel", 1., 10., 1., 3),
                ))
                .item(configure_button(ui, "speed_limit")),
        )
        .group(
            SettingGroup::new()
                .title(lang.tr("MediaOptionsCard", "Media Options").to_string())
                .item(SettingItem::new(
                    lang.tr("MediaOptionsCard", "Download standalone video stream")
                        .to_string(),
                    video_stream,
                ))
                .item(SettingItem::new(
                    lang.tr("MediaOptionsCard", "Download standalone audio stream")
                        .to_string(),
                    audio_stream,
                ))
                .item(
                    SettingItem::new(
                        lang.tr("MediaOptionsCard", "Merge video and audio")
                            .to_string(),
                        merge,
                    )
                    .disabled(!media.merge_enabled()),
                )
                .item(
                    SettingItem::new(
                        lang.tr("MediaOptionsCard", "Keep original files")
                            .to_string(),
                        keep,
                    )
                    .disabled(!media.keep_enabled()),
                )
                .item(
                    SettingItem::new(
                        lang.tr("MediaOptionsCard", "Original file type to keep")
                            .to_string(),
                        keep_type,
                    )
                    .disabled(!media.keep_type_enabled()),
                ),
        )
        .group(SettingGroup::new().title(t("Priority")).items([
            configure_button(ui, "video_priority"),
            configure_button(ui, "audio_priority"),
            configure_button(ui, "codec_priority"),
        ]))
        .group(
            SettingGroup::new()
                .title(lang.tr("DownloadFormatCard", "Download Format").to_string())
                .item(SettingItem::new(
                    lang.tr("DownloadFormatCard", "Output Container Format")
                        .to_string(),
                    cfg_dropdown(
                        ui,
                        "Download",
                        "video_container",
                        vec![
                            (SharedString::from("mp4"), t("MP4")),
                            (SharedString::from("mkv"), t("MKV")),
                        ],
                    ),
                ))
                .item(SettingItem::new(
                    lang.tr("DownloadFormatCard", "Convert M4A to MP3")
                        .to_string(),
                    cfg_switch(ui, "Download", "m4a_to_mp3"),
                )),
        )
}

// ---------- Additional page ----------

/// Gating for the embed/delete rows: download on, format `ass`, container
/// `mkv` (cross-card dependency, mirrored for danmaku + subtitle).
fn embed_gating(cfg: &config::ConfigStore, download_key: &str, type_key: &str) -> bool {
    model::can_embed_subtitle_like(
        model::get_bool(cfg, "Additional", download_key),
        &model::get_str(cfg, "Additional", type_key),
        &model::get_str(cfg, "Download", "video_container"),
    )
}

fn additional_page(ui: &Entity<SettingsUi>, lang: Lang, cx: &App) -> SettingPage {
    let t = move |s: &str| SharedString::from(lang.tr("SettingInterface", s).to_string());

    let s = ui.read(cx);
    let danmaku_gate = embed_gating(&s.cfg, "download_danmaku", "danmaku_type");
    let subtitle_gate = embed_gating(&s.cfg, "download_subtitle", "subtitle_type");
    let attach_gate = model::cover_cascade(
        model::get_bool(&s.cfg, "Additional", "download_cover"),
        &model::get_str(&s.cfg, "Additional", "cover_type"),
        true,
        false,
    )
    .1;
    let delete_cover_gate = model::cover_cascade(
        model::get_bool(&s.cfg, "Additional", "download_cover"),
        &model::get_str(&s.cfg, "Additional", "cover_type"),
        model::get_bool(&s.cfg, "Additional", "attach_cover"),
        true,
    )
    .2;

    SettingPage::new(t("Additional"))
        .icon(IconName::Sparkles)
        .group(
            SettingGroup::new()
                .title(lang.tr("ADDITIONAL_FILES_QUALIFIER", "Danmaku").to_string())
                .item(SettingItem::new(
                    lang.tr("DanmakuSettingCard", "Download Danmaku")
                        .to_string(),
                    cfg_switch(ui, "Additional", "download_danmaku"),
                ))
                .item(SettingItem::new(
                    lang.tr("DanmakuSettingCard", "Danmaku Format").to_string(),
                    cfg_dropdown(
                        ui,
                        "Additional",
                        "danmaku_type",
                        vec![
                            (SharedString::from("xml"), t("XML")),
                            (SharedString::from("ass"), t("ASS")),
                            (SharedString::from("protocol"), t("Protocol")),
                        ],
                    ),
                ))
                .item(
                    SettingItem::new(
                        lang.tr("DanmakuSettingCard", "Embed Danmaku").to_string(),
                        cfg_switch(ui, "Additional", "embed_danmaku"),
                    )
                    .disabled(!danmaku_gate),
                )
                .item(
                    SettingItem::new(
                        lang.tr("DanmakuSettingCard", "Delete Danmaku After Embedding")
                            .to_string(),
                        cfg_switch(ui, "Additional", "delete_danmaku_after_embed"),
                    )
                    .disabled(!danmaku_gate),
                )
                .item(configure_button(ui, "danmaku_style")),
        )
        .group(
            SettingGroup::new()
                .title(
                    lang.tr("ADDITIONAL_FILES_QUALIFIER", "Subtitles")
                        .to_string(),
                )
                .item(SettingItem::new(
                    lang.tr("SubtitleSettingCard", "Download Subtitles")
                        .to_string(),
                    cfg_switch(ui, "Additional", "download_subtitle"),
                ))
                .item(configure_button(ui, "subtitle_language"))
                .item(SettingItem::new(
                    lang.tr("SubtitleSettingCard", "Subtitle Format")
                        .to_string(),
                    cfg_dropdown(
                        ui,
                        "Additional",
                        "subtitle_type",
                        vec![
                            (SharedString::from("srt"), t("SRT")),
                            (SharedString::from("ass"), t("ASS")),
                        ],
                    ),
                ))
                .item(configure_button(ui, "subtitle_style"))
                .item(
                    SettingItem::new(
                        lang.tr("SubtitleSettingCard", "Embed Subtitles")
                            .to_string(),
                        cfg_switch(ui, "Additional", "embed_subtitle"),
                    )
                    .disabled(!subtitle_gate),
                )
                .item(
                    SettingItem::new(
                        lang.tr("SubtitleSettingCard", "Delete Subtitles After Embedding")
                            .to_string(),
                        cfg_switch(ui, "Additional", "delete_subtitle_after_embed"),
                    )
                    .disabled(!subtitle_gate),
                ),
        )
        .group(
            SettingGroup::new()
                .title(lang.tr("DownloadPreviewBar", "Cover").to_string())
                .item(SettingItem::new(
                    lang.tr("CoverSettingCard", "Download Cover").to_string(),
                    {
                        let get = ui.clone();
                        let set = ui.clone();
                        let id = ui.entity_id();
                        SettingField::switch(
                            move |cx| {
                                model::get_bool(&get.read(cx).cfg, "Additional", "download_cover")
                            },
                            move |v, cx| {
                                set.update(cx, |ui, _| {
                                    let ctype = model::get_str(&ui.cfg, "Additional", "cover_type");
                                    let attach =
                                        model::get_bool(&ui.cfg, "Additional", "attach_cover");
                                    let delete = model::get_bool(
                                        &ui.cfg,
                                        "Additional",
                                        "delete_cover_after_attach",
                                    );
                                    let (download, attach, delete, _) =
                                        model::cover_cascade(v, &ctype, attach, delete);
                                    ui.put("Additional", "download_cover", Value::from(download));
                                    ui.put("Additional", "attach_cover", Value::from(attach));
                                    ui.put(
                                        "Additional",
                                        "delete_cover_after_attach",
                                        Value::from(delete),
                                    );
                                });
                                kick(cx, id);
                            },
                        )
                    },
                ))
                .item(SettingItem::new(
                    lang.tr("CoverSettingCard", "Cover Format").to_string(),
                    {
                        let get = ui.clone();
                        let set = ui.clone();
                        let id = ui.entity_id();
                        SettingField::dropdown(
                            vec![
                                (SharedString::from("png"), t("PNG")),
                                (SharedString::from("jpg"), t("JPG")),
                                (SharedString::from("webp"), t("WebP")),
                                (SharedString::from("avif"), t("AVIF")),
                            ],
                            move |cx| {
                                model::get_str(&get.read(cx).cfg, "Additional", "cover_type").into()
                            },
                            move |v, cx| {
                                set.update(cx, |ui, _| {
                                    let download =
                                        model::get_bool(&ui.cfg, "Additional", "download_cover");
                                    let attach =
                                        model::get_bool(&ui.cfg, "Additional", "attach_cover");
                                    let delete = model::get_bool(
                                        &ui.cfg,
                                        "Additional",
                                        "delete_cover_after_attach",
                                    );
                                    let (_, attach, delete, _) =
                                        model::cover_cascade(download, v.as_ref(), attach, delete);
                                    ui.put("Additional", "cover_type", Value::from(v.to_string()));
                                    ui.put("Additional", "attach_cover", Value::from(attach));
                                    ui.put(
                                        "Additional",
                                        "delete_cover_after_attach",
                                        Value::from(delete),
                                    );
                                });
                                kick(cx, id);
                            },
                        )
                    },
                ))
                .item(
                    SettingItem::new(lang.tr("CoverSettingCard", "Embed Cover").to_string(), {
                        let get = ui.clone();
                        let set = ui.clone();
                        let id = ui.entity_id();
                        SettingField::switch(
                            move |cx| {
                                model::get_bool(&get.read(cx).cfg, "Additional", "attach_cover")
                            },
                            move |v, cx| {
                                set.update(cx, |ui, _| {
                                    let download =
                                        model::get_bool(&ui.cfg, "Additional", "download_cover");
                                    let ctype = model::get_str(&ui.cfg, "Additional", "cover_type");
                                    let delete = model::get_bool(
                                        &ui.cfg,
                                        "Additional",
                                        "delete_cover_after_attach",
                                    );
                                    let (_, attach, delete, _) =
                                        model::cover_cascade(download, &ctype, v, delete);
                                    ui.put("Additional", "attach_cover", Value::from(attach));
                                    ui.put(
                                        "Additional",
                                        "delete_cover_after_attach",
                                        Value::from(delete),
                                    );
                                });
                                kick(cx, id);
                            },
                        )
                    })
                    .disabled(!attach_gate),
                )
                .item(
                    SettingItem::new(
                        lang.tr("CoverSettingCard", "Delete Cover After Embedding")
                            .to_string(),
                        cfg_switch(ui, "Additional", "delete_cover_after_attach"),
                    )
                    .disabled(!delete_cover_gate),
                ),
        )
        .group(
            SettingGroup::new()
                .title(t("Chapter and metadata"))
                .item(SettingItem::new(
                    lang.tr("ChapterSettingCard", "Embed Chapters").to_string(),
                    cfg_switch(ui, "Additional", "embed_chapter"),
                ))
                .item(SettingItem::new(
                    lang.tr("MetadataSettingCard", "Download Metadata")
                        .to_string(),
                    cfg_switch(ui, "Additional", "download_metadata"),
                ))
                .item(SettingItem::new(
                    lang.tr("MetadataSettingCard", "Metadata Format")
                        .to_string(),
                    cfg_dropdown(
                        ui,
                        "Additional",
                        "metadata_type",
                        vec![
                            (SharedString::from("json"), t("JSON")),
                            (SharedString::from("xml"), t("XML")),
                            (SharedString::from("nfo"), t("NFO")),
                        ],
                    ),
                )),
        )
}

// ---------- File naming page ----------

fn file_naming_page(ui: &Entity<SettingsUi>, lang: Lang, cx: &App) -> SettingPage {
    let t = move |s: &str| SharedString::from(lang.tr("SettingInterface", s).to_string());

    let convention = {
        let h = ui.clone();
        let id = ui.entity_id();
        SettingItem::render(move |_, _, cx| {
            let text = h.update(cx, |ui, _| {
                ui.tr("SettingInterface", "Customize naming rules…")
            });
            let hh = h.clone();
            Button::new("open-naming-rules")
                .label(text)
                .outline()
                .on_click(move |_, _, cx| {
                    hh.update(cx, |ui, _| ui.rules_open = true);
                    kick(cx, id);
                })
                .into_any_element()
        })
    };

    let numbering = cfg_int_dropdown(
        ui,
        "File Naming",
        "numbering_type",
        vec![
            (
                0,
                lang.tr(
                    "NumberSettingCard",
                    "Sequential numbering starting from 1 per batch",
                )
                .to_string()
                .into(),
            ),
            (
                1,
                lang.tr("NumberSettingCard", "Use the index from the parse list")
                    .to_string()
                    .into(),
            ),
            (
                2,
                lang.tr("NumberSettingCard", "Global sequential numbering")
                    .to_string()
                    .into(),
            ),
        ],
        0,
    );

    let global_mode = model::get_int(&ui.read(cx).cfg, "File Naming", "numbering_type", 0) == 2;

    let starting_number = {
        let get = ui.clone();
        let set = ui.clone();
        let id = ui.entity_id();
        SettingField::number_input(
            NumberFieldOptions {
                min: 0.,
                max: 999_999.,
                step: 1.,
            },
            move |cx| get.read(cx).numbering.global_starting_number as f64,
            move |v, cx| {
                set.update(cx, |ui, _| {
                    ui.numbering.global_starting_number = v.round() as i64
                });
                kick(cx, id);
            },
        )
    };

    SettingPage::new(t("File naming"))
        .icon(IconName::Type)
        .group(
            SettingGroup::new()
                .title(
                    lang.tr("NamingConventionCard", "Naming Convention")
                        .to_string(),
                )
                .item(convention)
                .item(SettingItem::new(
                    lang.tr("NumberSettingCard", "Numbering Mode").to_string(),
                    numbering,
                ))
                .item(
                    SettingItem::new(
                        lang.tr("NumberSettingCard", "Global Sequential Starting Number")
                            .to_string(),
                        starting_number,
                    )
                    .disabled(!global_mode),
                ),
        )
}

// ---------- Advanced page ----------

fn advanced_page(ui: &Entity<SettingsUi>, lang: Lang) -> SettingPage {
    let t = move |s: &str| SharedString::from(lang.tr("SettingInterface", s).to_string());

    // MCP token row: label + Copy / Regenerate / Copy client config.
    let token_row = {
        let h = ui.clone();
        let id = ui.entity_id();
        SettingItem::render(move |_, _, cx| {
            let (token, copy, regen, client, copied) = h.update(cx, |ui, _| {
                let token = model::ensure_token(&mut ui.cfg);
                (
                    token,
                    ui.tr("MCPSettingCard", "Copy"),
                    ui.tr("MCPSettingCard", "Regenerate"),
                    ui.tr("MCPSettingCard", "Client Configuration"),
                    ui.tr("MCPSettingCard", "Client configuration copied"),
                )
            });
            let hh = h.clone();
            let mut row = div().flex().items_center().gap_2().child(Label::new(token));
            for (bid, label) in [
                ("mcp-copy-token", copy),
                ("mcp-regen", regen),
                ("mcp-copy-client", client),
            ] {
                let h2 = hh.clone();
                let copied = copied.clone();
                row = row.child(Button::new(bid).label(label).outline().on_click(
                    move |_, _, cx| match bid {
                        "mcp-copy-token" => {
                            let token =
                                h2.update(cx, |ui, _| model::get_str(&ui.cfg, "MCP", "mcp_token"));
                            cx.write_to_clipboard(ClipboardItem::new_string(token));
                        }
                        "mcp-regen" => {
                            h2.update(cx, |ui, _| {
                                let token = uuid::Uuid::new_v4().simple().to_string();
                                ui.put("MCP", "mcp_token", Value::from(token));
                            });
                            kick(cx, id);
                        }
                        _ => {
                            let conf = h2.update(cx, |ui, _| {
                                let token = model::get_str(&ui.cfg, "MCP", "mcp_token");
                                model::http_client_config(&ui.cfg, &token).to_string()
                            });
                            cx.write_to_clipboard(ClipboardItem::new_string(conf));
                            h2.update(cx, |ui, _| ui.notice = Some(copied.clone()));
                            kick(cx, id);
                        }
                    },
                ));
            }
            row.into_any_element()
        })
    };

    SettingPage::new(t("Advanced"))
        .icon(IconName::Wrench)
        .group(SettingGroup::new().title(t("CDN")).items([
            configure_button(ui, "cn_cdn_list"),
            configure_button(ui, "ov_cdn_list"),
            SettingItem::new(
                t("CDN provider"),
                cfg_dropdown(
                    ui,
                    "Advanced",
                    "prefer_cdn_server_provider_",
                    vec![
                        (
                            SharedString::from("auto"),
                            lang.tr("AUDIO_QUALITY", "Auto").to_string().into(),
                        ),
                        (SharedString::from("cn"), t("Mainland China")),
                        (SharedString::from("ov"), t("Overseas")),
                    ],
                ),
            ),
        ]))
        .group(
            SettingGroup::new()
                .title(t("FFmpeg"))
                .item(SettingItem::new(
                    t("FFmpeg source"),
                    cfg_dropdown(
                        ui,
                        "Advanced",
                        "ffmpeg_source",
                        vec![
                            (
                                SharedString::from("system"),
                                lang.tr("FFmpegSettingCard", "System PATH")
                                    .to_string()
                                    .into(),
                            ),
                            (
                                SharedString::from("custom"),
                                lang.tr("FFmpegSettingCard", "Custom path")
                                    .to_string()
                                    .into(),
                            ),
                        ],
                    ),
                ))
                .item(SettingItem::new(
                    t("Custom FFmpeg path"),
                    cfg_input(ui, "Advanced", "custom_ffmpeg_path"),
                )),
        )
        .group(
            SettingGroup::new()
                .title(t("Proxy"))
                .item(SettingItem::new(
                    t("Proxy mode"),
                    cfg_int_dropdown(
                        ui,
                        "Advanced",
                        "proxy_mode",
                        vec![
                            (0, t("System proxy")),
                            (1, t("Custom proxy")),
                            (2, t("No proxy")),
                        ],
                        0,
                    ),
                ))
                .item(SettingItem::new(
                    lang.tr("ProxyDialog", "Proxy Type").to_string(),
                    cfg_input(ui, "Advanced", "proxy_type"),
                ))
                .item(SettingItem::new(
                    t("Proxy server"),
                    cfg_input(ui, "Advanced", "proxy_server"),
                ))
                .item(SettingItem::new(
                    lang.tr("ProxyDialog", "Port").to_string(),
                    cfg_number(ui, "Advanced", "proxy_port", 0., 65_535., 1., 0),
                ))
                .item(SettingItem::new(
                    lang.tr("ProxyDialog", "Username").to_string(),
                    cfg_input(ui, "Advanced", "proxy_uname"),
                ))
                .item(SettingItem::new(
                    lang.tr("ProxyDialog", "Password").to_string(),
                    cfg_input(ui, "Advanced", "proxy_password"),
                )),
        )
        .group(
            SettingGroup::new()
                .title(lang.tr("MCPSettingCard", "MCP Server").to_string())
                .item(SettingItem::new(
                    lang.tr("MCPSettingCard", "Enable MCP Server").to_string(),
                    cfg_switch(ui, "MCP", "mcp_enabled"),
                ))
                .item(SettingItem::new(
                    lang.tr("MCPSettingCard", "Port").to_string(),
                    cfg_number(ui, "MCP", "mcp_port", 1., 65_535., 1., 23_330),
                ))
                .item(token_row),
        )
        .group(
            SettingGroup::new()
                .title(t("Other"))
                .item(SettingItem::new(
                    t("Area"),
                    cfg_dropdown(
                        ui,
                        "Advanced",
                        "area",
                        vec![
                            (SharedString::from("cn"), t("Mainland China")),
                            (SharedString::from("hk_tw"), t("Hong Kong & Taiwan")),
                            (SharedString::from("ov"), t("Overseas")),
                        ],
                    ),
                ))
                .item(SettingItem::new(
                    t("User agent"),
                    cfg_input(ui, "Advanced", "user_agent"),
                ))
                .item(configure_button(ui, "log_viewer")),
        )
}

// ---------- Updates page ----------

fn updates_page(ui: &Entity<SettingsUi>, lang: Lang) -> SettingPage {
    let t = move |s: &str| SharedString::from(lang.tr("SettingInterface", s).to_string());

    let check_now = {
        let h = ui.clone();
        let id = ui.entity_id();
        SettingItem::render(move |_, _, cx| {
            let text = h.update(cx, |ui, _| {
                ui.tr("CheckUpdateSettingCard", "Check for Updates")
            });
            let hh = h.clone();
            Button::new("check-updates")
                .label(text)
                .outline()
                .on_click(move |_, _, cx| {
                    hh.update(cx, |ui, _| ui.pending_dialog = Some("check_update"));
                    kick(cx, id);
                })
                .into_any_element()
        })
    };

    SettingPage::new(t("Updates"))
        .icon(IconName::RefreshCw)
        .group(
            SettingGroup::new()
                .title(t("Update"))
                .item(check_now)
                .item(SettingItem::new(
                    lang.tr("CheckUpdateSettingCard", "Include Prerelease Versions")
                        .to_string(),
                    cfg_switch(ui, "Update", "include_prerelease"),
                )),
        )
}

// ---------- page assembly ----------

fn render_pages(
    ui: &Entity<SettingsUi>,
    _window: &mut Window,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let lang = ui.read(cx).lang;
    let media = ui.read(cx).media;
    let _ = media;

    let pages = [
        interface_page(ui, lang),
        behavior_page(ui, lang),
        download_page(ui, lang, cx),
        additional_page(ui, lang, cx),
        file_naming_page(ui, lang, cx),
        advanced_page(ui, lang),
        updates_page(ui, lang),
    ];

    Settings::new("app-settings")
        .pages(pages)
        .into_any_element()
}

// ---------- naming-rules window ----------

fn render_rules_window(
    ui: &Entity<SettingsUi>,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let _ = window;
    let id = ui.entity_id();

    let (lang, entries, filter, selected) = {
        let s = ui.read(cx);
        let text = s.rules_search.read(cx).value().to_string();
        let filter = s.rules.filter_indices(&text);
        let entries: Vec<RuleEntry> = match &filter {
            Some(ixs) => ixs.iter().map(|&i| s.rules.rules[i].clone()).collect(),
            None => s.rules.display_entries(),
        };
        (s.lang, entries, filter, s.rules.current_index)
    };
    let t = move |s: &str| SharedString::from(lang.tr("RuleListDialog", s).to_string());

    // Rule list: name + type per row; click selects (committing edits).
    let h = ui.clone();
    let list = div()
        .flex()
        .flex_col()
        .gap_1()
        .children((0..entries.len()).map(move |display_ix| {
            // Display position → rule index through the active filter.
            let rule_ix = filter.as_ref().map(|f| f[display_ix]).unwrap_or(display_ix);
            let entry = entries[rule_ix].clone();
            let selected = selected == Some(rule_ix);
            let name = {
                let mut n = model::display_name(&entry);
                if entry.default {
                    n.push_str(&format!(" ({})", lang.tr("RuleBuilderWidget", "default")));
                }
                n
            };
            let type_label = model::type_key(entry.type_)
                .map(|k| lang.convention_type(k))
                .unwrap_or_default();
            let hh = h.clone();
            ListItem::new(SharedString::from(format!("rule-{display_ix}")))
                .selected(selected)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(Label::new(name))
                        .child(Label::new(type_label)),
                )
                .on_click(move |_, _, cx| {
                    hh.update(cx, |ui, _| {
                        if ui.rules.current_index != Some(rule_ix) {
                            if let Err((revert, _)) = ui.rules.on_select(rule_ix) {
                                ui.rules.select(revert);
                            }
                        }
                    });
                    kick(cx, id);
                })
        }));

    // Toolbar: CRUD + order + save.
    fn btn(
        h: &Entity<SettingsUi>,
        bid: &'static str,
        label: SharedString,
        act: impl Fn(&mut SettingsUi) + 'static,
    ) -> Button {
        let hh = h.clone();
        let id = h.entity_id();
        Button::new(bid)
            .label(label)
            .outline()
            .on_click(move |_, _, cx| {
                hh.update(cx, |ui, _| act(ui));
                kick(cx, id);
            })
    }
    let new_id = || uuid::Uuid::new_v4().simple().to_string();

    let toolbar = div().flex().flex_wrap().gap_2().children([
        btn(ui, "rule-add", t("Add"), move |ui| {
            if let Err((revert, e)) = ui.rules.add_rule(new_id()) {
                ui.rules.select(revert);
                ui.editor_error = Some(e);
            } else {
                ui.editor_error = None;
            }
        }),
        btn(ui, "rule-duplicate", t("Duplicate"), move |ui| {
            if let Err((revert, e)) = ui.rules.duplicate_rule(new_id()) {
                ui.rules.select(revert);
                ui.editor_error = Some(e);
            } else {
                ui.editor_error = None;
            }
        }),
        btn(ui, "rule-delete", t("Delete"), |ui| {
            if let Err(msg) = ui.rules.delete_rule() {
                ui.notice = Some(msg);
            } else {
                ui.editor_error = None;
            }
        }),
        btn(ui, "rule-restore", t("Restore to Built-in Default"), |ui| {
            if ui.rules.restore_default_rule() {
                ui.notice = Some(ui.tr("RuleListDialog", "Restored to the built-in default"));
            }
        }),
        btn(ui, "rule-reset", t("Reset to Default"), |ui| {
            ui.rules.reset_to_default();
        }),
        btn(
            ui,
            "rule-up",
            lang.tr("RuleLevelRow", "Move up").to_string().into(),
            |ui| {
                if let Some(ix) = ui.rules.current_index {
                    if ix > 0 {
                        let _ = ui.rules.move_rule(ix, ix - 1);
                    }
                }
            },
        ),
        btn(
            ui,
            "rule-down",
            lang.tr("RuleLevelRow", "Move down").to_string().into(),
            |ui| {
                if let Some(ix) = ui.rules.current_index {
                    if ix + 1 < ui.rules.rules.len() {
                        let _ = ui.rules.move_rule(ix, ix + 1);
                    }
                }
            },
        ),
        btn(ui, "rule-save", t("Save"), |ui| {
            match ui.rules.save(&mut ui.cfg) {
                Ok(()) => {
                    ui.notice = Some(ui.tr("RuleListDialog", "Naming rules saved"));
                    ui.editor_error = None;
                }
                Err(e) => ui.editor_error = Some((EditorField::Rule, e.to_string())),
            }
        }),
    ]);

    let close = {
        let hh = ui.clone();
        let id = ui.entity_id();
        Button::new("rules-close")
            .label(lang.tr("ViewCoverDialog", "Close").to_string())
            .outline()
            .on_click(move |_, _, cx| {
                hh.update(cx, |ui, _| {
                    if ui.rules.dirty() {
                        ui.notice = Some(ui.tr("RuleListDialog", "Discard changes?"));
                    } else {
                        ui.rules_open = false;
                    }
                });
                kick(cx, id);
            })
    };

    let search_handle = ui.read(cx).rules_search.clone();
    let search = Input::new(&search_handle);

    let editor = render_rule_editor(ui, cx);

    div()
        .id("naming-rules-window")
        .absolute()
        .size_full()
        .top_0()
        .left_0()
        .bg(cx.theme().background.alpha(0.3))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(960.))
                .h(px(640.))
                .bg(cx.theme().popover)
                .rounded_lg()
                .shadow_lg()
                .flex()
                .flex_col()
                .p_4()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(Label::new(t("Naming Rules")))
                        .child(close),
                )
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .gap_3()
                        .child(
                            div()
                                .w(px(340.))
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(search)
                                .child(toolbar)
                                .child(list),
                        )
                        .child(editor),
                ),
        )
        .into_any_element()
}

/// Right pane of the rules window: the `EditRuleDialog` fields.
fn render_rule_editor(ui: &Entity<SettingsUi>, cx: &mut Context<Shell>) -> AnyElement {
    let id = ui.entity_id();
    let s = ui.read(cx);
    let lang = s.lang;
    let type_ = s.rules.editor.type_;
    let default_checked = s.rules.editor.default_checked;
    let error = s.editor_error.clone();
    let preview = s.rules.editor.preview_rows();
    let convention = ConventionType::ALL
        .into_iter()
        .find(|t| t.id() == type_)
        .unwrap_or(ConventionType::Normal);
    let variables = VariableListFactory::new().build(convention, false);
    let rule_name = s.rule_name.clone();
    let rule_body = s.rule_body.clone();

    let t = move |s: &str| SharedString::from(lang.tr("EditRuleDialog", s).to_string());

    let type_selector = div()
        .flex()
        .flex_wrap()
        .gap_2()
        .children(ConventionType::ALL.iter().map(|ct| {
            let cid = ct.id();
            let label = model::type_key(cid)
                .map(|k| lang.convention_type(k))
                .unwrap_or_default();
            let hh = ui.clone();
            Button::new(SharedString::from(format!("rule-type-{cid}")))
                .label(label)
                .outline()
                .when(cid == type_, |this| this.primary())
                .on_click(move |_, _, cx| {
                    hh.update(cx, |ui, _| ui.rules.editor.type_ = cid);
                    kick(cx, id);
                })
        }));

    let default_box = {
        let hh = ui.clone();
        Switch::new("rule-default")
            .checked(default_checked)
            .on_change(move |v, _, cx| {
                hh.update(cx, |ui, _| ui.rules.editor.default_checked = *v);
                kick(cx, id);
            })
            .label(
                lang.tr("EditRuleDialog", "Set as default rule for this type")
                    .to_string(),
            )
    };

    let error_line = error.map(|(field, msg)| {
        let label = match field {
            EditorField::Name => t("Rule Name"),
            EditorField::Rule => t("Naming Rule"),
        };
        Label::new(format!("{label}: {msg}"))
    });

    let preview_rows: Vec<AnyElement> = preview
        .iter()
        .map(|(label, path)| {
            div()
                .flex()
                .flex_col()
                .child(Label::new(label.clone()))
                .child(Label::new(path.clone().unwrap_or_else(|| "—".to_string())))
                .into_any_element()
        })
        .collect();

    let var_rows: Vec<AnyElement> = variables
        .iter()
        .map(|v| {
            div()
                .flex()
                .flex_col()
                .child(Label::new(v.variable))
                .child(Label::new(lang.variable_description(v.name)))
                .into_any_element()
        })
        .collect();

    div()
        .id("rule-editor")
        .flex_1()
        .flex()
        .flex_col()
        .gap_3()
        .overflow_y_scroll()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(Label::new(t("Rule Name")))
                .child(Input::new(&rule_name)),
        )
        .child(type_selector)
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(Label::new(t("Naming Rule")))
                .child(Input::new(&rule_body)),
        )
        .child(default_box)
        .when_some(error_line, |this, e| this.child(e))
        .children(preview_rows)
        .child(Label::new(
            lang.tr("EditRuleDialog", "Variable reference").to_string(),
        ))
        .children(var_rows)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::ShellI18n;
    use gpui_kit::gpui::{Render, TestApp};
    use gpui_kit::{div, IntoElement};
    use services::i18n::{Language, ResolvedLanguage};

    /// Minimal window root that owns the settings entity: `SettingsUi::new`
    /// needs a real window, and the drop of this view releases it.
    struct Harness {
        ui: Entity<SettingsUi>,
    }

    impl Render for Harness {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// Instant apply: choosing a language flips the settings entity's
    /// resolved `Lang` and the app-global `ShellI18n` the shell renders
    /// from, and persists the string. No restart notice is set.
    #[test]
    fn language_setting_applies_instantly() {
        let mut app = TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app.open_window(|window, cx| Harness {
            ui: cx.new(|cx| SettingsUi::new(window, cx)),
        });
        window.update(|h: &mut Harness, _window, cx| {
            let ui = h.ui.clone();
            ui.update(cx, |ui, cx| apply_language(ui, cx, "zh_TW"));
        });
        window.read(|h: &Harness, cx| {
            let ui = h.ui.read(cx);
            assert_eq!(ui.lang.resolved, ResolvedLanguage::ZhTw);
            assert_eq!(
                model::get_str(&ui.cfg, "Interface", "language"),
                "zh_TW",
                "stored string persists"
            );
            assert!(ui.notice.is_none(), "no restart notice");
            assert_eq!(cx.global::<ShellI18n>().lang, Language::ChineseTraditional);
        });
        drop(window);
        app.update(|cx| cx.shutdown());
    }
}
