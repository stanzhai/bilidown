//! Search dialog (port of `src/gui/dialog/misc/search.py`).
//!
//! Server-search families (space/favlist/history/watch-later) offer a
//! scope choice: filter the loaded page locally, or re-parse with the
//! keyword baked into the url (`urls::search::build_search_url`). A
//! blank keyword is valid in server mode (it removes the param and
//! restores the full list); locally it is a validation error.

use gpui_kit::component::button::Button;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::label::Label;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, px, AppContext, ParentElement, Window};

use super::active_catalog;

/// i18n context of the Python dialog class.
const CTX: &str = "SearchDialog";

/// Headless view-model: keyword box + scope radios.
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    pub keywords: String,
    /// Server search available for the current url family.
    pub server_search_available: bool,
    /// Scope: true = search all pages (server), false = filter page.
    pub search_all: bool,
    /// Pagination present but no server search → page-filter tip.
    pub paginated: bool,
}

impl SearchState {
    /// Python ctor defaults: `search_all` mirrors the keyword already in
    /// the url, so a follow-up edit keeps the server scope.
    pub fn new(server_search_available: bool, current_keyword: &str, paginated: bool) -> Self {
        Self {
            keywords: current_keyword.to_owned(),
            server_search_available,
            search_all: server_search_available && !current_keyword.is_empty(),
            paginated,
        }
    }

    /// `use_server_search()`.
    pub fn use_server_search(&self) -> bool {
        self.server_search_available && self.search_all
    }

    /// `validate()`: server mode always passes (blank = clear keyword);
    /// local filter needs a non-blank keyword.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.use_server_search() {
            return Ok(());
        }
        if self.keywords.trim().is_empty() {
            return Err("Please enter search keywords");
        }
        Ok(())
    }

    /// `accept()`: the (trimmed keyword, server-search) pair.
    pub fn accept(&self) -> (String, bool) {
        (self.keywords.trim().to_owned(), self.use_server_search())
    }
}

/// Open the dialog; `on_accept` gets the keyword and whether the server
/// search scope was chosen.
pub fn open_search(
    cfg: config::ConfigStore,
    server_search_available: bool,
    current_keyword: &str,
    paginated: bool,
    on_accept: impl FnOnce(String, bool, &mut gpui_kit::App) + 'static,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let catalog = active_catalog(&cfg);
    let title = catalog.tr(CTX, "Search").to_owned();
    let placeholder = catalog.tr(CTX, "Enter keywords to search").to_owned();
    let scope = catalog.tr(CTX, "Search scope").to_owned();
    let filter_page = catalog.tr(CTX, "Filter the current page only").to_owned();
    let search_all = catalog.tr(CTX, "Search all pages").to_owned();
    let page_tip = catalog.tr(
        CTX,
        "Only the current page can be filtered. To search the full list, parse all pages first.",
    )
    .to_owned();
    let ok = "OK".to_owned();

    let input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(current_keyword)
    });
    let state = std::rc::Rc::new(std::cell::RefCell::new(SearchState::new(
        server_search_available,
        current_keyword,
        paginated,
    )));
    let on_accept = std::rc::Rc::new(std::cell::RefCell::new(Some(on_accept)));

    window.open_dialog(cx, move |dialog, _, _| {
        let st = state.borrow();
        let initial_all = st.search_all;
        let show_scope = st.server_search_available;
        let show_tip = !st.server_search_available && st.paginated;
        drop(st);
        dialog
            .title(title.clone())
            .w(px(450.))
            .content({
                let state = state.clone();
                let input = input.clone();
                let scope = scope.clone();
                let filter_page = filter_page.clone();
                let search_all = search_all.clone();
                let page_tip = page_tip.clone();
                move |content, _, _| {
                    let mut col = div().flex().flex_col().gap_2();
                    col = col.child(Input::new(&input).id("keywords"));
                    if show_scope {
                        let selected = if initial_all { 1 } else { 0 };
                        col = col.child(Label::new(scope.clone())).child(
                            RadioGroup::new("scope")
                                .selected_index(Some(selected))
                                .child(
                                    Radio::new("filter-page")
                                        .checked(!initial_all)
                                        .label(filter_page.clone())
                                        .on_click({
                                            let state = state.clone();
                                            move |checked, _, _| {
                                                if *checked {
                                                    state.borrow_mut().search_all = false;
                                                }
                                            }
                                        }),
                                )
                                .child(
                                    Radio::new("search-all")
                                        .checked(initial_all)
                                        .label(search_all.clone())
                                        .on_click({
                                            let state = state.clone();
                                            move |checked, _, _| {
                                                if *checked {
                                                    state.borrow_mut().search_all = true;
                                                }
                                            }
                                        }),
                                ),
                        );
                    } else if show_tip {
                        col = col.child(Label::new(page_tip.clone()));
                    }
                    content.child(col)
                }
            })
            .footer(
                DialogFooter::new().child(Button::new("confirm").label(ok.clone()).on_click({
                    let state = state.clone();
                    let input = input.clone();
                    let on_accept = on_accept.clone();
                    move |_, window, cx| {
                        let keywords = input.read(cx).value().to_string();
                        state.borrow_mut().keywords = keywords;
                        let st = state.borrow();
                        if let Err(msg) = st.validate() {
                            let msg = catalog.tr(CTX, msg).to_owned();
                            drop(st);
                            window.push_notification(Notification::error(msg), cx);
                            return;
                        }
                        let (keywords, server) = st.accept();
                        drop(st);
                        window.close_dialog(cx);
                        if let Some(cb) = on_accept.borrow_mut().take() {
                            cb(keywords, server, cx);
                        }
                    }
                })),
            )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_defaults_to_server_only_when_keyword_present() {
        let with_kw = SearchState::new(true, "old", false);
        assert!(with_kw.search_all, "existing keyword keeps server scope");
        let blank = SearchState::new(true, "", false);
        assert!(!blank.search_all, "blank keyword defaults to page filter");
    }

    #[test]
    fn blank_keyword_only_valid_in_server_mode() {
        let mut st = SearchState::new(true, "", false);
        st.keywords = "   ".to_owned();
        st.search_all = true;
        assert!(st.validate().is_ok(), "blank clears the server keyword");
        st.search_all = false;
        assert_eq!(st.validate().unwrap_err(), "Please enter search keywords");
    }

    #[test]
    fn accept_trims_and_carries_scope() {
        let mut st = SearchState::new(true, "", false);
        st.keywords = " abc ".to_owned();
        st.search_all = true;
        assert_eq!(st.accept(), ("abc".to_owned(), true));
        st.search_all = false;
        assert_eq!(st.accept(), ("abc".to_owned(), false));
        let no_server = SearchState::new(false, "", false);
        assert!(!no_server.use_server_search());
    }
}
