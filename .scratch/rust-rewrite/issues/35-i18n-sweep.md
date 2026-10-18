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

**Status:** ready-for-agent

- [ ] No user-visible hardcoded English/Chinese literals outside the
      catalog in `crates/app/src/ui/**` (names, ids, log strings exempt)
- [ ] Missing catalog entries added; both zh_CN and zh_TW provided
- [ ] Test: a scan (or curated list) of string sites resolves through
      the catalog; flipping `ShellI18n.lang` re-renders translated

## Comments
