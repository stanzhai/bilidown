# 10: List parsers (space, favorites, collected, watch-later, history, weekly, dynamic) + Auto Parse + server-side search

**What to build:** The list-family parsers — space pages, favorites
folders, collected lists, watch-later, history, weekly popular, dynamic —
with pagination walking (Auto Parse progress), and server-side keyword
search inside those lists.

**Blocked by:** 08, 06.

**Status:** resolved

- [x] Each list family parses and paginates via wiremock fixtures
- [x] Space parser ports test_space_parser vectors
- [x] Auto Parse walks all pages with progress callbacks (test_parse_worker, test_reparse_worker semantics)
- [x] Server-side keyword search issues the API's search params and filters results
- [x] List view model supports in-page search + match jumping (test_parse_list_view semantics)

## Answer

Done in 017beea1 (merged integration in 5f66695a): `crates/parsers` gains `list.rs` (six family parsers, wrapper→category→items output, pagination + server-side keyword search), `auto_parse.rs` (`walk_pages` with progress/stop), `view.rs` (`ParseListView`), `worker.rs` (WBI endpoint sets); tree model shared with ticket 11. 33 tests (22 list vectors, 3 walk, 8 lib) + workspace build/test/clippy green.

## Comments
