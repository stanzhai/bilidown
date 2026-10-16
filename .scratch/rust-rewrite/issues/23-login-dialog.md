# 23: Login dialog (QR, SMS+GeeTest webview, cookie paste)

**What to build:** The login dialog on the auth stack: QR code display +
poll status; SMS flow with GeeTest captcha rendered in an embedded
gpui-webview (WebKit/WebView2/webkit2gtk), falling back to the loopback
system-browser flow when the webview fails; manual cookie paste.

**Blocked by:** 08, 20.

**Status:** resolved

- [x] QR renders and logs in on confirm (wiremock-backed)
- [x] GeeTest captcha loads in webview; fallback loopback path exercised headless
- [x] Cookie paste accepted and persisted
- [x] Login state reflected in the shell

## Answer

Ported `src/gui/dialog/login.py` into `crates/app/src/ui/login/` (model/cid/qrcode_png/captcha page+server/controller/screen) on `auth` + `services::i18n`; QR poll, GeeTest loopback captcha server, and cookie paste verified with 44 in-crate tests incl. wiremock flows — commit `116c2ad1` (merged integration as `97719fa4`).

## Comments
