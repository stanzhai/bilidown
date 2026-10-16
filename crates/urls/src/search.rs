//! Search-keyword handling for URL types that support server-side search
//! (space/favlist/history/watch_later). Others (list/popular/…) have no
//! search parameter, so `build_search_url` returns them unchanged.
//!
//! The keyword always lives in the link: paging, auto-parse of paginated
//! results, and parse history all re-parse the same link, so no separate
//! search state has to be kept.
use url::Url;

use crate::classify::{parser_type, ParserType};

/// Which query parameter carries the keyword for this link's type.
/// Bilibili endpoints are inconsistent here; only the types listed below
/// support server-side search. Collections (`list`), weekly popular
/// (`popular`) and friends have no search parameter, so they are absent.
///
/// `None` = the type doesn't support server-side search.
pub fn keyword_param(url: &str) -> Option<&'static str> {
    match parser_type(url)? {
        ParserType::Space | ParserType::Favlist | ParserType::History => Some("keyword"),
        ParserType::WatchLater => Some("key"),
        _ => None,
    }
}

/// Does this link's type support server-side keyword search?
pub fn support_search(url: &str) -> bool {
    keyword_param(url).is_some()
}

/// Extract the percent-decoded, whitespace-stripped keyword from a link.
/// `None` when the type doesn't support search or the parameter is absent.
pub fn extract_keyword(url: &str) -> Option<String> {
    let param = keyword_param(url)?;
    let parsed = Url::parse(url).ok()?;
    parsed
        .query_pairs()
        .find(|(k, _)| k == param)
        .map(|(_, v)| v.trim().to_owned())
}

/// Write the keyword back into the link. An empty/whitespace keyword removes
/// the parameter (restoring the full list); types without search support get
/// the link back unchanged. Other params keep their values and order.
pub fn build_search_url(url: &str, keyword: &str) -> String {
    let Some(param) = keyword_param(url) else {
        return url.to_owned();
    };
    let Ok(mut parsed) = Url::parse(url) else {
        return url.to_owned();
    };
    let keyword = keyword.trim();

    // Rebuild the query: keep other params (and their order), drop or
    // replace the keyword param.
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    let mut kept = 0;
    for (k, v) in parsed.query_pairs() {
        if k != param {
            query.append_pair(&k, &v);
            kept += 1;
        }
    }
    if keyword.is_empty() {
        if kept == 0 {
            parsed.set_query(None);
        } else {
            parsed.set_query(Some(&query.finish()));
        }
    } else {
        query.append_pair(param, keyword);
        parsed.set_query(Some(&query.finish()));
    }
    parsed.to_string()
}
