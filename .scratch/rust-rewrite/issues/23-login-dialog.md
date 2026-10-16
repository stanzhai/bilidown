# 23: Login dialog (QR, SMS+GeeTest webview, cookie paste)

**What to build:** The login dialog on the auth stack: QR code display +
poll status; SMS flow with GeeTest captcha rendered in an embedded
gpui-webview (WebKit/WebView2/webkit2gtk), falling back to the loopback
system-browser flow when the webview fails; manual cookie paste.

**Blocked by:** 08, 20.

**Status:** ready-for-agent

- [ ] QR renders and logs in on confirm (wiremock-backed)
- [ ] GeeTest captcha loads in webview; fallback loopback path exercised headless
- [ ] Cookie paste accepted and persisted
- [ ] Login state reflected in the shell

## Comments
