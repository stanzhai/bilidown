//! Bilibili parse layer (ticket 11): the shared episode tree model plus
//! the audio (au/am), batch-parse and area-detection parsers.
//!
//! Sibling tickets land their parsers in this same crate against the same
//! [`tree`] model; nothing here is audio/batch-specific beyond the
//! modules themselves.

pub mod area;
pub mod audio;
pub mod batch;
pub mod episode;
pub mod parser_list;
pub mod tree;

pub use area::{detect_area, AreaError, DetectOptions, AREA_DETECT_TIMEOUT};
pub use audio::{
    build_audio_tree, parse_audio, AudioError, AudioInfo, MENU_INFO_API, MENU_PAGE_SIZE,
    SONG_INFO_API, SONG_OF_MENU_API,
};
pub use batch::{
    aid_to_bvid, bvid_of, build_video_items, fetch_video_data, parse_batch,
    validate_batch_input, video_episode_data, BatchError, BatchPage, VIDEO_VIEW_API,
};
pub use tree::{Attribute, CheckState, EpisodeData, Number, TreeItem};
