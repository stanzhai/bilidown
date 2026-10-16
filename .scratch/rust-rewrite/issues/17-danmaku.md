# 17: Danmaku pipeline

**What to build:** Danmaku fetch (protobuf segments via a prost-generated
client) and export to xml/ass/json, with the native ASS renderer ported
including all Danmaku Style options (font/border/display-area/opacity/
speed/density/resolution).

**Blocked by:** 08.

**Status:** resolved

- [x] Protobuf segment decode from fixture bytes
- [x] xml/ass/json outputs byte-comparable to Python's for the same fixture (test_danmaku_xml vectors ported)
- [x] ASS renderer honors every style option; style-option matrix tested
- [x] Density/resolution parameters affect output as in Python

## Answer

`crates/danmaku`: prost-generated `DmSegMobileReply` decode + `DanmakuEntry` (MessageToDict semantics), wiremock-tested `fetch_segments` (one seg.so request per ceil(360 s)), and xml/ass/json renderers ported verbatim (ASS style line, `format_ass_time_by_ms`, ScrollTrack/StaticTrack layout engine); text metrics use a documented em-grid approximation of QFontMetrics. Commit `1b139617`, merged as `f19fbeb8`; workspace build/test/clippy green (29 danmaku tests).

## Comments
