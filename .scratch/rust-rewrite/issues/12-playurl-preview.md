# 12: Playurl + Preview (qualities, codecs, sizes)

**What to build:** playurl fetching and the Preview: given a logged-in
session and an Episode, fetch actual stream qualities (16→127 incl. 8K/
Dolby/HDR/4K), audio qualities (30216→30251), codecs (AVC/HEVC/AV1) with
bitrates and sizes, and resolve the download selection by Quality
Priority + Codec Priority with fallback warning.

**Blocked by:** 08, 09, 11.

**Status:** resolved

- [x] playurl DASH parsing (video+audio streams, ids, codecs, bandwidth, size)
- [x] Selection honors quality priority lists; picks best available (test_media_options, test_codec_fallback vectors ported)
- [x] Codec fallback detected and reported as a warning
- [x] Legacy FLV multi-part delivery parsed when DASH absent
- [x] Preview data shape carries everything the dialog shows

## Answer

`crates/parsers/src/playurl.rs` + `preview.rs` + `tests/playurl_preview.rs`
(25 wiremock tests, commit 9811578b → merge 54680328). test/test_media_options.py
has no portable selection vectors (Python drives selection through the GUI
dialog) — new wiremock tests cover selection; test_codec_fallback.py
TestDownloadSideWarning ported verbatim; TestDialogWarning belongs to ticket 21.

## Comments
