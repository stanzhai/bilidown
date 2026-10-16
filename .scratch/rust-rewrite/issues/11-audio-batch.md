# 11: Audio parsers (au/am incl. menus) + batch parse

**What to build:** Audio parsing: au singles and am menus (menus expand to
their tracks); batch parse accepting a multi-line list of av/BV ids and
folding it into one Parse result. Also area auto-detection feeding CDN
area choices.

**Blocked by:** 08, 06.

**Status:** resolved

- [x] au/am parse to audio Episode lists; menu expansion correct
- [x] Batch parse: multi-line id list → combined Episode list
- [x] Area auto-detection ports test_area_detection vectors
- [x] All via wiremock fixtures

## Answer

`crates/parsers`: `tree.rs` (TreeItem/Attribute/EpisodeData port), `audio.rs` (au/am + menu expansion), `batch.rs` (dialog validation + view-API walk folding multi-line av/BV into one root, aid→bvid algorithm), `area.rs` (zone-fetch client over `network::cdn::parse_area`, which landed via the ticket-14 merge — the pure vectors live in T14's `network::cdn` tests; ticket 11's contribution is the detect client + wiremock zone tests with the real mainland/HK bodies). 23 wiremock tests. Commit: e697b521; final docs closeout 83cc6c9f.

## Comments
