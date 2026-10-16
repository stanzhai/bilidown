# 18: Subtitles + metadata extras

**What to build:** Subtitle export (srt/lrc/txt/ass/json) with language
selection and Subtitle Style config; NFO sidecars (Kodi/Jellyfin/Emby) or
JSON alternative; cover export at original quality (jpg/png/avif/webp)
with the thumbnail DB as source cache.

**Blocked by:** 08, 03.

**Status:** resolved
- [x] Subtitle list fetch + each format export tested against fixtures
- [x] Language selection filters; ASS styling applied per config
- [x] NFO variants and JSON sidecar match Python output shapes
- [x] Cover export formats from thumbnail cache bytes

## Answer

crates/extras: subtitles (srt/lrc/txt/ass/json + language filter + MKV
embed w/ ISO 639-2 + AI track title), ASS renderer (Subtitle Style from
config), NFO movie/tvshow/episodedetails + JSON sidecar, cover export via
CoverDatabase original-quality cache, player info fetch, options
resolve() fallbacks — see commit 33cb6151.

## Comments
