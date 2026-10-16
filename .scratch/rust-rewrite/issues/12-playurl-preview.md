# 12: Playurl + Preview (qualities, codecs, sizes)

**What to build:** playurl fetching and the Preview: given a logged-in
session and an Episode, fetch actual stream qualities (16→127 incl. 8K/
Dolby/HDR/4K), audio qualities (30216→30251), codecs (AVC/HEVC/AV1) with
bitrates and sizes, and resolve the download selection by Quality
Priority + Codec Priority with fallback warning.

**Blocked by:** 08, 09, 11.

**Status:** ready-for-agent

- [ ] playurl DASH parsing (video+audio streams, ids, codecs, bandwidth, size)
- [ ] Selection honors quality priority lists; picks best available (test_media_options, test_codec_fallback vectors ported)
- [ ] Codec fallback detected and reported as a warning
- [ ] Legacy FLV multi-part delivery parsed when DASH absent
- [ ] Preview data shape carries everything the dialog shows

## Comments
