# 25: Misc dialogs (batch parse, about, misc setting dialogs)

**What to build:** The remaining dialogs: batch parse dialog (multi-line
av/BV ids → one Parse), interactive-video node selection dialog, and the
misc setting dialogs (danmaku style, subtitle style, rule builder entry
points, update prompt with skip-version).

**Blocked by:** 20.

**Status:** resolved

- [x] Batch parse dialog accepts multi-line ids and enqueues
- [x] Interactive node selection dialog walks the stein graph choices
- [x] Danmaku/subtitle style dialogs edit and persist their config sections
- [x] Update prompt honors skip-version/prerelease settings

## Comments

- Batch parse (`ui/dialogs/batch_parse.rs`): multi-line input, each line
  a URL/av/BV id; accept runs `start_batch_parse` (generation-guarded
  `parse_batch` walk, `LINK_INTERVAL` pacing) and applies the result via
  the normal `apply_parse_result` path.
- Batch select (`ui/dialogs/batch_select.rs` + `ParseListView::
  check_numbers`): "1,3,5-10" grammar with Python `validate()` rules
  (zero/negative/oversized numbers rejected, ranges must ascend);
  accepted numbers check the matching tree rows in place.
- Interactive video (`ui/dialogs/interactive.rs`): Rust walks the stein
  graph eagerly inside `parse_video_url`, so the tree is parked in
  `ParseScreen.pending_interactive_node` and the dialog only gates
  presentation (`auto_parse` re-applies the parked tree). Faithful to the
  Python dialog's only real choice (auto-parse all nodes).
- Style dialogs (`ui/dialogs/styles.rs`): plain Input rows over the
  Python setting groups; save parses each field back to its JSON type,
  reloads the group maps, inserts the edited keys, writes
  `Additional.danmaku_style` / `Additional.subtitle_style` (lowercase +
  legacy capital-A `Alignment` for the ASS reader), `ConfigStore::save`.
  Call sites: T24's settings cards (stand-in buttons on the settings
  stub until they land).
- Update (`ui/dialogs/update.rs`): gate + dialog + skip-version on top
  of `services::update` (T27): `decide` delegates to
  `suppresses_prompt`, `check_for_update` wraps `VerhubClient`;
  `Update/include_prerelease` → `include_preview`, `Interface/language`
  → `to_verhub_locale`.

## Answer

Ticket 25 done: batch parse/select + interactive + danmaku/subtitle
style + update dialogs in `app/src/ui/dialogs/` (feature commit
74464856; services::update swap at tip).
