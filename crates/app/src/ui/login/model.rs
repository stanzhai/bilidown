//! Login dialog view-model — port of `src/gui/dialog/login.py`
//! (LoginDialog + CookieLoginDialog) state machine, gpui-free so the
//! countdown/status flows test headless. The render layer (`screen.rs`)
//! drives these transitions; `controller.rs` performs the network flows.

use auth::login::QRCodeScanStatus;

/// Where the SMS panel is: idle, waiting for the GeeTest challenge to be
/// solved (browser page open), or code sent (countdown running).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmsPhase {
    Idle,
    CaptchaPending,
    CodeSent,
    LoggingIn,
}

/// QR panel: image bytes are held by the screen; the model tracks the
/// poll status text key + whether polling continues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QrPhase {
    /// Waiting for the generate response.
    Loading,
    WaitingForScan,
    WaitingForConfirmation,
    Success,
    Expired,
}

impl From<QRCodeScanStatus> for QrPhase {
    fn from(s: QRCodeScanStatus) -> Self {
        match s {
            QRCodeScanStatus::WaitingForScan => Self::WaitingForScan,
            QRCodeScanStatus::WaitingForConfirmation => Self::WaitingForConfirmation,
            QRCodeScanStatus::Success => Self::Success,
            QRCodeScanStatus::Expired => Self::Expired,
        }
    }
}

/// Whether the 1s QR poll loop keeps going (Python `timer.isActive()`).
#[cfg_attr(not(test), allow(dead_code))] // used by model tests
pub fn qr_poll_continues(phase: QrPhase) -> bool {
    !matches!(phase, QrPhase::Success | QrPhase::Expired)
}

/// SMS countdown seconds (Python `SMSInfo.countdown`, fixed 60 → 1).
pub const COUNTDOWN_START: i32 = 60;

/// The login dialog's mutable state (all i18n happens at render time).
pub struct LoginModel {
    pub qr: QrPhase,
    /// QR url + key from `qr_generate` (empty until loaded).
    pub qr_key: String,
    pub sms: SmsPhase,
    pub cid: String,
    pub tel: String,
    pub code: String,
    pub countdown: i32,
    /// Error toast text (already localized by the caller) pending display.
    pub error: Option<String>,
    /// Success toast text pending display; closes the dialog after 300ms.
    pub success: Option<String>,
}

impl Default for LoginModel {
    fn default() -> Self {
        Self::new()
    }
}

impl LoginModel {
    pub fn new() -> Self {
        Self {
            qr: QrPhase::Loading,
            qr_key: String::new(),
            sms: SmsPhase::Idle,
            cid: crate::ui::login::cid::DEFAULT_CID.to_string(),
            tel: String::new(),
            code: String::new(),
            countdown: 0,
            error: None,
            success: None,
        }
    }

    /// `validate_input`: trim + empty check; on failure sets the error.
    pub fn require(&mut self, value: &str, message: String) -> bool {
        let ok = !value.trim().is_empty();
        if !ok {
            self.error = Some(message);
        }
        ok
    }

    /// QR generate landed: store the key, first poll pending.
    pub fn qr_generated(&mut self, qrcode_key: &str) {
        self.qr_key = qrcode_key.to_string();
        self.qr = QrPhase::WaitingForScan;
    }

    /// Poll result landed.
    pub fn qr_polled(&mut self, status: QRCodeScanStatus) {
        self.qr = status.into();
    }

    /// `on_send_verification` gate: tel non-empty → captcha pending.
    pub fn start_captcha(&mut self, tel_empty_message: String) -> bool {
        let tel = self.tel.clone();
        if !self.require(&tel, tel_empty_message) {
            return false;
        }
        self.sms = SmsPhase::CaptchaPending;
        true
    }

    /// `on_verfication_sent` — code sent, start the countdown.
    pub fn code_sent(&mut self) {
        self.countdown = COUNTDOWN_START;
        self.sms = SmsPhase::CodeSent;
    }

    /// `on_update_sms_countdown` — one timer tick. Returns true while the
    /// countdown continues.
    pub fn tick_countdown(&mut self) -> bool {
        if self.countdown <= 1 {
            // timer stops, button re-enables with the plain label
            self.countdown = 0;
            self.sms = SmsPhase::Idle;
            return false;
        }
        self.countdown -= 1;
        true
    }

    /// Resend-button label key: `Resend({countdown})` while counting,
    /// `Get Code` when idle. (Template substitution at render time.)
    pub fn send_button_label(&self) -> Option<i32> {
        (self.sms == SmsPhase::CodeSent).then_some(self.countdown)
    }

    /// `on_sms_login` gate: both fields non-empty.
    pub fn start_sms_login(
        &mut self,
        tel_empty: String,
        code_empty: String,
    ) -> bool {
        let tel = self.tel.clone();
        let code = self.code.clone();
        if !self.require(&tel, tel_empty) {
            return false;
        }
        if !self.require(&code, code_empty) {
            return false;
        }
        self.sms = SmsPhase::LoggingIn;
        true
    }

    /// Any flow error: surface the toast, leave the SMS button enabled.
    pub fn fail(&mut self, message: String) {
        self.sms = SmsPhase::Idle;
        self.error = Some(message);
    }

    /// `login_success` — success toast, dialog closes 300ms later.
    pub fn login_success(&mut self, message: String) {
        self.success = Some(message);
        self.sms = SmsPhase::Idle;
    }
}

/// Cookie-paste sub-dialog state (CookieLoginDialog).
pub struct CookieModel {
    pub text: String,
    pub verifying: bool,
    pub error: Option<String>,
    pub accepted: bool,
}

impl Default for CookieModel {
    fn default() -> Self {
        Self::new()
    }
}

impl CookieModel {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            verifying: false,
            error: None,
            accepted: false,
        }
    }

    /// `validate` — empty check, then verify (stays open until accepted).
    pub fn verify(&mut self, empty_message: String) -> bool {
        if self.verifying {
            return false;
        }
        if self.text.trim().is_empty() {
            self.error = Some(empty_message);
            return false;
        }
        self.verifying = true;
        false
    }

    /// `on_login_error` — re-enable, surface the toast.
    pub fn fail(&mut self, message: String) {
        self.verifying = false;
        self.error = Some(message);
    }

    /// `on_login_success` — accept closes the dialog.
    pub fn accept(&mut self) {
        self.accepted = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_phase_maps_python_status_codes() {
        assert_eq!(
            QrPhase::from(QRCodeScanStatus::WaitingForScan),
            QrPhase::WaitingForScan
        );
        assert_eq!(
            QrPhase::from(QRCodeScanStatus::WaitingForConfirmation),
            QrPhase::WaitingForConfirmation
        );
        assert_eq!(QrPhase::from(QRCodeScanStatus::Success), QrPhase::Success);
        assert_eq!(QrPhase::from(QRCodeScanStatus::Expired), QrPhase::Expired);
    }

    #[test]
    fn qr_poll_stops_on_success_and_expiry() {
        assert!(qr_poll_continues(QrPhase::WaitingForScan));
        assert!(qr_poll_continues(QrPhase::WaitingForConfirmation));
        assert!(!qr_poll_continues(QrPhase::Success));
        assert!(!qr_poll_continues(QrPhase::Expired));
    }

    #[test]
    fn send_code_requires_tel_and_enters_captcha_phase() {
        let mut m = LoginModel::new();
        assert!(!m.start_captcha("tel empty".into()));
        assert_eq!(m.error.as_deref(), Some("tel empty"));
        m.tel = "13800138000".into();
        assert!(m.start_captcha("tel empty".into()));
        assert_eq!(m.sms, SmsPhase::CaptchaPending);
    }

    #[test]
    fn countdown_runs_60_to_reenable() {
        let mut m = LoginModel::new();
        m.code_sent();
        assert_eq!(m.countdown, 60);
        assert_eq!(m.send_button_label(), Some(60));
        // 59 ticks: 60→1, button still counting
        for expected in (1..60).rev() {
            assert!(m.tick_countdown(), "still ticking at {expected}");
            assert_eq!(m.countdown, expected);
        }
        assert_eq!(m.countdown, 1);
        // the tick that hits 1 stops the timer and re-enables
        assert!(!m.tick_countdown());
        assert_eq!(m.countdown, 0);
        assert_eq!(m.send_button_label(), None);
        assert_eq!(m.sms, SmsPhase::Idle);
    }

    #[test]
    fn sms_login_requires_both_fields() {
        let mut m = LoginModel::new();
        assert!(!m.start_sms_login("tel".into(), "code".into()));
        assert_eq!(m.error.as_deref(), Some("tel"));
        m.tel = "13800138000".into();
        assert!(!m.start_sms_login("tel".into(), "code".into()));
        assert_eq!(m.error.as_deref(), Some("code"));
        m.code = "123456".into();
        assert!(m.start_sms_login("tel".into(), "code".into()));
        assert_eq!(m.sms, SmsPhase::LoggingIn);
    }

    #[test]
    fn cookie_dialog_gates_and_accepts() {
        let mut m = CookieModel::new();
        assert!(!m.verify("empty".into()));
        assert_eq!(m.error.as_deref(), Some("empty"));
        m.text = "SESSDATA=abc; bili_jct=x".into();
        assert!(!m.verify("empty".into()), "stays open while verifying");
        assert!(m.verifying);
        assert!(!m.verify("again".into()), "second click ignored");
        m.fail("invalid".into());
        assert!(!m.verifying);
        assert_eq!(m.error.as_deref(), Some("invalid"));
        m.accept();
        assert!(m.accepted);
    }

    #[test]
    fn login_success_sets_toast_and_resets_sms_phase() {
        let mut m = LoginModel::new();
        m.sms = SmsPhase::LoggingIn;
        m.login_success("ok".into());
        assert_eq!(m.success.as_deref(), Some("ok"));
        assert_eq!(m.sms, SmsPhase::Idle);
    }
}
