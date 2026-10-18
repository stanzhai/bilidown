//! CDN-server-list editor (ticket 31): ports of
//! `src/gui/dialog/setting/cdn_server.py` (the list manager) and
//! `src/gui/dialog/setting/edit_host.py` (the nested single-node
//! editor). Two settings rows — `cn_cdn_list` / `ov_cdn_list` — edit
//! `Advanced.cn_cdn_server_list` / `Advanced.ov_cdn_server_list`
//! (arrays of `{host, provider}`), the keys `cdn_from_config` reads.
//! The working copy is only persisted on OK, so cancel discards — no
//! deep-copy hazard (the Python version deep-copies for the same reason).

use serde_json::{json, Value};

use gpui_kit::component::button::Button;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::label::Label;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, px, AppContext, ParentElement, SharedString, Window};

use super::active_catalog;
use services::i18n::Catalog;

const LIST_CTX: &str = "CDNServerDialog";
const EDIT_CTX: &str = "EditHostDialog";

/// Which `Advanced` list key a dialog edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CdnListKey {
    Cn,
    Ov,
}

impl CdnListKey {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cn => "cn_cdn_server_list",
            Self::Ov => "ov_cdn_server_list",
        }
    }

    fn default_servers(self) -> Vec<CdnServerEntry> {
        let servers = match self {
            Self::Cn => network::cdn::CdnConfig::default_cn_servers(),
            Self::Ov => network::cdn::CdnConfig::default_ov_servers(),
        };
        servers
            .into_iter()
            .map(|s| CdnServerEntry {
                host: s.host,
                provider: s.provider,
            })
            .collect()
    }
}

/// `Translator.CDN_SERVER_PROVIDER`: provider key → display label.
pub fn provider_label(catalog: &Catalog, provider: &str) -> String {
    let source = match provider {
        "ALIYUN" => "Aliyun",
        "TENCENT" => "Tencent Cloud",
        "HUAWEI" => "Huawei Cloud",
        "CUSTOM" => "Custom",
        other => return other.to_owned(),
    };
    catalog.tr("CDN_SERVER_PROVIDER", source).to_owned()
}

/// One `{host, provider}` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct CdnServerEntry {
    pub host: String,
    pub provider: String,
}

/// Headless view-model of one CDN list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CdnListState {
    pub entries: Vec<CdnServerEntry>,
}

impl CdnListState {
    pub fn load(cfg: &config::ConfigStore, key: CdnListKey) -> Self {
        let entries = cfg
            .get("Advanced", key.as_str())
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|entry| {
                        Some(CdnServerEntry {
                            host: entry.get("host").and_then(Value::as_str)?.to_owned(),
                            provider: entry
                                .get("provider")
                                .and_then(Value::as_str)
                                .unwrap_or("CUSTOM")
                                .to_owned(),
                        })
                    })
                    .collect()
            })
            .filter(|l: &Vec<CdnServerEntry>| !l.is_empty())
            .unwrap_or_else(|| key.default_servers());
        Self { entries }
    }

    pub fn to_value(&self) -> Value {
        Value::Array(
            self.entries
                .iter()
                .map(|e| json!({ "host": e.host, "provider": e.provider }))
                .collect(),
        )
    }

    pub fn save(&self, cfg: &mut config::ConfigStore, key: CdnListKey) -> anyhow::Result<()> {
        cfg.set("Advanced", key.as_str(), self.to_value());
        cfg.save()
    }

    /// `EditHostDialog.validate`: a blank host is rejected.
    pub fn validate_host(host: &str) -> Result<(), &'static str> {
        if host.trim().is_empty() {
            Err("CDN node cannot be empty")
        } else {
            Ok(())
        }
    }
}

/// Open the nested single-node editor (`EditHostDialog`); `on_accept`
/// receives the trimmed host on OK.
pub fn open_edit_host(
    catalog: &'static Catalog,
    initial: String,
    on_accept: impl FnOnce(String, &mut gpui_kit::App) + 'static,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let title = catalog.tr(EDIT_CTX, "Edit CDN Node").to_owned();
    let placeholder = catalog.tr(EDIT_CTX, "Please enter a CDN node").to_owned();
    let empty_msg = catalog.tr(EDIT_CTX, "CDN node cannot be empty").to_owned();
    let ok = "OK".to_owned();

    let input = cx.new(|cx| {
        let mut s = InputState::new(window, cx).placeholder(placeholder);
        s.set_value(initial, window, cx);
        s
    });
    let on_accept = std::rc::Rc::new(std::cell::RefCell::new(Some(on_accept)));

    window.open_dialog(cx, move |dialog, _, _| {
        let input_cell = input.clone();
        dialog
            .title(title.clone())
            .w(px(400.))
            .content(move |content, _, _| {
                content.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(Input::new(&input_cell).id("cdn-node")),
                )
            })
            .footer(
                DialogFooter::new().child(Button::new("confirm").label(ok.clone()).on_click({
                    let input = input.clone();
                    let on_accept = on_accept.clone();
                    let empty_msg = empty_msg.clone();
                    move |_, window, cx| {
                        let host = input.read(cx).value().trim().to_owned();
                        if CdnListState::validate_host(&host).is_err() {
                            window.push_notification(Notification::error(empty_msg.clone()), cx);
                            return;
                        }
                        window.close_dialog(cx);
                        if let Some(cb) = on_accept.borrow_mut().take() {
                            cb(host, cx);
                        }
                    }
                })),
            )
    });
}

/// Open the CDN list manager (`CDNServerDialog`) for one of the two
/// list keys. Add appends a CUSTOM entry then opens the node editor;
/// per-row Edit/Remove; "Reset to Default" restores the shipped list;
/// OK persists. No drag reorder (fixed order approximates priority).
pub fn open_cdn_server_list(
    cfg: config::ConfigStore,
    key: CdnListKey,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let catalog = active_catalog(&cfg);
    let title = catalog
        .tr(LIST_CTX, "Customize Service Provider CDN")
        .to_owned();
    let tip = catalog
        .tr(
            LIST_CTX,
            "Drag items to reorder. Higher items have higher priority.",
        )
        .to_owned();
    let add = catalog.tr(LIST_CTX, "Add").to_owned();
    let reset = catalog.tr(LIST_CTX, "Reset to Default").to_owned();
    let node = catalog.tr(LIST_CTX, "Node").to_owned();
    let provider = catalog.tr(LIST_CTX, "Provider").to_owned();
    let actions = catalog.tr(LIST_CTX, "Actions").to_owned();
    let edit = catalog.tr(LIST_CTX, "Edit").to_owned();
    let remove = catalog.tr(LIST_CTX, "Remove").to_owned();
    let ok = catalog.tr(LIST_CTX, "OK").to_owned();

    let state = std::rc::Rc::new(std::cell::RefCell::new(CdnListState::load(&cfg, key)));
    let cfg_cell = std::rc::Rc::new(std::cell::RefCell::new(cfg));

    window.open_dialog(cx, move |dialog, _, _| {
        let st = state.borrow();
        let rows: Vec<(String, String)> = st
            .entries
            .iter()
            .map(|e| (e.host.clone(), provider_label(catalog, &e.provider)))
            .collect();
        drop(st);

        dialog
            .title(title.clone())
            .w(px(650.))
            .content({
                let state = state.clone();
                let tip = tip.clone();
                let add = add.clone();
                let reset = reset.clone();
                let node = node.clone();
                let provider = provider.clone();
                let actions = actions.clone();
                let edit = edit.clone();
                let remove = remove.clone();
                move |content, _, _| {
                    content.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(Label::new(tip.clone()))
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(Button::new("cdn-add").label(add.clone()).on_click({
                                        let state = state.clone();
                                        move |_, window, cx| {
                                            state.borrow_mut().entries.push(CdnServerEntry {
                                                host: String::new(),
                                                provider: "CUSTOM".into(),
                                            });
                                            let last = state.borrow().entries.len() - 1;
                                            let initial = state.borrow().entries[last].host.clone();
                                            open_edit_host(
                                                catalog,
                                                initial,
                                                {
                                                    let state = state.clone();
                                                    move |host, _| {
                                                        let mut st = state.borrow_mut();
                                                        if let Some(e) = st.entries.get_mut(last) {
                                                            e.host = host;
                                                        }
                                                    }
                                                },
                                                window,
                                                cx,
                                            );
                                        }
                                    }))
                                    .child(Button::new("cdn-reset").label(reset.clone()).on_click(
                                        {
                                            let state = state.clone();
                                            move |_, _, _| {
                                                state.borrow_mut().entries = key.default_servers();
                                            }
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(div().flex_1().child(Label::new(node.clone())))
                                    .child(div().w(px(110.)).child(Label::new(provider.clone())))
                                    .child(div().w(px(150.)).child(Label::new(actions.clone()))),
                            )
                            .child(
                                div()
                                    .id("cdn-server-list")
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .max_h(px(300.))
                                    .overflow_y_scroll()
                                    .children(rows.iter().enumerate().map(|(i, (host, label))| {
                                        let edit_btn = Button::new(SharedString::from(format!(
                                            "cdn-edit-{i}"
                                        )))
                                        .label(edit.clone())
                                        .on_click({
                                            let state = state.clone();
                                            move |_, window, cx| {
                                                let initial = state
                                                    .borrow()
                                                    .entries
                                                    .get(i)
                                                    .map(|e| e.host.clone())
                                                    .unwrap_or_default();
                                                open_edit_host(
                                                    catalog,
                                                    initial,
                                                    {
                                                        let state = state.clone();
                                                        move |host, _| {
                                                            let mut st = state.borrow_mut();
                                                            if let Some(e) = st.entries.get_mut(i) {
                                                                e.host = host;
                                                            }
                                                        }
                                                    },
                                                    window,
                                                    cx,
                                                );
                                            }
                                        });
                                        let remove_btn = Button::new(SharedString::from(format!(
                                            "cdn-remove-{i}"
                                        )))
                                        .label(remove.clone())
                                        .on_click({
                                            let state = state.clone();
                                            move |_, _, _| {
                                                state.borrow_mut().entries.remove(i);
                                            }
                                        });
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(div().flex_1().child(Label::new(host.clone())))
                                            .child(
                                                div().w(px(110.)).child(Label::new(label.clone())),
                                            )
                                            .child(
                                                div()
                                                    .w(px(150.))
                                                    .flex()
                                                    .gap_1()
                                                    .child(edit_btn)
                                                    .child(remove_btn),
                                            )
                                    })),
                            ),
                    )
                }
            })
            .footer(
                DialogFooter::new().child(Button::new("confirm").label(ok.clone()).on_click({
                    let state = state.clone();
                    let cfg_cell = cfg_cell.clone();
                    move |_, window, cx| {
                        let result = state.borrow().save(&mut cfg_cell.borrow_mut(), key);
                        if let Err(e) = result {
                            window.push_notification(Notification::error(e.to_string()), cx);
                            return;
                        }
                        window.close_dialog(cx);
                    }
                })),
            )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Unique throwaway config dir per test.
    fn scratch_cfg() -> config::ConfigStore {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "t31-edit-host-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        config::ConfigStore::with_paths(&dir, "")
    }

    #[test]
    fn cn_load_defaults_match_the_shipped_list() {
        let cfg = scratch_cfg();
        let st = CdnListState::load(&cfg, CdnListKey::Cn);
        assert_eq!(st.entries.len(), 8);
        assert_eq!(st.entries[0].host, "upos-sz-mirror08c.bilivideo.com");
        assert_eq!(st.entries[0].provider, "HUAWEI");
        assert_eq!(st.entries[7].provider, "ALIYUN");
    }

    #[test]
    fn ov_load_defaults_match_the_shipped_list() {
        let cfg = scratch_cfg();
        let st = CdnListState::load(&cfg, CdnListKey::Ov);
        assert_eq!(st.entries.len(), 2);
        assert_eq!(st.entries[0].provider, "ALIYUN");
        assert_eq!(st.entries[1].provider, "TENCENT");
    }

    #[test]
    fn list_round_trips_through_the_config_section() {
        let mut cfg = scratch_cfg();
        let mut st = CdnListState::load(&cfg, CdnListKey::Cn);
        st.entries.truncate(2);
        st.entries.push(CdnServerEntry {
            host: "mirror.example.com".into(),
            provider: "CUSTOM".into(),
        });
        st.save(&mut cfg, CdnListKey::Cn).unwrap();

        let reloaded = CdnListState::load(&cfg, CdnListKey::Cn);
        assert_eq!(reloaded, st);
        assert_eq!(
            cfg.get("Advanced", "cn_cdn_server_list").unwrap(),
            &serde_json::json!([
                { "host": "upos-sz-mirror08c.bilivideo.com", "provider": "HUAWEI" },
                { "host": "upos-sz-mirrorhw.bilivideo.com", "provider": "HUAWEI" },
                { "host": "mirror.example.com", "provider": "CUSTOM" },
            ])
        );
    }

    #[test]
    fn empty_saved_list_falls_back_to_defaults_on_reload() {
        // `cdn_from_config` ignores empty lists; the dialog mirrors that.
        let mut cfg = scratch_cfg();
        let st = CdnListState { entries: vec![] };
        st.save(&mut cfg, CdnListKey::Ov).unwrap();
        let reloaded = CdnListState::load(&cfg, CdnListKey::Ov);
        assert_eq!(reloaded.entries.len(), 2);
    }

    #[test]
    fn validate_host_rejects_only_blank_input() {
        assert!(CdnListState::validate_host("").is_err());
        assert!(CdnListState::validate_host("   ").is_err());
        assert!(CdnListState::validate_host("upos-sz-mirror08c.bilivideo.com").is_ok());
    }

    #[test]
    fn provider_labels_map_through_the_catalog() {
        let catalog = services::i18n::catalog(services::i18n::ResolvedLanguage::En);
        assert_eq!(provider_label(catalog, "ALIYUN"), "Aliyun");
        assert_eq!(provider_label(catalog, "TENCENT"), "Tencent Cloud");
        assert_eq!(provider_label(catalog, "HUAWEI"), "Huawei Cloud");
        assert_eq!(provider_label(catalog, "CUSTOM"), "Custom");
        assert_eq!(provider_label(catalog, "UNKNOWN"), "UNKNOWN");
    }
}
