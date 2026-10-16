# 19: App services layer (i18n, themes, logs, clipboard, notifications, window state, teaching tips, terms gate)

**What to build:** The non-parser app services in Rust: zh_CN/zh_TW/en
i18n catalog; light/dark/auto theme + accent + display-scaling tokens;
log viewer backend with filter; debounced clipboard monitoring + startup
re-check; OS notifications; window-state memory; Teaching Tips data;
Terms Gate + privacy state; Parse History (capped 100).

**Blocked by:** 02.

**Status:** resolved

- [x] i18n: all three catalogs cover the Python message keys (test_i18n_context semantics)
- [x] Theme tokens map the two qss skins onto gpui-kit themes
- [x] Clipboard monitor: debounce behavior + startup re-check tested headless (injectable source)
- [x] Notifications, window-state persist/restore, terms-acceptance state, history cap 100 all round-trip

## Answer

Implemented in `crates/services` (commit dc36dc99): generated i18n catalog (973 entries / 125 contexts, zh_CN + zh_TW verbatim from the `.ts` files, en passthrough), theme tokens + accent parsing, log viewer backend (verbatim Python LOG_PATTERN port + filters), headless clipboard monitor (1.5 s same-URL debounce, injectable source/clock), notifications, window-state persist/restore with title-bar visibility check, teaching tips, terms gate, parse-history wrapper over `database::HistoryDatabase` (cap enforced there). 42 integration tests.

## Comments
