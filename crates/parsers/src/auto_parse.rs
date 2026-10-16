//! Auto-parse pagination walking (port of the DYNAMIC page walk in
//! `src/util/parse/worker.py`'s ProgressParseWorker +
//! `gui/dialog/misc/auto_parse.py`'s payload).
//!
//! The user picks a page range (default: current → last) and an interval;
//! the walk fetches one page at a time and appends its nodes to one shared
//! result, reporting progress after each page
//! (`Parsing page N, total M pages, progress: X%`) so the view's locate
//! and search state from page one survive the appends.

use std::time::Duration;

use crate::list::{ListError, ParseOutput};
use crate::tree::TreeItem;

/// Interval between page fetches, matching `config.auto_parse_interval`'s
/// default (2.0s). Configurable per walk; tests pass `Duration::ZERO`.
pub const DEFAULT_PAGE_INTERVAL: Duration = Duration::from_secs(2);

/// Range and pacing of a walk (`AutoParsePayload`: url, start_page,
/// end_page; the interval comes from config).
#[derive(Debug, Clone)]
pub struct AutoParseOptions {
    pub start_page: i64,
    pub end_page: i64,
    pub interval: Duration,
}

impl Default for AutoParseOptions {
    fn default() -> Self {
        Self {
            start_page: 1,
            end_page: 1,
            interval: DEFAULT_PAGE_INTERVAL,
        }
    }
}

/// What each page of the walk produced (page number + its parse output).
#[derive(Debug)]
pub struct PageOutcome {
    pub page: i64,
    pub output: ParseOutput,
}

/// Whole-walk outcome: pages walked, all episodes appended, stop reason.
#[derive(Debug)]
pub struct WalkOutcome {
    pub pages: Vec<PageOutcome>,
    /// Episodes across all pages, in walk order.
    pub episodes: Vec<TreeItem>,
}

/// Progress message after each page (`Translator.TIP_MESSAGES` — English
/// source; the UI layer renders the locale).
pub fn page_progress_text(page: i64, total_pages: i64) -> String {
    let pct = if total_pages > 0 {
        (page as f64 / total_pages as f64 * 100.0) as i64
    } else {
        0
    };
    format!("Parsing page {page}, total {total_pages} pages, progress: {pct}%")
}

/// Walk the page range of one list url, appending every page's episodes
/// to one shared tree. `parse` is the family parser for the url (the
/// dispatcher picks it; the walk itself is family-agnostic).
/// `on_progress(text, page, total_pages)` fires after each page.
/// `should_stop` is polled between pages (the dialog's stop button).
pub async fn walk_pages<P, F, S>(
    options: &AutoParseOptions,
    mut parse: P,
    mut on_progress: F,
    should_stop: S,
) -> Result<WalkOutcome, ListError>
where
    P: FnMut(i64) -> futures_boxed::BoxFuture<'static, Result<ParseOutput, ListError>>,
    F: FnMut(&str, i64, i64),
    S: Fn() -> bool,
{
    let mut outcome = WalkOutcome {
        pages: Vec::new(),
        episodes: Vec::new(),
    };

    for page in options.start_page..=options.end_page {
        if should_stop() {
            break;
        }

        let output = parse(page).await?;
        outcome.episodes.extend(output.node.children.clone());
        on_progress(&page_progress_text(page, options.end_page), page, options.end_page);
        outcome.pages.push(PageOutcome { page, output });

        if page < options.end_page && !options.interval.is_zero() {
            tokio::time::sleep(options.interval).await;
        }
    }

    Ok(outcome)
}

// ponytail: the generic `parse` closure needs a boxed future; the alias
// lives here instead of a futures dep just for one signature.
mod futures_boxed {
    use std::future::Future;
    use std::pin::Pin;

    pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;
}
