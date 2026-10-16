//! Bilibili parse layer (ticket 10): list-family parsers (space,
//! favorites, collection/series, watch-later, history, weekly popular),
//! pagination auto-parse walking, and the parse-list view model — against
//! the shared episode tree model owned by ticket 11.

pub mod auto_parse;
pub mod list;
pub mod tree;
pub mod view;
pub mod worker;

pub use auto_parse::{
    page_progress_text, walk_pages, AutoParseOptions, PageOutcome, WalkOutcome,
    DEFAULT_PAGE_INTERVAL,
};
pub use list::{
    check_response, parse_favlist, parse_history, parse_list, parse_popular, parse_space,
    parse_watch_later, space_params, swap_host, ExtraData, ListError, ListParseContext,
    Pagination, ParseOutput,
};
pub use tree::{Attribute, CheckState, EpisodeData, Number, TreeItem};
pub use view::{ParseListView, ViewError};
pub use worker::{needs_wbi_signature, NO_WBI_PARSER_TYPES, WBI_PARSER_TYPES};
