//! Login dialog screen — port of `src/gui/dialog/login.py`
//! (LoginDialog + CookieLoginDialog) onto gpui.
//!
//! Layout (Python parity): left column = "Scan QR Code to Log In" section
//! with a 160×160 QR image + status line; right column = "SMS Login"
//! section with cid combo + phone, code + Get Code, Log In, and the
//! cookies hyperlink. Min width 700, closable by backdrop click, no
//! button bar.
//!
//! The QR poll loop runs as a spawned task with a generation token so
//! stale polls after close/reopen are dropped (ParseScreen precedent).
//! The shell owns the shared auth client/state (`Shell::auth_*`).

use std::sync::Arc;

use auth::login::QRCodeScanStatus;
use gpui_kit::gpui::{img, Image, ImageFormat, ImageSource};
use gpui_kit::base::{Dialog, DialogHandle};
use gpui_kit::base::Disableable;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::{AppContext, Context, Entity, IntoElement, Window, div, prelude::*, px};
use services::i18n::catalog;

use super::cid;
use super::controller::{self, LoginEndpoints};
use super::model::{CookieModel, LoginModel, QrPhase, SmsPhase};
use super::server::CaptchaServer;

const CTX: &str = "LoginDialog";
const CTX_COOKIE: &str = "CookieLoginDialog";
const CTX_ERR: &str = "ERROR_MESSAGES";

/// The shell-owned login state: dialog handles + models + the running
/// loopback server. All network I/O goes through the shell's shared
/// auth client so captured cookies stick.
pub struct LoginState {
    pub handle: DialogHandle,
    pub cookie_handle: DialogHandle,
    pub model: LoginModel,
    pub cookie: CookieModel,
    pub qr_png: Option<Arc<Image>>,
    pub tel: Entity<InputState>,
    pub code: Entity<InputState>,
    pub cookie_text: Entity<InputState>,
    pub server: Option<CaptchaServer>,
    pub captcha_key: String,
    /// Monotonic token: work from an older dialog session is dropped.
    pub generation: u64,
    /// Shell login flag (mirrors `Shell::login`, config-backed at start).
    pub is_login: bool,
    /// Pending toast text (error or success) rendered as a status line.
    pub toast: Option<String>,
}

impl LoginState {
    pub fn new(window: &mut Window, cx: &mut Context<crate::ui::Shell>, is_login: bool) -> Self {
        Self {
            handle: DialogHandle::new(false),
            cookie_handle: DialogHandle::new(false),
            model: LoginModel::new(),
            cookie: CookieModel::new(),
            qr_png: None,
            tel: cx.new(|cx| InputState::new(window, cx).placeholder("Enter phone number")),
            code: cx.new(|cx| InputState::new(window, cx).placeholder("Enter verification code")),
            cookie_text: cx.new(|cx| InputState::new(window, cx).placeholder(
                "SESSDATA=xxx;bili_jct=xxx;DedeUserID=xxx;DedeUserID__ckMd5=xxx",
            )),
            server: None,
            captcha_key: String::new(),
            generation: 0,
            is_login,
            toast: None,
        }
    }

    fn tr(cx: &Context<crate::ui::Shell>, context: &str, source: &str) -> String {
        let lang = cx.read_global::<crate::ui::ShellI18n, _>(|i18n, _| i18n.lang);
        catalog(lang.resolve()).tr(context, source).to_owned()
    }

    /// Open (or reopen) the login dialog: reset state, kick QR generate.
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<crate::ui::Shell>) {
        self.model = LoginModel::new();
        self.cookie = CookieModel::new();
        self.qr_png = None;
        self.captcha_key = String::new();
        self.toast = None;
        if let Some(server) = self.server.take() {
            server.stop();
        }
        self.generation += 1;
        self.handle.open(window, cx);
        self.start_qr(cx);
    }

    /// Python `showEvent` → `QRCodeLogin.generate` + `start_polling`.
    fn start_qr(&mut self, cx: &mut Context<crate::ui::Shell>) {
        let generation = self.generation;
        let endpoints = LoginEndpoints::default();
        let client = shell_client(cx);
        cx.spawn(async move |shell, cx| {
            let Ok((png, key)) = controller::qr_start(&client, &endpoints).await else {
                return;
            };
            let _ = shell.update(cx, |shell, cx| {
                let login = &mut shell.login;
                if login.generation != generation {
                    return;
                }
                login.qr_png =
                    Some(Arc::new(Image::from_bytes(ImageFormat::Png, png)));
                login.model.qr_generated(&key);
                login.poll_qr(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// The 1s poll loop; stops on Success/Expired or dialog close.
    fn poll_qr(&mut self, cx: &mut Context<crate::ui::Shell>) {
        let generation = self.generation;
        let key = self.model.qr_key.clone();
        let endpoints = LoginEndpoints::default();
        cx.spawn(async move |shell, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let Ok((mut state, user_agent, client)) = shell.read_with(cx, |shell, _| {
                    (
                        shell.auth_state.clone(),
                        shell.user_agent.clone(),
                        shell.auth_client.clone(),
                    )
                }) else {
                    break;
                };
                let status = controller::qr_poll_once(
                    client.as_ref(),
                    &endpoints,
                    &key,
                    &mut state,
                    &user_agent,
                )
                .await;
                let keep_going = shell.update(cx, |shell, cx| {
                    if shell.login.generation != generation || !shell.login.handle.is_open() {
                        return false;
                    }
                    match status {
                        Ok(s) => {
                            shell.login.model.qr_polled(s);
                            if s == QRCodeScanStatus::Success {
                                shell.set_auth_state(state, cx);
                                let text = Self::tr(cx, CTX, "Successfully logged in via QR code");
                                shell.login.model.login_success(text.clone());
                                shell.login.toast = Some(text);
                                shell.on_login_success(cx);
                                return false;
                            }
                            s != QRCodeScanStatus::Expired
                        }
                        Err(e) => {
                            // transport hiccup: keep polling; surface the
                            // first failure only to avoid spamming the
                            // status line
                            if shell.login.toast.is_none() {
                                shell.login.toast = Some(e);
                            }
                            true
                        }
                    }
                });
                match keep_going {
                    Ok(true) => continue,
                    _ => break,
                }
            }
        })
        .detach();
    }

    /// "Get Code" → validate tel, fetch captcha, start the loopback
    /// server, open the browser page (Python `on_send_verification` →
    /// `init_geetest`).
    pub fn send_code(&mut self, cx: &mut Context<crate::ui::Shell>) {
        let tel_empty = Self::tr(cx, CTX, "Phone number cannot be empty");
        self.model.tel = self.tel.read(cx).value().to_string();
        if !self.model.start_captcha(tel_empty) {
            self.take_toast(cx);
            return;
        }
        let client = shell_client(cx);
        cx.spawn(async move |shell, cx| {
            match controller::captcha_start(&client).await {
                Ok(server) => {
                    let _ = shell.update(cx, |shell, cx| {
                        if let Some(old) = shell.login.server.replace(server) {
                            old.stop();
                        }
                        cx.notify();
                    });
                }
                Err(e) => {
                    let _ = shell.update(cx, |shell, cx| {
                        shell.login.model.fail(e.clone());
                        shell.login.toast = Some(e);
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    /// GeeTest callback landed → drive `sms_send` with the solved pair.
    pub fn on_captcha_solved(&mut self, cx: &mut Context<crate::ui::Shell>) {
        let Some(solved) = self.server.as_ref().and_then(|s| s.take_solved()) else {
            return;
        };
        let endpoints = LoginEndpoints::default();
        let cid = self.model.cid.clone();
        let tel = self.model.tel.clone();
        let captcha_key = self.captcha_key.clone();
        cx.spawn(async move |shell, cx| {
            let Ok((client, has_server)) = shell.read_with(cx, |shell, _| {
                (
                    shell.auth_client.clone(),
                    shell.login.server.is_some(),
                )
            }) else {
                return;
            };
            if !has_server {
                return;
            }
            let result = controller::sms_send_with_captcha(
                client.as_ref(),
                &endpoints,
                &cid,
                &tel,
                solved,
                &captcha_key,
            )
            .await;
            let _ = shell.update(cx, |shell, cx| {
                match result {
                    Ok(key) => {
                        shell.login.captcha_key = key;
                        shell.login.model.code_sent();
                        shell.login.start_countdown(cx);
                    }
                    Err(e) => {
                        shell.login.model.fail(e.clone());
                        shell.login.toast = Some(e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 1s countdown timer driving `model.tick_countdown`.
    fn start_countdown(&mut self, cx: &mut Context<crate::ui::Shell>) {
        let generation = self.generation;
        cx.spawn(async move |shell, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let keep = shell.update(cx, |shell, cx| {
                    let login = &mut shell.login;
                    if login.generation != generation {
                        return false;
                    }
                    let more = login.model.tick_countdown();
                    cx.notify();
                    more
                });
                match keep {
                    Ok(true) => continue,
                    _ => break,
                }
            }
        })
        .detach();
    }

    /// "Log In" (SMS) — validate both, exchange the code for cookies.
    pub fn sms_login(&mut self, cx: &mut Context<crate::ui::Shell>) {
        let tel_empty = Self::tr(cx, CTX, "Phone number cannot be empty");
        let code_empty = Self::tr(cx, CTX, "Verification code cannot be empty");
        self.model.tel = self.tel.read(cx).value().to_string();
        self.model.code = self.code.read(cx).value().to_string();
        if !self.model.start_sms_login(tel_empty, code_empty) {
            self.take_toast(cx);
            return;
        }
        let endpoints = LoginEndpoints::default();
        let cid = self.model.cid.clone();
        let tel = self.model.tel.clone();
        let code = self.model.code.clone();
        let captcha_key = self.captcha_key.clone();
        cx.spawn(async move |shell, cx| {
            let Ok((mut state, user_agent, client)) = shell.read_with(cx, |shell, _| {
                (
                    shell.auth_state.clone(),
                    shell.user_agent.clone(),
                    shell.auth_client.clone(),
                )
            }) else {
                return;
            };
            let result = controller::sms_login_with(
                client.as_ref(),
                &endpoints,
                &controller::SmsLoginForm {
                    cid: &cid,
                    tel: &tel,
                    code: &code,
                    captcha_key: &captcha_key,
                },
                &mut state,
                &user_agent,
            )
            .await;
            let _ = shell.update(cx, |shell, cx| match result {
                Ok(()) => {
                    shell.set_auth_state(state, cx);
                    let text = Self::tr(cx, CTX, "Successfully logged in via SMS");
                    shell.login.model.login_success(text.clone());
                    shell.login.toast = Some(text);
                    shell.on_login_success(cx);
                }
                Err(e) => {
                    shell.login.model.fail(e.clone());
                    shell.login.toast = Some(e);
                }
            });
        })
        .detach();
    }

    /// Cookie sub-dialog verify (Python `CookieLoginDialog.verify`).
    pub fn cookie_verify(&mut self, cx: &mut Context<crate::ui::Shell>) {
        let empty = Self::tr(cx, CTX_COOKIE, "Cookies cannot be empty");
        self.cookie.text = self.cookie_text.read(cx).value().to_string();
        if !self.cookie.verify(empty) {
            self.take_toast(cx);
            return;
        }
        let endpoints = LoginEndpoints::default();
        let text = self.cookie.text.clone();
        cx.spawn(async move |shell, cx| {
            let Ok((mut state, user_agent, client, wbi)) = shell.read_with(cx, |shell, _| {
                (
                    shell.auth_state.clone(),
                    shell.user_agent.clone(),
                    shell.auth_client.clone(),
                    shell.auth_wbi.clone(),
                )
            }) else {
                return;
            };
            let result =
                controller::cookie_verify(client.as_ref(), &endpoints, &text, &wbi, &mut state, &user_agent)
                    .await;
            let _ = shell.update(cx, |shell, cx| {
                use auth::login::{CookieLoginError, CookieLoginOutcome};
                match result {
                    Ok(CookieLoginOutcome::Success) => {
                        shell.set_auth_state(state, cx);
                        let text = Self::tr(cx, CTX, "Successfully logged in via Cookies");
                        shell.login.cookie.accept();
                        shell.login.model.login_success(text.clone());
                        shell.login.toast = Some(text);
                        // the cookie dialog auto-closes via `CookieModel.accepted`
                        shell.on_login_success(cx);
                    }
                    Ok(CookieLoginOutcome::Invalid) => {
                        let msg = Self::tr(cx, CTX_ERR, "Cookies are invalid or expired");
                        shell.login.cookie.fail(msg.clone());
                        shell.login.toast = Some(msg);
                    }
                    Err(CookieLoginError::FormatInvalid) => {
                        let msg = Self::tr(cx, CTX_ERR, "Invalid Cookies format");
                        shell.login.cookie.fail(msg.clone());
                        shell.login.toast = Some(msg);
                    }
                    Err(CookieLoginError::MissingSessdata) => {
                        let msg =
                            Self::tr(cx, CTX_ERR, "SESSDATA field not found in the pasted Cookies");
                        shell.login.cookie.fail(msg.clone());
                        shell.login.toast = Some(msg);
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        shell.login.cookie.fail(msg.clone());
                        shell.login.toast = Some(msg);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn take_toast(&mut self, cx: &mut Context<crate::ui::Shell>) {
        if let Some(e) = self.model.error.take() {
            self.toast = Some(e);
        }
        if let Some(e) = self.cookie.error.take() {
            self.toast = Some(e);
        }
        cx.notify();
    }

    /// Render both dialogs into the shell's element tree.
    pub fn render(&mut self, cx: &mut Context<crate::ui::Shell>) -> impl IntoElement {
        let login = Dialog::new(cx)
            .handle(self.handle.clone())
            .close_on_backdrop_press(true)
            .child(self.render_body(cx));
        let cookie = Dialog::new(cx)
            .handle(self.cookie_handle.clone())
            .close_on_backdrop_press(true)
            .child(self.render_cookie_body(cx));
        div().child(login).child(cookie)
    }

    fn render_body(&mut self, cx: &mut Context<crate::ui::Shell>) -> impl IntoElement {
        let tr = |source: &str| Self::tr(cx, CTX, source);
        let tr_cookie = |source: &str| Self::tr(cx, CTX_COOKIE, source);

        let status_text = match self.model.qr {
            QrPhase::Loading | QrPhase::WaitingForScan => {
                tr("Scan with the Bilibili app to log in")
            }
            QrPhase::WaitingForConfirmation => tr("Confirm login on your device"),
            QrPhase::Success => tr("Successfully logged in via QR code"),
            QrPhase::Expired => tr("QR code has expired"),
        };

        let qr_element: gpui_kit::gpui::AnyElement = match &self.qr_png {
            Some(image) => img(ImageSource::from(image.clone()))
                .size(px(super::qrcode_png::QR_DISPLAY_SIZE as f32))
                .into_any_element(),
            None => div()
                .size(px(super::qrcode_png::QR_DISPLAY_SIZE as f32))
                .into_any_element(),
        };

        let countdown_label = match self.model.send_button_label() {
            Some(n) => tr("Resend({countdown})").replace("{countdown}", &n.to_string()),
            None => tr("Get Code"),
        };
        let send_disabled = matches!(
            self.model.sms,
            SmsPhase::CodeSent | SmsPhase::CaptchaPending
        );
        let toast = self.toast.clone();

        div()
            .id("login-dialog-body")
            .flex()
            .gap_8()
            .p_6()
            .min_w(px(700.))
            .child(
                // left: QR
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .items_center()
                    .child(div().text_sm().child(tr("Scan QR Code to Log In")))
                    .child(qr_element)
                    .child(div().min_w(px(200.)).text_sm().child(status_text)),
            )
            .child(
                // right: SMS
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_sm().child(tr("SMS Login")))
                    .child(
                        div()
                            .flex()
                            .w_full()
                            .gap_2()
                            .child(div().w(px(100.)).child(self.render_cid_select()))
                            .child(div().flex_1().child(Input::new(&self.tel).id("login-tel"))),
                    )
                    .child(
                        div()
                            .flex()
                            .w_full()
                            .gap_2()
                            .child(div().flex_1().child(Input::new(&self.code).id("login-code")))
                            .child(
                                div().min_w(px(100.)).child(
                                    Button::new("login-send-code")
                                        .label(countdown_label)
                                        .disabled(send_disabled)
                                        .on_click(cx.listener(|shell, _, _, cx| {
                                            shell.login.send_code(cx);
                                        })),
                                ),
                            ),
                    )
                    .child(
                        div().w(px(175.)).child(
                            Button::new("login-submit")
                                .label(tr("Log In"))
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.login.sms_login(cx);
                                })),
                        ),
                    )
                    .child(
                        Button::new("login-cookies").label(tr_cookie("Log in with Cookies")).on_click(
                            cx.listener(|shell, _, window, cx| {
                                shell.login.cookie_handle.open(window, cx);
                            }),
                        ),
                    )
                    .children(toast.map(|t| div().id("login-toast").text_sm().child(t))),
            )
    }

    /// Python CidComboBox: a "+{code}" dropdown. gpui-kit has no Select
    /// yet; render the current selection as a label (the shared Select
    /// lands with the options/settings tickets).
    fn render_cid_select(&mut self) -> impl IntoElement {
        div().id("login-cid").child(format!("+{}", self.model.cid))
    }

    fn render_cookie_body(&mut self, cx: &mut Context<crate::ui::Shell>) -> impl IntoElement {
        let tr = |source: &str| Self::tr(cx, CTX_COOKIE, source);
        let verifying = self.cookie.verifying;
        let toast = self.cookie.error.clone();
        div()
            .id("cookie-dialog-body")
            .flex()
            .flex_col()
            .gap_3()
            .p_6()
            .min_w(px(500.))
            .child(div().text_sm().child(tr("Log in with Cookies")))
            .child(Input::new(&self.cookie_text).id("cookie-text"))
            .child(
                div()
                    .text_xs()
                    .child(tr("Support JSON format and semicolon-separated format")),
            )
            .child(
                Button::new("cookie-verify")
                    .label(tr("Log in with Cookies"))
                    .disabled(verifying)
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.login.cookie_verify(cx);
                    })),
            )
            .children(toast.map(|t| div().id("cookie-toast").text_sm().child(t)))
    }
}

impl Drop for LoginState {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            server.stop();
        }
    }
}

/// Keep `cid` linked into the binary (the table feeds the combo when the
/// shared Select widget lands).
#[allow(dead_code)]
fn _cid_table_is_used() -> &'static [(&'static str, &'static str)] {
    cid::CID_LIST
}

/// The shell's shared auth client (cookie jar survives across dialogs).
fn shell_client(cx: &Context<crate::ui::Shell>) -> Arc<network::HttpClient> {
    cx.entity().read_with(cx, |shell: &crate::ui::Shell, _| shell.auth_client.clone())
}

#[cfg(test)]
mod tests {
    use super::super::model::COUNTDOWN_START;

    #[test]
    fn countdown_start_matches_python() {
        assert_eq!(COUNTDOWN_START, 60);
    }
}
