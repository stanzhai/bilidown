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

Final pass (wave 2): the settings screen's `t("Key")` closures were
redirected to the professional-card contexts instead of hand-adding —
~79 call sites in `settings/screen.rs` now use
`lang.tr("Ctx", "Key")` against PersonalizationCard,
WindowBehaviorSettingCard, DownloadHandlingSettingCard, MediaOptionsCard,
DownloadFormatCard, chapter/danmaku/subtitle/cover/metadata/numbering
cards, NamingConventionCard, MCPSettingCard, CheckUpdateSettingCard,
FFmpegSettingCard, CDNSettingCard, ProxyDialog and friends; misspelled
keys were fixed to the real catalog sources (`Regenerate`,
`Client Configuration`, `Check for Updates`, `Restore to Built-in Default`,
`Reset to Default`, `Naming Rules`, `Rule Name`, …). Format names
(MP4/MKV/XML/ASS/SRT/PNG/JPG/WebP/AVIF/JSON/NFO) are catalogued but
untranslated (both zh columns keep the source). Sweep coverage grew to
~225 `(ctx, source)` assertions; the catalog now pins **1052 entries
across 125 contexts** (973 generated + 79 hand-added in the marked
block, 介面→界面 applied). `cargo test -p app -p services` green
(208 app tests + all services suites), `/tmp/verify_sweep.py` reports
`MISSING: 0`.
