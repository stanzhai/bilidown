# 20: gpui-kit app shell + parse interface

**What to build:** The main window: navigation shell, parse interface
(URL input, auto-detect, parse button, progress), parsed Episode list
with checkable tree, pager + in-page search with match jumping, batch
select by ranges, clipboard-monitor hookup. Native gpui-kit visuals, same
layout/IA as the Python screens.

**Blocked by:** 01, 06, 09, 10, 11, 19.

**Status:** ready-for-agent

- [ ] Shell renders the app's navigation structure and screens
- [ ] Paste URL → parse → Episode tree with multi-P/collection/season expansion, checkable
- [ ] Pager + in-page search jumping; batch select by number ranges
- [ ] List-type parses show Auto Parse progress and server-side search box
- [ ] Headless smoke: window opens, screens render (manual checklist stub for visuals)

## Comments
