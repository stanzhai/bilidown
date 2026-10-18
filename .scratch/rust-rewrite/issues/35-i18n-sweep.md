# 35: i18n sweep — all UI strings route through the catalog

**What to build:** After switching language to Chinese, English
leftovers remain. Audit all UI string sites in `crates/app/src/ui/**`
and route every user-visible literal through the i18n catalog `tr`
lookup. Known hot spots from ticket 32: download_options dialog (tabs,
media/additional/download sections, rule builder, footer), parse screen
(toolbar, status lines, category names via `convention_type`), parse
model labels (count/match/pagination), login placeholders, favorites
stub, shell status notes.

Missing catalog entries get hand-added to the catalog source with a
HAND-ADDED marker section (the generation script is gone; note the
mechanism in the file header). `Lang` plumbing already exists.

**Blocked by:** 33 (login tooltip strings).

**Status:** resolved

- [x] No user-visible hardcoded English/Chinese literals outside the
      catalog in `crates/app/src/ui/**` (names, ids, log strings exempt)
- [x] Missing catalog entries added; both zh_CN and zh_TW provided
- [x] Test: a scan (or curated list) of string sites resolves through
      the catalog; flipping `ShellI18n.lang` re-renders translated

## Answer

Routing (catalog contexts resolved from `services/src/i18n_data.rs`):

- **Shell** (`ui/mod.rs`): `run_tip_pass` status notes via
  `dialogs::active_catalog` (bug fixed: was reading `Interface`→`General`,
  now `Interface.language`); render-path tooltip + sidebar nav via
  `cx.read_global::<ShellI18n>()` → `c.tr("MainWindow", …)`; sidebar first
  entry source key is `"Parser"` (MainWindow nav entry), Favorites arm now
  passes `cx`.
- **Parse screen** (`parse/screen.rs`): toolbar buttons, search bar,
  placeholders, pager totals, all status lines (`Parsing…`,
  `Parse failed: {error}`, `Batch parse failed: {error}`, `Preparing…`,
  `Auto-parse failed: {error}`, `Queued {count} task(s)`,
  ` ({count} duplicate(s) skipped)`, `Task creation failed: {error}`,
  `Invalid page number: {body}`, `Downloads unavailable…`) → ParseInterface
  / Pager / SearchWidget. `category_name` now goes through
  `i18n::episode_type(c, key)` (both `apply_parse_result` and the
  interactive-dialog accept).
- **Parse model** (`parse/model.rs`): `PaginationState::label(c)`,
  `count_label(c)`, `match_label(c)` take the catalog (tests updated).
- **Download options** (`download_options/mod.rs`): title, 4 tabs, media
  switches + field labels (MediaOptionsCard / MediaInfoCard), additional
  switches (Danmaku/Subtitle/Cover/Chapter/Metadata SettingCard), download
  page fields (DownloadPathSettingCard / DownloadFormatCard /
  DownloadOptionsDialog hand-adds / DownloadSettingsPage), rule builder
  (RuleBuilderWidget / RuleLevelRow / FragmentChip), preview bar
  (DownloadPreviewBar), footer Cancel/Download. `cx_of_catalog()` now
  resolves `Interface.language` from the ConfigStore (ponytail comment
  removed). `model.rs` "Unknown" → MediaInfoCard.
- **Login** (`login/screen.rs`): tel/code placeholders → LoginDialog;
  cookie placeholder → CookieLoginDialog.
- **Dialogs**: auto_parse Cancel; edit_host Edit/Remove/OK; view_cover
  Close + Loading...; batch_select OK + placeholder; jump_to_page OK;
  download_as_single_video OK; styles.rs Alignment fallback →
  AlignmentGroup. `screens.rs` Favorites stub → MainWindow.

39 hand-added entries (HAND-ADDED block in `i18n_data.rs`, both zh columns,
catalog pinned at 1012): ParseInterface ×12 (placeholders, toolbar,
statuses), DownloadOptionsDialog ×7 (Container, Metadata format, Filename
preview, Rule Builder, Naming rules, Cancel, Download), RuleBuilderWidget ×7,
ViewCoverDialog ×2, AutoParseDialog/JumpToPageDialog/BatchSelectDialog/
DownloadAsSingleVideoDialog OK, CDNServerDialog Edit/Remove/OK,
CookieLoginDialog placeholder, MainWindow ×3 (welcome tips + "Logged in").

Exempt (recorded): pager/tree glyphs (‹ › … ↑ ↓ × ← →), numeric page
counter `format!("{}/{}")`, codec names (AAC LC, FLAC), element ids, log
strings, `format!("+{}", cid)`, `format!("{{{v}}}")` chips,
`type {} → {}` prefix, app/version strings.

Tests: `ui::tests::i18n_sweep_sources_resolve_in_catalog` (curated ~100
`(ctx, source)` pairs assert `lookup().is_some()` against zh-CN, which
carries every entry; the En catalog is the identity map) and
`ui::tests::shell_language_flip_resolves_zh_catalog` (zh vs en catalogs
differ for routed sources). `cargo test -p services -p app` green (204 bin
tests, services suites).
