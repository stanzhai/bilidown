# 08: Wbi signing + anonymous session + login

**What to build:** The Bilibili auth stack: wbi img_key/sub_key fetch and
`enc_wbi` request signing; anonymous session auto-provisioning (buvid3/
buvid4/buvid_fp via murmur3, uuid, b_lsid, b_nut, bili_ticket via
HMAC-SHA256 GenWebTicket, ExClimbWuzhi activation); QR login flow; SMS
login flow (GeeTest captcha data structures only — UI in a later ticket);
manual cookie paste; persisted login state + wbi keys.

**Blocked by:** 04.

**Status:** resolved

- [x] enc_wbi signature vectors from test_wbi_signature pass verbatim
- [x] Anonymous provisioning produces the full cookie set (wiremock-pinned endpoints, murmur3 outputs match Python's algorithm)
- [x] bili_ticket GenWebTicket HMAC construction matches Python byte-for-byte
- [x] QR login: generate/poll/confirm via wiremock; cookies persisted
- [x] SMS login: send/poll with GeeTest gt/challenge/validate fields flowing; manual cookie paste accepted
- [x] Session + wbi keys persisted to user data dir; test_parse_wbi_gate expectations ported

## Answer

Implemented in `crates/auth` (wbi/murmur3/exclimbwuzhi/session/login modules, 13 tests: 2 verbatim enc_wbi vectors, 5 murmur3 vectors incl. the real config.json buvid_fp, 2 GenWebTicket hexsign vectors, ExClimbWuzhi structure, config round-trip, parse_cookies matrix) — commit 03264b5e.

## Comments
