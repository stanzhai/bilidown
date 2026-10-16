//! Parse screen entity: URL bar, Parse button with progress, the
//! Episode tree (checkbox rows), pager, in-page search bar with match
//! jumping (port of `src/gui/interface/parse.py` and the parse-list
//! widget trio).
//!
//! The screen owns a [`ParseViewModel`]; parse work runs through
//! [`super::dispatch`] (spawned by the shell when a parse completes; the
//! headless tests inject a canned tree directly). Dialog-opening methods
//! are named seams: tickets 21–26 hang their dialogs off them; until
//! then they record what they'd open in `pending_dialog`.

use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::tree::{Tree, TreeItem as DisplayItem, TreeState};
use gpui_kit::base::Disableable;
use gpui_kit::{AppContext, Context, Entity, IntoElement, ScrollStrategy, Window, div, px, prelude::*};
use super::model::ParseViewModel;
use crate::ui::download_options;
use crate::ui::Shell;
use config::ConfigStore;
use naming::naming_rules::RuleStore;

pub struct ParseScreen {
    pub url: Entity<InputState>,
    pub search: Entity<InputState>,
    pub model: ParseViewModel,
    pub tree: Entity<TreeState>,
    /// Parse progress line (PARSING / WBI_KEY_WAITING / page counts).
    pub status: String,
    /// Dialog seam the next ticket implements (`(name, payload)`).
    pub pending_dialog: Option<&'static str>,
    /// Walked stein tree awaiting the interactive dialog's decision.
    pub pending_interactive_node: Option<parsers::tree::TreeItem>,
    /// Monotonic token: results from an older parse are dropped.
    generation: u64,
}

impl ParseScreen {
    pub fn new(window: &mut Window, cx: &mut Context<Shell>) -> Self {
        let url = cx.new(|cx| InputState::new(window, cx).placeholder("Paste a Bilibili link"));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let tree = cx.new(|cx| TreeState::new(cx));
        Self {
            url,
            search,
            model: ParseViewModel::new(),
            tree,
            status: String::new(),
            pending_dialog: None,
            pending_interactive_node: None,
            generation: 0,
        }
    }

    /// Render the screen body inside the shell.
    pub fn render(&mut self, cx: &mut Context<Shell>) -> impl IntoElement {
        div()
            .id("parse-screen")
            .flex()
            .flex_col()
            .size_full()
            .p_4()
            .gap_3()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.url).id("url-input")))
                    .child(
                        Button::new("parse-button").label("Parse").on_click(cx.listener(
                            |shell, _, _, cx| {
                                shell.parse.start_parse(cx);
                            },
                        )),
                    )
                    .child(
                        Button::new("batch-parse-button")
                            .label("Batch Parse")
                            .on_click(cx.listener(
                                |shell, _, window, cx| {
                                    shell.parse.open_batch_parse_dialog(window, cx);
                                },
                            )),
                    )
                    .child(
                        Button::new("batch-select-button")
                            .label("Batch Select")
                            .on_click(cx.listener(
                                |shell, _, window, cx| {
                                    shell.parse.open_batch_select_dialog(window, cx);
                                },
                            )),
                    ),
            )
            .child(div().id("parse-status").child(self.status.clone()))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(div().w(px(220.)).child(Input::new(&self.search).id("search-input")))
                    .child(
                        Button::new("search-button").label("Search").on_click(cx.listener(
                            |shell, _, _, cx| {
                                let kw = shell.parse.search.read(cx).value().to_string();
                                shell.parse.run_search(Some(kw), cx);
                            },
                        )),
                    )
                    .child(
                        Button::new("prev-match").label("↑").on_click(cx.listener(
                            |shell, _, _, cx| {
                                shell.parse.jump_match(false, cx);
                            },
                        )),
                    )
                    .child(
                        Button::new("next-match").label("↓").on_click(cx.listener(
                            |shell, _, _, cx| {
                                shell.parse.jump_match(true, cx);
                            },
                        )),
                    )
                    .child(div().id("match-label").child(self.model.match_label())),
            )
            .child(div().id("count-label").child(self.model.count_label()))
            .child(self.render_tree())
            .child(self.render_pager())
    }

    fn render_tree(&mut self) -> impl IntoElement {
        let tree = self.tree.clone();
        div().flex_1().min_h_0().child(Tree::new(&tree, |ix, entry, _, _, _| {
            ListItem::new(("episode", ix)).child(entry.item().label.clone())
        }))
    }

    fn render_pager(&mut self) -> impl IntoElement {
        let Some(p) = self.model.pagination.clone() else {
            return div();
        };
        div()
            .flex()
            .gap_2()
            .items_center()
            .child(Button::new("page-prev").label("‹").disabled(p.current_page <= 1))
            .child(
                div()
                    .id("page-label")
                    .child(format!("{}/{}", p.current_page, p.total_pages)),
            )
            .child(
                Button::new("page-next")
                    .label("›")
                    .disabled(p.current_page >= p.total_pages),
            )
            .child(div().child(p.label()))
    }

    /// Sync display rows → tree items and notify.
    pub fn refresh_tree(&mut self, cx: &mut Context<Shell>) {
        self.model.refresh_rows();
        let items: Vec<DisplayItem> = self
            .model
            .rows()
            .iter()
            .map(|row| DisplayItem::new(row.id.clone(), row.label.clone()))
            .collect();
        self.tree.update(cx, |tree, cx| tree.set_items(items, cx));
        cx.notify();
    }

    /// Parse button handler: reset state, spawn the real parse on the
    /// background runtime, drop stale results by generation token.
    pub fn start_parse(&mut self, cx: &mut Context<Shell>) {
        let url = self.url.read(cx).value().to_string();
        if url.trim().is_empty() {
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        self.model.reset();
        self.status = "Parsing…".to_string();
        self.refresh_tree(cx);

        let auth = auth::AuthState::default();
        cx.spawn(async move |shell, cx| {
            let service = super::dispatch::ParseService::new(&config::ConfigStore::open()?);
            let req = super::dispatch::ParseRequest {
                url: &url,
                pn: 1,
                auth: &auth,
            };
            // WBI-signed families wait for the nav keys (bounded).
            if service.needs_wbi(&url) == Some(true) && !service.key_store.ready() {
                let client = service.client.clone();
                let base = super::dispatch::BASE_URL.to_string();
                let _ = service
                    .wait_for_wbi_keys(move || {
                        let client = client.clone();
                        let base = base.clone();
                        async move {
                            let resp: serde_json::Value = client
                                .get_json(&format!("{base}/x/web-interface/nav"), &[])
                                .await
                                .map_err(|e| e.to_string())?;
                            Ok(resp)
                        }
                    })
                    .await;
            }
            let result = super::dispatch::parse(&service.client, &service.ctx, &req).await;
            shell.update(cx, |shell, cx| {
                if shell.parse.generation != generation {
                    return; // a newer parse superseded this one
                }
                match result {
                    Ok(output) => {
                        shell.parse.apply_parse_result(&output, cx);
                        // open() 失败就落到默认配置（with_paths 构建默认值）
                        let cfg = config::ConfigStore::open()
                            .unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
                        shell.parse.model.apply_auto_select(&cfg);
                    }
                    Err(e) => {
                        shell.parse.status = format!("Parse failed: {e}");
                        cx.notify();
                    }
                }
            })
        })
        .detach();
    }

    /// Apply one parse output to the screen (tree + pager + auto-select).
    pub fn apply_parse_result(
        &mut self,
        output: &parsers::list::ParseOutput,
        cx: &mut Context<Shell>,
    ) {
        self.model.category_key = output.category.to_string();
        self.model.category_name = output.category.to_string();
        if output.extra.pagination.is_some() {
            self.model.pagination = output
                .extra
                .pagination
                .as_ref()
                .map(super::model::PaginationState::from_extra);
        }
        self.model.server_search = output.extra.server_search;
        if output.category == "INTERACTIVE_VIDEO" {
            // The walked stein tree is parked; the interactive dialog
            // decides on accept whether to present it.
            self.pending_interactive_node = Some(output.node.clone());
            self.status = String::new();
            cx.notify();
            return;
        }
        let _ = self
            .model
            .list
            .update_tree(output.node.clone(), output.current_episode.clone());
        self.refresh_tree(cx);
        self.status = String::new();
    }

    /// In-page search over the loaded tree.
    pub fn run_search(&mut self, keyword: Option<String>, cx: &mut Context<Shell>) {
        let count = self.model.search(keyword.as_deref());
        self.status = self.model.match_label();
        if count > 0 {
            if let Some(ix) = self.model.matches.first().copied() {
                self.reveal_row(ix, cx);
            }
        }
        cx.notify();
    }

    /// Jump to the next/previous match, scrolling it into view.
    pub fn jump_match(&mut self, forward: bool, cx: &mut Context<Shell>) {
        let ix = if forward {
            self.model.next_match()
        } else {
            self.model.prev_match()
        };
        self.status = self.model.match_label();
        if let Some(ix) = ix {
            self.reveal_row(ix, cx);
        }
        cx.notify();
    }

    fn reveal_row(&mut self, ix: usize, cx: &mut Context<Shell>) {
        if let Some(row) = self.model.rows().get(ix) {
            let id: gpui_kit::SharedString = row.id.clone().into();
            self.tree
                .update(cx, |tree, cx| tree.reveal_item(&id, ScrollStrategy::Top, cx));
        }
    }

    // ---- Dialog seams (tickets 21–26 implement these) ----

    pub fn open_batch_parse_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) {
        let cfg = config::ConfigStore::open()
            .unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
        let shell = cx.entity();
        crate::ui::dialogs::batch_parse::open_batch_parse(
            cfg,
            move |urls, _auto_add, cx| {
                shell.update(cx, |shell, cx| {
                    shell.parse.start_batch_parse(urls, cx);
                });
            },
            window,
            cx,
        );
    }

    /// Batch-parse dialog accept: fold every pasted link into one tree via
    /// `parsers::batch::parse_batch` and present it as a video parse.
    pub fn start_batch_parse(&mut self, urls: Vec<String>, cx: &mut Context<Shell>) {
        self.generation += 1;
        let generation = self.generation;
        self.model.reset();
        self.status = "Parsing…".to_string();
        self.refresh_tree(cx);
        cx.spawn(async move |shell, cx| {
            let cfg = config::ConfigStore::open()
                .unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
            let service = super::dispatch::ParseService::new(&cfg);
            let base = super::dispatch::BASE_URL.to_string();
            let interval = std::time::Duration::from_secs_f32(parsers::batch::LINK_INTERVAL);
            let result = parsers::batch::parse_batch(
                &service.client,
                &base,
                &urls,
                |_, _| {},
                interval,
            )
            .await;
            shell.update(cx, |shell, cx| {
                if shell.parse.generation != generation {
                    return; // superseded by a newer parse
                }
                match result {
                    Ok(node) => {
                        let output = parsers::list::ParseOutput {
                            category: "video",
                            node,
                            extra: parsers::list::ExtraData::default(),
                            current_episode: None,
                        };
                        shell.parse.apply_parse_result(&output, cx);
                    }
                    Err(e) => {
                        shell.parse.status = format!("Batch parse failed: {e}");
                        cx.notify();
                    }
                }
            })
        })
        .detach();
    }

    pub fn open_search_dialog(&mut self, _: &mut Context<Shell>) {
        self.pending_dialog = Some("search");
    }

    pub fn open_batch_select_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) {
        let cfg = config::ConfigStore::open()
            .unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
        let shell = cx.entity();
        crate::ui::dialogs::batch_select::open_batch_select(
            cfg,
            move |numbers, cx| {
                shell.update(cx, |shell, cx| {
                    shell.parse.model.list.check_numbers(&numbers);
                    shell.parse.refresh_tree(cx);
                });
            },
            window,
            cx,
        );
    }

    pub fn open_history_dialog(&mut self, _: &mut Context<Shell>) {
        self.pending_dialog = Some("history");
    }

    pub fn open_auto_parse_dialog(&mut self, _: &mut Context<Shell>) {
        self.pending_dialog = Some("auto_parse");
    }

    /// Open the interactive-video dialog when a walked stein tree is
    /// parked; otherwise record the intent.
    pub fn open_interactive_video_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) {
        let Some(node) = self.pending_interactive_node.take() else {
            self.pending_dialog = Some("interactive_video");
            return;
        };
        let cfg = config::ConfigStore::open()
            .unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
        let shell = cx.entity();
        crate::ui::dialogs::interactive::open_interactive_video(
            serde_json::Value::Null,
            cfg,
            move |payload, cx| {
                shell.update(cx, |shell, cx| {
                    // The parked tree already holds the stein graph;
                    // `data` (entry-node view) is only diagnostic.
                    let _ = &payload.data;
                    if !payload.auto_parse {
                        return; // only "parse all nodes" exists upstream
                    }
                    let output = parsers::list::ParseOutput {
                        category: "INTERACTIVE_VIDEO",
                        node,
                        extra: parsers::list::ExtraData::default(),
                        current_episode: None,
                    };
                    shell.parse.model.category_key = output.category.to_string();
                    shell.parse.model.category_name = output.category.to_string();
                    shell.parse.model.pagination = None;
                    shell.parse.model.server_search = false;
                    let _ = shell.parse.model.list.update_tree(
                        output.node.clone(),
                        output.current_episode.clone(),
                    );
                    shell.parse.refresh_tree(cx);
                    shell.parse.status = String::new();
                });
            },
            window,
            cx,
        );
    }

    pub fn open_download_options_dialog(&mut self, window: &mut Window, cx: &mut Context<Shell>) {
        // `get_checked_type_ids` — one naming-rule selector per checked
        // content type.
        let attributes: Vec<i64> = self
            .model
            .list
            .get_checked_items()
            .iter()
            .map(|item| item.attribute.0 as i64)
            .collect();
        let type_ids = download_options::DownloadOptionsModel::checked_type_ids(&attributes);

        let info = self.model.preview_info();
        let cfg = ConfigStore::open().unwrap_or_else(|_| ConfigStore::with_paths("", ""));
        let store = RuleStore::default();
        download_options::open_download_options(window, cx, &cfg, &info, &type_ids, &store);
    }
}
