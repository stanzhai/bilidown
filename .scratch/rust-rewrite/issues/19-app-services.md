# 19: App services layer (i18n, themes, logs, clipboard, notifications, window state, teaching tips, terms gate)

**What to build:** The non-parser app services in Rust: zh_CN/zh_TW/en
i18n catalog; light/dark/auto theme + accent + display-scaling tokens;
log viewer backend with filter; debounced clipboard monitoring + startup
re-check; OS notifications; window-state memory; Teaching Tips data;
Terms Gate + privacy state; Parse History (capped 100).

**Blocked by:** 02.

**Status:** ready-for-agent

- [ ] i18n: all three catalogs cover the Python message keys (test_i18n_context semantics)
- [ ] Theme tokens map the two qss skins onto gpui-kit themes
- [ ] Clipboard monitor: debounce behavior + startup re-check tested headless (injectable source)
- [ ] Notifications, window-state persist/restore, terms-acceptance state, history cap 100 all round-trip

## Comments
