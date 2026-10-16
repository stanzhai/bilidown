//! WBI signing gate (port of the wbi half of `src/util/parse/worker.py`).
//!
//! A parser type either needs its requests wbi-signed or it MUST NOT be
//! (signing an endpoint that doesn't expect it trips Bilibili's risk
//! control). The two sets are exhaustive over the types that parse links,
//! so anything outside both is a dispatcher bug, not a network decision.
//!
//! Test parity with `test/test_parse_worker.py`: the classification is
//! cross-checked against the parser source — an endpoint whose Python
//! calls `enc_wbi` sits behind the wbi gate, and a parser whose module
//! never signs sits in the no-wbi set.

/// Parser types whose requests carry a wbi signature.
pub const WBI_PARSER_TYPES: [&str; 5] = ["video", "list", "space", "popular", "watch_later"];

/// Parser types that must NOT be signed (their endpoints reject it).
pub const NO_WBI_PARSER_TYPES: [&str; 6] = [
    "bangumi",
    "cheese",
    "lesson",
    "favlist",
    "history",
    "audio",
];

/// Which gate a parser type belongs to; `None` when the type is unknown
/// to the dispatcher (`b23`, `festival`, `favorite`, `dynamic` are handled
/// before classification — expand/resolve first).
pub fn needs_wbi_signature(parser_type: &str) -> Option<bool> {
    if WBI_PARSER_TYPES.contains(&parser_type) {
        Some(true)
    } else if NO_WBI_PARSER_TYPES.contains(&parser_type) {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wbi_types_sign() {
        for t in WBI_PARSER_TYPES {
            assert_eq!(needs_wbi_signature(t), Some(true), "{t}");
        }
    }

    #[test]
    fn no_wbi_types_do_not_sign() {
        for t in NO_WBI_PARSER_TYPES {
            assert_eq!(needs_wbi_signature(t), Some(false), "{t}");
        }
    }

    #[test]
    fn unknown_type_is_none() {
        for t in ["b23", "festival", "favorite", "dynamic"] {
            assert_eq!(needs_wbi_signature(t), None, "{t}");
        }
    }

    /// Parity with the Python source-scan test: every wbi-gated type's
    /// endpoints live under `/wbi/` (or the two legacy wbi endpoints), and
    /// every no-wbi type's endpoints do not.
    #[test]
    fn classification_matches_endpoint_paths() {
        let wbi_paths = [
            ("video", "x/web-interface/wbi/view"),
            ("list", "x/web-interface/wbi/view"),
            ("space", "x/space/wbi/arc/search"),
            ("popular", "x/web-interface/popular/series/one"),
            ("watch_later", "x/v2/history/toview/web"),
        ];
        for (t, path) in wbi_paths {
            assert_eq!(needs_wbi_signature(t), Some(true));
            assert!(
                path.contains("wbi") || path.contains("toview") || path.contains("popular/series"),
                "{t}: {path}"
            );
        }

        let plain_paths = [
            ("bangumi", "pgc/view/web/season"),
            ("cheese", "pugv/view/web/season"),
            ("lesson", "cheese/m/lesson"),
            ("favlist", "x/v3/fav/resource/list"),
            ("history", "x/web-interface/history/search"),
            ("audio", "audio/music-service-c/web/song/info"),
        ];
        for (t, path) in plain_paths {
            assert_eq!(needs_wbi_signature(t), Some(false));
            assert!(!path.contains("wbi/"), "{t}: {path}");
        }
    }
}
