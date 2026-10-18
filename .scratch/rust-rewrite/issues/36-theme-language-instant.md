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

**Status:** ready-for-agent

- [ ] Startup applies configured theme; dark mode works
- [ ] Theme change in settings applies without restart
- [ ] Language change in settings re-renders shell + login translated
      without restart; only `display_scaling` still shows restart notice
- [ ] Tests for theme-mode mapping and language-setter behavior

## Comments
