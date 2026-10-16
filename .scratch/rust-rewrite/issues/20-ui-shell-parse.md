# 20: gpui-kit app shell + parse interface

**What to build:** The main window: navigation shell, parse interface
(URL input, auto-detect, parse button, progress), parsed Episode list
with checkable tree, pager + in-page search with match jumping, batch
select by ranges, clipboard-monitor hookup. Native gpui-kit visuals, same
layout/IA as the Python screens.

**Blocked by:** 01, 06, 09, 10, 11, 19.

**Status:** resolved

- [x] Shell renders the app's navigation structure and screens
- [x] Paste URL → parse → Episode tree with multi-P/collection/season expansion, checkable
- [x] Pager + in-page search jumping; batch select by number ranges
- [x] List-type parses show Auto Parse progress and server-side search box
- [x] Headless smoke: window opens, screens render (manual checklist stub for visuals)

## Answer

Shell + parse screen ported to gpui-kit in `e7e12eb3`: `Shell` sidebar
nav (Parse/Downloads/Favorites/About/Settings), `ParseScreen` (URL bar,
generation-guarded parse spawn, episode tree, pager, in-page search with
match jumping), `ParseViewModel` (batch-select ranges, auto-select
matrix, downloaded marking) + `dispatch.rs` parser routing; 13 unit
tests, workspace gate green. Headless-smoke checklist stub (spec line
17): open app → all 5 screens render; paste a video url → tree appears
and is checkable; pager/search/batch-select behave per model tests.

## Comments
