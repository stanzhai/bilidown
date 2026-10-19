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

use super::model::ParseViewModel;
use crate::ui::download_options;
use crate::ui::Shell;
use config::ConfigStore;
use gpui_kit::base::Disableable;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::tree::{Tree, TreeItem as DisplayItem, TreeState};
use gpui_kit::component::WindowExt;
use gpui_kit::{
    div, prelude::*, px, App, AppContext, Context, Entity, IntoElement, ScrollStrategy, Window,
};
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
    /// Side-data table of the most recent parse (`EpisodeData` global in
    /// Python); task creation reads it keyed by episode_id.
    pub episode_data: std::sync::Arc<parsers::EpisodeData>,
}
impl ParseScreen {
    /// Active catalog off the shell-global language.
    fn catalog(cx: &Context<Shell>) -> &'static services::i18n::Catalog {
        let lang = cx
            .read_global::<crate::ui::ShellI18n, _>(|i18n, _| i18n.lang)
            .resolve();
        services::i18n::catalog(lang)
    }

    pub fn new(window: &mut Window, cx: &mut Context<Shell>) -> Self {
        let c = Self::catalog(cx);
        let url = cx.new(|cx| {
            InputState::new(window, cx).placeholder(c.tr("ParseInterface", "Paste a Bilibili link"))
        });
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(c.tr("ParseInterface", "Search")));
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
            episode_data: std::sync::Arc::new(parsers::EpisodeData::new()),
        }
    }

    /// Render the screen body inside the shell.
    pub fn render(&mut self, cx: &mut Context<Shell>) -> impl IntoElement {
        let c = Self::catalog(cx);
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
                        Button::new("parse-button")
                            .label(c.tr("ParseInterface", "Parse"))
                            .on_click(cx.listener(|shell, _, _, cx| {
                                shell.parse.start_parse(&shell.auth_state, cx);
                            })),
                    )
                    .child(
                        Button::new("batch-parse-button")
                            .label(c.tr("ParseInterface", "Batch Parse"))
                            .on_click(cx.listener(|shell, _, window, cx| {
                                shell.parse.open_batch_parse_dialog(window, cx);
                            })),
                    )
                    .child(
                        Button::new("search-dialog-button")
                            .label(c.tr("ParseInterface", "Search…"))
                            .on_click(cx.listener(|shell, _, window, cx| {
                                shell.parse.open_search_dialog(window, cx);
                            })),
                    )
                    .child(
                        Button::new("history-button")
                            .label(c.tr("ParseInterface", "Parsing History"))
                            .on_click(cx.listener(|shell, _, window, cx| {
                                shell.parse.open_history_dialog(window, cx);
                            })),
                    )
                    .child(
                        Button::new("auto-parse-button")
                            .label(c.tr("ParseInterface", "Auto-parse Pagination"))
                            .on_click(cx.listener(|shell, _, window, cx| {
                                shell.parse.open_auto_parse_dialog(window, cx);
                            })),
                    )
                    .child(
                        Button::new("download-button")
                            .label(c.tr("ParseInterface", "Download Selected Items"))
                            .on_click(cx.listener(|shell, _, window, cx| {
                                shell.parse.on_download(window, shell.manager.clone(), cx);
                            })),
                    ),
            )
            .child(div().id("parse-status").child(self.status.clone()))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .w(px(220.))
                            .child(Input::new(&self.search).id("search-input")),
                    )
                    .child(
                        Button::new("search-button")
                            .label(c.tr("ParseInterface", "Search"))
                            .on_click(cx.listener(|shell, _, _, cx| {
                                let kw = shell.parse.search.read(cx).value().to_string();
                                shell.parse.run_search(Some(kw), cx);
                            })),
                    )
                    .child(Button::new("prev-match").label("↑").on_click(cx.listener(
                        |shell, _, _, cx| {
                            shell.parse.jump_match(false, cx);
                        },
                    )))
                    .child(Button::new("next-match").label("↓").on_click(cx.listener(
                        |shell, _, _, cx| {
                            shell.parse.jump_match(true, cx);
                        },
                    )))
                    .child(div().id("match-label").child(self.model.match_label(c))),
            )
            .child(div().id("count-label").child(self.model.count_label(c)))
            .child(self.render_tree(cx))
            .child(self.render_pager(cx))
    }

    /// Episode tree: one tri-state checkbox per row (the Python
    /// `parse_list` checkbox column — without it nothing can ever be
    /// checked, so Download Selected does nothing), plus the hover-bar
    /// actions as per-row suffix buttons: "View Multi-part Video List"
    /// (favorites items with multi-P videos) and "Download as Single
    /// Video" (leaf rows only, mirroring the hover bar hiding on
    /// parents).
    fn render_tree(&mut self, cx: &mut Context<Shell>) -> impl IntoElement {
        let tree = self.tree.clone();
        let shell = cx.weak_entity();
        // Row metadata in the same flatten order as `model.rows()`:
        // (url, leaf?, has multi-P, cover, row-id, checked?) per
        // display row.
        let mut row_meta: Vec<RowMeta> = Vec::new();
        collect_row_meta(self.model.list.root(), &mut row_meta);
        // `Tree::new` takes an `Fn('static)` — the metadata table moves
        // in behind an `Rc` so every render call clones the handle.
        let row_meta = std::rc::Rc::new(row_meta);
        div()
            .flex_1()
            .min_h_0()
            .child(Tree::new(&tree, move |ix, entry, _, _, _| {
                let meta = row_meta.get(ix).cloned();
                let meta_suffix = meta.clone();
                let checked = meta.as_ref().map(|m| m.checked).unwrap_or(false);
                let row_id = meta.as_ref().map(|m| m.id.clone()).unwrap_or_default();
                let shell_check = shell.clone();
                ListItem::new(("episode", ix))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .when_some(meta, |el, m| {
                                // Structural tree nodes (seasons/sections)
                                // aggregate their children but have no
                                // downloadable identity; only the row id
                                // targets a real item.
                                if m.id.is_empty() {
                                    return el;
                                }
                                el.child(
                                    Checkbox::new(("row-check", ix)).checked(checked).on_click({
                                        let row_id = row_id.clone();
                                        move |_, window, cx| {
                                            if let Some(shell) = shell_check.upgrade() {
                                                shell.update(cx, |shell, cx| {
                                                    shell.parse.model.toggle_row(&row_id);
                                                    shell.parse.refresh_tree(cx);
                                                    let _ = window;
                                                });
                                            }
                                        }
                                    }),
                                )
                            })
                            .child(entry.item().label.clone()),
                    )
                    .when_some(meta_suffix, |item, m| {
                        let (url, leaf, multi_part, cover) =
                            (m.url.clone(), m.leaf, m.multi_part, m.cover.clone());
                        if url.is_empty() {
                            return item;
                        }
                        let shell_part = shell.clone();
                        let shell_dl = shell.clone();
                        let shell_cover = shell.clone();
                        item.suffix(move |_, _| {
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    Button::new(("row-parts", ix))
                                        .label("≡")
                                        .disabled(!multi_part)
                                        .on_click({
                                            let shell_part = shell_part.clone();
                                            let url = url.clone();
                                            move |_, window, cx| {
                                                if let Some(shell) = shell_part.upgrade() {
                                                    shell.update(cx, |shell, cx| {
                                                        shell.parse.open_multi_part_lists_dialog(
                                                            url.clone(),
                                                            window,
                                                            cx,
                                                        );
                                                    });
                                                }
                                            }
                                        }),
                                )
                                .child(
                                    Button::new(("row-download", ix))
                                        .label("⬇")
                                        .disabled(!leaf)
                                        .on_click({
                                            let shell_dl = shell_dl.clone();
                                            let url = url.clone();
                                            move |_, window, cx| {
                                                if let Some(shell) = shell_dl.upgrade() {
                                                    shell.update(cx, |shell, cx| {
                                                        shell.parse.download_as_single_video(
                                                            url.clone(),
                                                            window,
                                                            shell.manager.clone(),
                                                            cx,
                                                        );
                                                    });
                                                }
                                            }
                                        }),
                                )
                                // Python gates the cover action on leaf rows
                                // (`tree_view.py:647` `item.count() == 0`).
                                .child(
                                    Button::new(("row-cover", ix))
                                        .label("🖼")
                                        .disabled(!leaf || cover.is_empty())
                                        .on_click({
                                            let shell_cover = shell_cover.clone();
                                            let cover = cover.clone();
                                            move |_, window, cx| {
                                                if let Some(shell) = shell_cover.upgrade() {
                                                    shell.update(cx, |_, cx| {
                                                        let cfg = ConfigStore::open()
                                                            .unwrap_or_else(|_| {
                                                                ConfigStore::with_paths("", "")
                                                            });
                                                        crate::ui::dialogs::view_cover::
                                                        open_view_cover(
                                                            cover.clone(), cfg, window, cx,
                                                        );
                                                    });
                                                }
                                            }
                                        }),
                                )
                        })
                    })
            }))
    }

    /// Prev / next / jump pager (port of `widget/pager.py`):
    /// `pageChanged` re-parses the current url at the new page.
    fn render_pager(&mut self, cx: &mut Context<Shell>) -> impl IntoElement {
        let c = Self::catalog(cx);
        let Some(p) = self.model.pagination.clone() else {
            return div();
        };
        div()
            .flex()
            .gap_2()
            .items_center()
            .child(
                Button::new("page-prev")
                    .label("‹")
                    .disabled(p.current_page <= 1)
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.parse.change_page(-1, shell.auth_state.clone(), cx);
                    })),
            )
            .child(
                div()
                    .id("page-label")
                    .child(format!("{}/{}", p.current_page, p.total_pages)),
            )
            .child(
                Button::new("page-next")
                    .label("›")
                    .disabled(p.current_page >= p.total_pages)
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.parse.change_page(1, shell.auth_state.clone(), cx);
                    })),
            )
            .child(Button::new("page-jump").label("…").on_click(cx.listener(
                |shell, _, window, cx| {
                    shell.parse.open_jump_to_page_dialog(window, cx);
                },
            )))
            .child(div().child(p.label(c)))
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
    pub fn start_parse(&mut self, auth: &auth::AuthState, cx: &mut Context<Shell>) {
        let url = self.url.read(cx).value().to_string();
        self.start_parse_with(url, 1, auth.clone(), cx);
    }

    /// Fill the input with a discovered URL (clipboard monitor) and parse.
    /// The input entity needs a window to redraw, which the poll lacks,
    /// so the URL is stored and applied on the next render click.
    pub fn start_parse_from_clipboard(
        &mut self,
        url: String,
        auth: &auth::AuthState,
        cx: &mut Context<Shell>,
    ) {
        self.start_parse_with(url, 1, auth.clone(), cx);
    }

    fn start_parse_with(
        &mut self,
        url: String,
        pn: i64,
        auth: auth::AuthState,
        cx: &mut Context<Shell>,
    ) {
        if url.trim().is_empty() {
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        self.model.reset();
        self.status = Self::catalog(cx)
            .tr("ParseInterface", "Parsing…")
            .to_string();
        self.refresh_tree(cx);

        // Parity with Python's parse worker: watch-later/history/lesson
        // families need the live login session. `auth` is passed in
        // because reading the Shell entity here would re-enter the
        // update closure the caller is already inside (gpui panics).
        // One side-data table across parses: the screen keeps the handle
        // so task creation can read the parsed extras.
        let c = Self::catalog(cx);
        let episode_data = self.episode_data.clone();
        cx.spawn(async move |shell, cx| {
            let _tokio = crate::bootstrap::tokio_context();
            let service = super::dispatch::ParseService::with_episode_data(
                &config::ConfigStore::open()?,
                episode_data,
            );
            let req = super::dispatch::ParseRequest {
                url: &url,
                pn,
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
                        shell.parse.record_history(cx);
                    }
                    Err(e) => {
                        shell.parse.status = crate::ui::dialogs::format_tr(
                            c.tr("ParseInterface", "Parse failed: {error}"),
                            &[("error", &e.to_string())],
                        );
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
        self.model.category_name = services::i18n::episode_type(Self::catalog(cx), output.category)
            .unwrap_or_else(|| output.category.to_string());
        if output.extra.pagination.is_some() {
            self.model.pagination = output
                .extra
                .pagination
                .as_ref()
                .map(super::model::PaginationState::from_extra);
        }
        self.model.server_search = output.extra.server_search;
        self.model.search_keyword = output.extra.keyword.clone();
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

    /// Python `on_update_parse_list` (`parse.py:363`): record the parse
    /// into history.db when `Behavior/parse_history` is on. Reads the
    /// URL from the input box (Python `url_box.text()`).
    pub fn record_history(&self, cx: &App) {
        let Ok(cfg) = config::ConfigStore::open() else {
            return;
        };
        let Some(dir) = dirs::data_dir().map(|d| d.join(config::APP_NAME)) else {
            return;
        };
        let Ok(db) = database::HistoryDatabase::open(dir.join("history.db")) else {
            return;
        };
        let url = self.url.read(cx).value().to_string();
        let title = self.model.list.root().title.clone();
        let _ = services::history::record(&db, &cfg, &title, &url, &self.model.category_name);
    }

    /// In-page search over the loaded tree.
    pub fn run_search(&mut self, keyword: Option<String>, cx: &mut Context<Shell>) {
        let c = Self::catalog(cx);
        let count = self.model.search(keyword.as_deref());
        self.status = self.model.match_label(c);
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
        self.status = self.model.match_label(Self::catalog(cx));
        if let Some(ix) = ix {
            self.reveal_row(ix, cx);
        }
        cx.notify();
    }

    fn reveal_row(&mut self, ix: usize, cx: &mut Context<Shell>) {
        if let Some(row) = self.model.rows().get(ix) {
            let id: gpui_kit::SharedString = row.id.clone().into();
            self.tree.update(cx, |tree, cx| {
                tree.reveal_item(&id, ScrollStrategy::Top, cx)
            });
        }
    }

    // ---- Dialog seams (tickets 21–26 implement these) ----

    pub fn open_batch_parse_dialog(&mut self, window: &mut Window, cx: &mut Context<Shell>) {
        let cfg =
            config::ConfigStore::open().unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
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
        let c = Self::catalog(cx);
        self.status = c.tr("ParseInterface", "Parsing…").to_string();
        self.refresh_tree(cx);
        cx.spawn(async move |shell, cx| {
            let _tokio = crate::bootstrap::tokio_context();
            let cfg = config::ConfigStore::open()
                .unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
            let service = super::dispatch::ParseService::new(&cfg);
            let base = super::dispatch::BASE_URL.to_string();
            let interval = std::time::Duration::from_secs_f32(parsers::batch::LINK_INTERVAL);
            let result =
                parsers::batch::parse_batch(&service.client, &base, &urls, |_, _| {}, interval)
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
                        shell.parse.status = crate::ui::dialogs::format_tr(
                            c.tr("ParseInterface", "Batch parse failed: {error}"),
                            &[("error", &e.to_string())],
                        );
                        cx.notify();
                    }
                }
            })
        })
        .detach();
    }

    /// Auto-parse pagination dialog accept: walk the chosen page range,
    /// appending every page's episodes into the current tree
    /// (`ProgressParseWorker` + the additive `append_nodes` walk).
    pub fn start_auto_parse(
        &mut self,
        payload: crate::ui::dialogs::auto_parse::AutoParsePayload,
        auth: auth::AuthState,
        cx: &mut Context<Shell>,
    ) {
        let c = Self::catalog(cx);
        let episode_data = self.episode_data.clone();
        self.status = c.tr("ParseInterface", "Preparing…").to_string();
        cx.notify();
        cx.spawn(async move |shell, cx| {
            let _tokio = crate::bootstrap::tokio_context();
            let service = std::sync::Arc::new(super::dispatch::ParseService::with_episode_data(
                &config::ConfigStore::open()
                    .unwrap_or_else(|_| config::ConfigStore::with_paths("", "")),
                episode_data,
            ));
            let client = service.client.clone();
            let url = std::sync::Arc::new(payload.url);
            let auth = std::sync::Arc::new(auth);
            let options = parsers::auto_parse::AutoParseOptions {
                start_page: payload.start_page,
                end_page: payload.end_page,
                interval: std::time::Duration::from_secs_f32(payload.interval as f32),
            };
            let result = parsers::auto_parse::walk_pages(
                &options,
                move |pn| {
                    let (client, service, url, auth) =
                        (client.clone(), service.clone(), url.clone(), auth.clone());
                    Box::pin(async move {
                        let req = super::dispatch::ParseRequest {
                            url: &url,
                            pn,
                            auth: &auth,
                        };
                        super::dispatch::parse(&client, &service.ctx, &req)
                            .await
                            .map_err(|e| parsers::list::ListError::Api(e.to_string()))
                    })
                },
                {
                    let cx = cx.clone();
                    let shell = shell.clone();
                    move |text, _page, _total| {
                        let text = text.to_owned();
                        cx.spawn({
                            let shell = shell.clone();
                            async move |cx| {
                                if let Some(shell) = shell.upgrade() {
                                    cx.update_entity(&shell, |shell: &mut Shell, cx| {
                                        shell.parse.status = text;
                                        cx.notify();
                                    });
                                }
                            }
                        })
                        .detach();
                    }
                },
                || false,
            )
            .await;
            shell.update(cx, |shell, cx| match result {
                Ok(outcome) => {
                    // The tree is additive: page one is already loaded,
                    // later pages append under the same category.
                    let _ = shell.parse.model.list.append_nodes(outcome.episodes);
                    shell.parse.refresh_tree(cx);
                    shell.parse.status = String::new();
                    cx.notify();
                }
                Err(e) => {
                    shell.parse.status = crate::ui::dialogs::format_tr(
                        c.tr("ParseInterface", "Auto-parse failed: {error}"),
                        &[("error", &e.to_string())],
                    );
                    cx.notify();
                }
            })
        })
        .detach();
    }

    pub fn open_search_dialog(&mut self, window: &mut Window, cx: &mut Context<Shell>) {
        let cfg =
            config::ConfigStore::open().unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
        let url = self.url.read(cx).value().to_string();
        let server_search_available = urls::search::support_search(&url);
        let current_keyword = urls::search::extract_keyword(&url).unwrap_or_default();
        let paginated = self.model.pagination.is_some();
        let shell = cx.entity();
        crate::ui::dialogs::search::open_search(
            cfg,
            server_search_available,
            &current_keyword,
            paginated,
            move |keyword, server, cx| {
                shell.update(cx, |shell, cx| {
                    if server {
                        let url = urls::search::build_search_url(
                            &shell.parse.url.read(cx).value(),
                            &keyword,
                        );
                        shell
                            .parse
                            .start_parse_with(url, 1, shell.auth_state.clone(), cx);
                    } else {
                        shell.parse.run_search(Some(keyword), cx);
                    }
                });
            },
            window,
            cx,
        );
    }

    pub fn open_batch_select_dialog(&mut self, window: &mut Window, cx: &mut Context<Shell>) {
        let cfg =
            config::ConfigStore::open().unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
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

    pub fn open_history_dialog(&mut self, window: &mut Window, cx: &mut Context<Shell>) {
        let cfg =
            config::ConfigStore::open().unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
        let shell = cx.entity();
        crate::ui::dialogs::parse_history::open_parse_history(
            cfg,
            move |url, cx| {
                shell.update(cx, |shell, cx| {
                    shell
                        .parse
                        .start_parse_from_clipboard(url, &shell.auth_state, cx);
                });
            },
            window,
            cx,
        );
    }

    pub fn open_auto_parse_dialog(&mut self, window: &mut Window, cx: &mut Context<Shell>) {
        let cfg =
            config::ConfigStore::open().unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
        let url = self.url.read(cx).value().to_string();
        let (current_page, total_pages) = self
            .model
            .pagination
            .as_ref()
            .map(|p| (p.current_page, p.total_pages))
            .unwrap_or((1, 1));
        let shell = cx.entity();
        crate::ui::dialogs::auto_parse::open_auto_parse(
            cfg,
            &url,
            current_page,
            total_pages,
            move |payload, cx| {
                shell.update(cx, |shell, cx| {
                    shell
                        .parse
                        .start_auto_parse(payload, shell.auth_state.clone(), cx);
                });
            },
            window,
            cx,
        );
    }

    /// Open the interactive-video dialog when a walked stein tree is
    /// parked; otherwise record the intent.
    pub fn open_interactive_video_dialog(&mut self, window: &mut Window, cx: &mut Context<Shell>) {
        let c = Self::catalog(cx);
        let Some(node) = self.pending_interactive_node.take() else {
            self.pending_dialog = Some("interactive_video");
            return;
        };
        let cfg =
            config::ConfigStore::open().unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
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
                    shell.parse.model.category_name =
                        services::i18n::episode_type(c, output.category)
                            .unwrap_or_else(|| output.category.to_string());
                    shell.parse.model.pagination = None;
                    shell.parse.model.server_search = false;
                    let _ = shell
                        .parse
                        .model
                        .list
                        .update_tree(output.node.clone(), output.current_episode.clone());
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
        let store = RuleStore::from_config_value(cfg.get("File Naming", "naming_rule_list").as_ref());
        let candidates = self.model.preview_items(3);
        download_options::open_download_options(
            window, cx, &cfg, &info, &type_ids, &store, candidates,
        );
    }

    /// `on_download` (F8 UI story): checked episodes → task rows via
    /// `TaskManager::create`; the bootstrap driver picks them up from
    /// QUEUED and runs the full pipeline.
    pub fn on_download(
        &mut self,
        window: &mut Window,
        manager: Option<std::sync::Arc<tasks::TaskManager>>,
        cx: &mut Context<Shell>,
    ) {
        // Python `on_download`: the options dialog runs first when the
        // behavior flag is on; its OK handler commits settings, task
        // creation then proceeds for the still-checked rows.
        let c = Self::catalog(cx);
        if let Ok(cfg) = ConfigStore::open() {
            let show = cfg
                .get("Behavior", "show_download_options_dialog")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if show {
                self.open_download_options_dialog(window, cx);
            }
        }
        let Some(manager) = manager else {
            self.status = c
                .tr("ParseInterface", "Downloads unavailable (no task database)")
                .to_string();
            cx.notify();
            return;
        };
        let episodes: Vec<serde_json::Map<String, serde_json::Value>> = self
            .model
            .checked_dicts_mark_downloaded()
            .into_iter()
            .filter_map(|v| v.as_object().cloned())
            .collect();
        if episodes.is_empty() {
            return;
        }
        let episode_data = self.episode_data.clone();
        let created = {
            let Ok(cfg) = ConfigStore::open() else { return };
            manager.create(&cfg, &episode_data, &episodes, None, |_| {
                tasks::info::NamingInput::default()
            })
        };
        match created {
            Ok(result) => {
                self.status = format!(
                    "{}{}",
                    crate::ui::dialogs::format_tr(
                        c.tr("ParseInterface", "Queued {count} task(s)"),
                        &[("count", &result.created.len().to_string())],
                    ),
                    if result.skipped_duplicates.is_empty() {
                        String::new()
                    } else {
                        crate::ui::dialogs::format_tr(
                            c.tr("ParseInterface", " ({count} duplicate(s) skipped)"),
                            &[("count", &result.skipped_duplicates.len().to_string())],
                        )
                    }
                );
                // Leave checked rows visibly downloaded.
                self.refresh_tree(cx);
            }
            Err(e) => {
                self.status = crate::ui::dialogs::format_tr(
                    c.tr("ParseInterface", "Task creation failed: {error}"),
                    &[("error", &e.to_string())],
                );
            }
        }
        cx.notify();
        let _ = window;
    }

    // ---- Pager actions (widget/pager.py) ----

    /// `on_change_page` via prev/next: clamp to 1..=total, then
    /// re-parse the current url at the new page (`pageChanged` →
    /// `on_parse(page)` reads `url_box.text()`).
    pub fn change_page(&mut self, delta: i64, auth: auth::AuthState, cx: &mut Context<Shell>) {
        let Some(p) = self.model.pagination.clone() else {
            return;
        };
        let page = (p.current_page + delta).clamp(1, p.total_pages);
        if page == p.current_page {
            return;
        }
        self.go_to_page(page, auth, cx);
    }

    /// `on_jump_to_page`'s accept path: validate against the live
    /// bounds (out of range → warning notification) and re-parse.
    pub fn go_to_page(&mut self, page: i64, auth: auth::AuthState, cx: &mut Context<Shell>) {
        let Some(p) = self.model.pagination.clone() else {
            return;
        };
        if !(1..=p.total_pages).contains(&page) {
            let c = Self::catalog(cx);
            let body = crate::ui::dialogs::format_tr(
                c.tr("Pager", "Please enter a number between 1 and {total_pages}"),
                &[("total_pages", &p.total_pages.to_string())],
            );
            let t = c.tr("ParseInterface", "Invalid page number: {body}");
            self.status = t.replace("{body}", &body);
            cx.notify();
            return;
        }
        let url = self.url.read(cx).value().to_string();
        self.start_parse_with(url, page, auth, cx);
    }

    /// Jump ("…") button: open the Jump To Page dialog and re-parse on
    /// accept (`on_jump_to_page`).
    pub fn open_jump_to_page_dialog(&mut self, window: &mut Window, cx: &mut Context<Shell>) {
        let cfg = ConfigStore::open().unwrap_or_else(|_| ConfigStore::with_paths("", ""));
        let shell = cx.entity();
        crate::ui::dialogs::jump_to_page::open_jump_to_page(
            cfg,
            move |page, cx| {
                shell.update(cx, |shell, cx| {
                    shell.parse.go_to_page(page, shell.auth_state.clone(), cx)
                });
            },
            window,
            cx,
        );
    }

    // ---- Hover-bar actions (tree_view.py) ----

    /// `_on_hover_download` → `on_download_as_single_video`: optional
    /// confirm dialog, then mark the row downloaded and queue the one
    /// episode as a task. `url` locates the row (per-row button).
    pub fn download_as_single_video(
        &mut self,
        url: String,
        window: &mut Window,
        manager: Option<std::sync::Arc<tasks::TaskManager>>,
        cx: &mut Context<Shell>,
    ) {
        let Some(item) = find_item_by_url(self.model.list.root_mut(), &url) else {
            return;
        };
        item.set_attribute(parsers::tree::Attribute::DOWNLOAD_AS_SINGLE_VIDEO_BIT);
        let dict = item.to_dict();
        if !item.bvid.is_empty() {
            self.model.downloaded.insert(item.bvid.clone());
        }
        // manager passed in — no re-entrant Shell read
        let episode_data = self.episode_data.clone();
        // Queue the one episode (Python marks the item and emits
        // create_task([dict], True, None) regardless of dialog result).
        let c = Self::catalog(cx);
        let queue = move |shell: &mut Shell, cx: &mut Context<Shell>| {
            let Some(manager) = manager else {
                cx.notify();
                return;
            };
            let episodes: Vec<serde_json::Map<String, serde_json::Value>> =
                dict.as_object().cloned().into_iter().collect();
            if let Ok(cfg) = ConfigStore::open() {
                match manager.create(&cfg, &episode_data, &episodes, None, |_| {
                    tasks::info::NamingInput::default()
                }) {
                    Ok(result) => {
                        shell.parse.status = crate::ui::dialogs::format_tr(
                            c.tr("ParseInterface", "Queued {count} task(s)"),
                            &[("count", &result.created.len().to_string())],
                        );
                    }
                    Err(e) => {
                        shell.parse.status = crate::ui::dialogs::format_tr(
                            c.tr("ParseInterface", "Task creation failed: {error}"),
                            &[("error", &e.to_string())],
                        );
                    }
                }
            }
            shell.parse.refresh_tree(cx);
        };
        let shell = cx.entity();
        if let Ok(cfg) = ConfigStore::open() {
            if crate::ui::dialogs::download_as_single_video::show_dialog_flag(&cfg) {
                crate::ui::dialogs::download_as_single_video::open_download_as_single_video(
                    cfg,
                    {
                        let shell = shell.clone();
                        move |cx| {
                            shell.update(cx, queue);
                        }
                    },
                    window,
                    cx,
                );
                return;
            }
        }
        shell.update(cx, queue);
    }

    /// `_on_hover_view_part_list` → `on_show_multi_part_lists_dialog`:
    /// re-parse the item's url with `ugc_season` stripped (so the 分P
    /// list parser runs), then show the selection dialog; accept queues
    /// the checked parts with numbering reset to 1.
    pub fn open_multi_part_lists_dialog(
        &mut self,
        url: String,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) {
        let c = Self::catalog(cx);
        let episode_data = self.episode_data.clone();
        let shell = cx.entity();
        window
            .spawn(cx, async move |cx: &mut gpui_kit::AsyncWindowContext| {
                let cfg = ConfigStore::open().unwrap_or_else(|_| ConfigStore::with_paths("", ""));
                let service =
                    super::dispatch::ParseService::with_episode_data(&cfg, episode_data.clone());
                // Python's ParseWorker: view fetch, drop ugc_season so
                // the 分P list parser runs, parse the pages tree.
                let Some(bvid) = parsers::media_parser::video_bvid(&url) else {
                    return;
                };
                let mut data = match parsers::media_parser::get_video_info(
                    &service.client,
                    super::dispatch::BASE_URL,
                    &bvid,
                    &url,
                )
                .await
                {
                    Ok(data) => data,
                    Err(e) => {
                        let msg = e.to_string();
                        let _ = cx.update(|window, cx| {
                            window.push_notification(
                                gpui_kit::component::notification::Notification::error(msg),
                                cx,
                            );
                        });
                        return;
                    }
                };
                if let Some(obj) = data.as_object_mut() {
                    obj.remove("ugc_season");
                }
                let episode_id = episode_data.add_episode();
                let (node, _) =
                    parsers::episode::video::parse_video(&episode_data, &episode_id, &data);
                let rows = crate::ui::dialogs::multi_part_lists::MultiPartState::from_children(
                    &node.children,
                )
                .rows;
                let cfg2 = ConfigStore::open().unwrap_or_else(|_| ConfigStore::with_paths("", ""));
                let _ = cx.update(move |window, cx| {
                    crate::ui::dialogs::multi_part_lists::open_multi_part_lists(
                        cfg2,
                        rows,
                        move |episodes, cx| {
                            shell.update(cx, |shell, cx| {
                                let Some(manager) = shell.manager.clone() else {
                                    shell.parse.status = c
                                        .tr(
                                            "ParseInterface",
                                            "Downloads unavailable (no task database)",
                                        )
                                        .to_string();
                                    cx.notify();
                                    return;
                                };
                                manager.set_current_starting_number(1);
                                let episode_data = shell.parse.episode_data.clone();
                                if let Ok(cfg) = ConfigStore::open() {
                                    match manager.create(
                                        &cfg,
                                        &episode_data,
                                        &episodes,
                                        None,
                                        |_| tasks::info::NamingInput::default(),
                                    ) {
                                        Ok(result) => {
                                            shell.parse.status = format!(
                                                "{}{}",
                                                crate::ui::dialogs::format_tr(
                                                    c.tr(
                                                        "ParseInterface",
                                                        "Queued {count} task(s)",
                                                    ),
                                                    &[("count", &result.created.len().to_string())],
                                                ),
                                                if result.skipped_duplicates.is_empty() {
                                                    String::new()
                                                } else {
                                                    crate::ui::dialogs::format_tr(
                                                        c.tr(
                                                            "ParseInterface",
                                                            " ({count} duplicate(s) skipped)",
                                                        ),
                                                        &[(
                                                            "count",
                                                            &result.skipped_duplicates.len().to_string(),
                                                        )],
                                                    )
                                                }
                                            );
                                        }
                                        Err(e) => {
                                            shell.parse.status =
                                                crate::ui::dialogs::format_tr(
                                                    c.tr(
                                                        "ParseInterface",
                                                        "Task creation failed: {error}",
                                                    ),
                                                    &[("error", &e.to_string())],
                                                );
                                        }
                                    }
                                    cx.notify();
                                }
                            });
                        },
                        window,
                        cx,
                    );
                });
            })
            .detach();
    }
}

/// Per-row metadata snapshot for the tree render (same DFS order as
/// `ParseViewModel::rows`): suffix-button config plus checkbox state.
#[derive(Clone)]
struct RowMeta {
    url: String,
    leaf: bool,
    multi_part: bool,
    cover: String,
    /// Stable row id (`model::row_id`) — the toggle key.
    id: String,
    checked: bool,
}

fn collect_row_meta(item: &parsers::tree::TreeItem, out: &mut Vec<RowMeta>) {
    // Children only: `model::flatten` skips the invisible wrapper root,
    // so the metadata table must too or every row is off by one
    // (ticket 32 item 3).
    for child in &item.children {
        let id = super::model::display_row_id(child);
        out.push(RowMeta {
            url: child.url.clone(),
            leaf: child.children.is_empty(),
            multi_part: child
                .has_attribute(parsers::tree::Attribute::FAVORITE_WITH_MULTI_PART_VIDEO_BIT),
            cover: child.cover.clone(),
            checked: matches!(child.checked, parsers::tree::CheckState::Checked),
            id,
        });
        collect_row_meta(child, out);
    }
}

/// Find the tree item whose url matches (the hover-bar actions key on
/// the row's url), depth-first like the row snapshot.
fn find_item_by_url<'a>(
    item: &'a mut parsers::tree::TreeItem,
    url: &str,
) -> Option<&'a mut parsers::tree::TreeItem> {
    if item.url == url {
        return Some(item);
    }
    item.children
        .iter_mut()
        .find_map(|child| find_item_by_url(child, url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use parsers::tree::{Number, TreeItem};

    fn item(bvid: &str, number: i64, title: &str) -> TreeItem {
        let mut it = TreeItem::empty();
        it.bvid = bvid.into();
        it.title = title.into();
        it.number = Number::Int(number);
        it
    }

    /// Ticket 32 item 3: the per-row metadata table must line up with
    /// `model.rows()` one-for-one — same length, same order, same
    /// leaf-ness — or every checkbox/action lands on the wrong row.
    #[test]
    fn collect_row_meta_matches_display_rows() {
        // Bangumi shape: wrapper root > season > episodes.
        let mut root = TreeItem::empty();
        let mut season = TreeItem::root(Number::Text("正片".into()), "正片");
        season.add_child(item("BV1", 1, "EP1"));
        season.add_child(item("BV2", 2, "EP2"));
        root.add_child(season);
        let mut plain = TreeItem::root(Number::Text("收藏".into()), "收藏");
        plain.add_child(item("BV3", 1, "P1"));
        root.add_child(plain);

        let mut model = super::super::model::ParseViewModel::new();
        model.list.update_tree(root, None).unwrap();
        model.refresh_rows();

        let mut meta = Vec::new();
        collect_row_meta(model.list.root(), &mut meta);
        let rows = model.rows();

        assert_eq!(
            meta.len(),
            rows.len(),
            "row_meta and display rows must be parallel arrays"
        );
        for (ix, (m, r)) in meta.iter().zip(rows).enumerate() {
            assert_eq!(m.id, r.id, "row {ix} id mismatch");
            assert_eq!(m.leaf, r.is_leaf, "row {ix} leaf mismatch");
        }
    }
}
