# 27: Verhub update service

**What to build:** Verhub HTTP API client in Rust against the real API
surface (channel key `scottsloan-bili23-downloader`): version check,
skip-version, prerelease opt-in, forced-update handling, download-
and-apply update prompt integration.

**Blocked by:** 04, 24.

**Status:** resolved

- [x] Client hits the real Verhub endpoints (wiremock-pinned from captured API shapes)
- [x] Skip-version suppresses the prompt for that version only; prerelease opt-in gates tags
- [x] Forced update flow enforced per response
- [ ] Settings + About integrate the check (ticket 25 owns the dialog + integration; backend exposed)

## Answer

`crates/services/src/update.rs` ports `src/util/misc/update.py` plus the
verhub_sdk 0.2.10 wire surface it calls (check-update / versions list /
latest / latest-preview, platform + client-time headers, SDK UA); skip
gate `suppresses_prompt`, prerelease `include_preview` param, forced
`required` flag exposed to ticket 25's dialog. Tests: wiremock wire pins
(`tests/update.rs`) + unit pins. Commit 753f7f98.
