//! App services (ticket 19): i18n catalogs, theme tokens, log viewer backend,
//! clipboard monitor, notifications, window-state memory, teaching tips,
//! terms gate, parse history service.
//!
//! Non-parser app services ported from the Python build's `src/gui` +
//! `src/util/misc`. Persistence goes through `config::ConfigStore` and
//! `database::HistoryDatabase`; no service writes files of its own except
//! the log file (same `logs/app.log` the Python build writes).

pub mod clipboard;
pub mod history;
pub mod i18n;
#[rustfmt::skip]
mod i18n_data;
pub mod log;
pub mod notify;
pub mod terms;
pub mod theme;
pub mod tips;
pub mod window_state;
