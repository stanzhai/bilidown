//! Parse screen: URL input, parse dispatch, episode tree, pager and
//! in-page search (port of `src/gui/interface/parse.py`).

pub mod dispatch;
pub mod model;
pub mod screen;

pub use screen::ParseScreen;
