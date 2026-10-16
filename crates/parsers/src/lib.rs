//! Bilibili parse layer: the shared episode tree model plus the parsers
//! each ticket landed against it — list-family parsers (space, favorites,
//! collection/series, watch-later, history, weekly popular), pagination
//! auto-parse walking, the parse-list view model (ticket 10), and the
//! audio (au/am), batch-parse and area-detection parsers (ticket 11).

pub mod area;
pub mod audio;
pub mod auto_parse;
pub mod batch;
pub mod episode;
pub mod list;
pub mod media_parser;
pub mod tree;
pub mod view;
pub mod worker;

pub use area::{detect_area, AreaError, DetectOptions, AREA_DETECT_TIMEOUT};
pub use audio::{
    build_audio_tree, parse_audio, AudioError, AudioInfo, MENU_INFO_API, MENU_PAGE_SIZE,
    SONG_INFO_API, SONG_OF_MENU_API,
};
pub use auto_parse::{
    page_progress_text, walk_pages, AutoParseOptions, PageOutcome, WalkOutcome,
    DEFAULT_PAGE_INTERVAL,
};
pub use batch::{
    aid_to_bvid, bvid_of, build_video_items, fetch_video_data, parse_batch,
    validate_batch_input, video_episode_data, BatchError, BatchPage, VIDEO_VIEW_API,
};
pub use episode::list_base::{
    episode_type, unformat_episode_duration, with_search_keyword, wrap_node,
};
pub use list::{
    check_response, parse_favlist, parse_history, parse_list, parse_popular, parse_space,
    parse_watch_later, space_params, swap_host, ExtraData, ListError, ListParseContext,
    Pagination, ParseOutput,
};
pub use tree::{Attribute, CheckState, EpisodeData, Number, TreeItem};
pub use view::{ParseListView, ViewError};
pub use worker::{needs_wbi_signature, NO_WBI_PARSER_TYPES, WBI_PARSER_TYPES};
