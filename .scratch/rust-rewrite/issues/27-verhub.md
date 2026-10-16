# 27: Verhub update service

**What to build:** Verhub HTTP API client in Rust against the real API
surface (channel key `scottsloan-bili23-downloader`): version check,
skip-version, prerelease opt-in, forced-update handling, download-
and-apply update prompt integration.

**Blocked by:** 04, 24.

**Status:** in-progress

- [ ] Client hits the real Verhub endpoints (wiremock-pinned from captured API shapes)
- [ ] Skip-version suppresses the prompt for that version only; prerelease opt-in gates tags
- [ ] Forced update flow enforced per response
- [ ] Settings + About integrate the check

## Comments
