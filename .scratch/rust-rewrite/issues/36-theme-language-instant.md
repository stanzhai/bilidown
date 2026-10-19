# 36: Theme + language apply instantly; startup reads configured theme

**What to build:** Two settings bugs: (1) the dark-mode setting is
ignored — kit default is Light; startup must apply the configured
`QFluentWidgets.ThemeMode` right after `gpui_kit::init` (Light/Dark via
`Theme::change`, Auto via `Theme::sync_system_appearance`). (2) Changing
theme or language in settings must apply instantly, not after restart.
Theme setter calls `Theme::change` / `sync_system_appearance` directly;
language setter flips `SettingsUi.lang` and the `ShellI18n` app global
so shell/login re-render translated. `restart_notice` remains only for
`display_scaling`.

**Blocked by:** 33 (icon rail lands first; settings page touches the
same Shell state), 35 (i18n catalog must be complete before final
instant-apply wiring proof).

**Status:** resolved

- [x] Startup applies configured theme; dark mode works
- [x] Theme change in settings applies without restart
- [x] Language change in settings re-renders shell + login translated
      without restart; only `display_scaling` still shows restart notice
- [x] Tests for theme-mode mapping and language-setter behavior

## Answer

Implemented in `crates/app`:

- **Startup theme** (`main.rs`): new `apply_configured_theme(&cfg, cx)`
  runs between `gpui_kit::init` and window open — `ThemeModeSetting::
  from_config` on the stored `QFluentWidgets.ThemeMode` string; Light/Dark
  → `Theme::change`, Auto → `Theme::sync_system_appearance(None, cx)` (the
  service layer cannot see the OS appearance, so Auto is re-resolved here).
- **Theme setter** (`ui/settings/screen.rs`): new `apply_theme_now(&v, cx)`
  applies the same mapping through the entity-update boundary; the restart
  notice is gone. `Theme::change` syncs colors + tokens + Base and refreshes
  every window, so no manual redraw is needed.
- **Language setter**: new `apply_language(ui, cx, value)` — write-through
  save, `ui.lang = Lang::from_config(value)` (settings re-render), and
  `cx.set_global(ShellI18n { lang })` (shell + login read it every frame).
- `restart_notice` stays only for `display_scaling`; its doc comment was
  updated to match.

Tests: `main.rs::configured_theme_string_maps_to_kit_mode` (Light/Dark/
Auto/unknown → kit mode on a live `TestApp`) and
`ui/settings/screen.rs::language_setting_applies_instantly` (resolved `Lang`,
persisted string, no restart notice, `ShellI18n` global flip), plus
`services/tests/theme.rs::theme_mode_mapping_for_startup_apply`.
`cargo test -p services -p app` green (206 app tests, all services suites).

Wave 2: Auto mode now *tracks* the OS appearance live — `Shell::new`
installs `window.observe_window_appearance` which re-reads
`QFluentWidgets.ThemeMode` on every appearance flip and re-runs
`Theme::sync_system_appearance` only while the setting is Auto (manual
Light/Dark stay pinned). Dark-mode hardcoded colors were tokenized:
downloads cover placeholder `0x444444` → `theme().muted`, settings
rules-window scrim/panel → `theme().background.alpha(0.3)` /
`theme().popover`, download_options muted grays `0x8a8a8a` →
`theme().muted_foreground` (6 sites).

## Comments
