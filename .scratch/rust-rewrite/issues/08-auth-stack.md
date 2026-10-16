# 08: Wbi signing + anonymous session + login

**What to build:** The Bilibili auth stack: wbi img_key/sub_key fetch and
`enc_wbi` request signing; anonymous session auto-provisioning (buvid3/
buvid4/buvid_fp via murmur3, uuid, b_lsid, b_nut, bili_ticket via
HMAC-SHA256 GenWebTicket, ExClimbWuzhi activation); QR login flow; SMS
login flow (GeeTest captcha data structures only — UI in a later ticket);
manual cookie paste; persisted login state + wbi keys.

**Blocked by:** 04.

**Status:** in-progress

- [ ] enc_wbi signature vectors from test_wbi_signature pass verbatim
- [ ] Anonymous provisioning produces the full cookie set (wiremock-pinned endpoints, murmur3 outputs match Python's algorithm)
- [ ] bili_ticket GenWebTicket HMAC construction matches Python byte-for-byte
- [ ] QR login: generate/poll/confirm via wiremock; cookies persisted
- [ ] SMS login: send/poll with GeeTest gt/challenge/validate fields flowing; manual cookie paste accepted
- [ ] Session + wbi keys persisted to user data dir; test_parse_wbi_gate expectations ported

## Comments
