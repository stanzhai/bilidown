# 17: Danmaku pipeline

**What to build:** Danmaku fetch (protobuf segments via a prost-generated
client) and export to xml/ass/json, with the native ASS renderer ported
including all Danmaku Style options (font/border/display-area/opacity/
speed/density/resolution).

**Blocked by:** 08.

**Status:** ready-for-agent

- [ ] Protobuf segment decode from fixture bytes
- [ ] xml/ass/json outputs byte-comparable to Python's for the same fixture (test_danmaku_xml vectors ported)
- [ ] ASS renderer honors every style option; style-option matrix tested
- [ ] Density/resolution parameters affect output as in Python

## Comments
