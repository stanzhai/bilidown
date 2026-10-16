//! Login flows — QR (`util/auth/qrcode.py`), SMS + GeeTest captcha
//! (`util/auth/sms.py`, `util/auth/captcha.py`), manual cookie paste
//! (`util/auth/cookie_login.py`), and logout (in `util/auth/user.py`).
//!
//! GeeTest is data-structures only here; rendering the challenge is
//! ticket 23.

use serde::{Deserialize, Serialize};

use crate::session::{AuthError, Session};
use network::HttpClient;

/// `https://passport.bilibili.com/x/passport-login/web/qrcode/generate`.
pub const QR_GENERATE_API: &str =
    "https://passport.bilibili.com/x/passport-login/web/qrcode/generate";
/// `.../web/qrcode/poll`.
pub const QR_POLL_API: &str = "https://passport.bilibili.com/x/passport-login/web/qrcode/poll";
/// `https://passport.bilibili.com/x/passport-login/captcha` (Python pins
/// the hard-coded `t=0.1867987009754133`).
pub const CAPTCHA_API: &str =
    "https://passport.bilibili.com/x/passport-login/captcha?source=main-fe-header&t=0.1867987009754133";
/// `.../web/sms/send`.
pub const SMS_SEND_API: &str = "https://passport.bilibili.com/x/passport-login/web/sms/send";
/// `.../web/login/sms`.
pub const SMS_LOGIN_API: &str = "https://passport.bilibili.com/x/passport-login/web/login/sms";
/// `https://passport.bilibili.com/login/exit/v2`.
pub const LOGOUT_API: &str = "https://passport.bilibili.com/login/exit/v2";

/// Python `QRCodeScanStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QRCodeScanStatus {
    WaitingForScan = 86101,
    WaitingForConfirmation = 86090,
    Success = 0,
    Expired = 86038,
}

impl QRCodeScanStatus {
    pub fn from_code(code: i64) -> Self {
        match code {
            86101 => Self::WaitingForScan,
            86090 => Self::WaitingForConfirmation,
            0 => Self::Success,
            _ => Self::Expired,
        }
    }
}

/// `CaptchaInfo` — GeeTest fields fetched once, filled by the webview
/// (ticket 23), then attached to the SMS send.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaptchaInfo {
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub challenge: String,
    #[serde(default)]
    pub gt: String,
    #[serde(default)]
    pub seccode: String,
    #[serde(default)]
    pub validate: String,
    #[serde(default)]
    pub captcha_key: String,
}

/// `init_geetest` — fetch token + geetest challenge/gt.
pub async fn fetch_captcha(client: &HttpClient) -> Result<CaptchaInfo, AuthError> {
    let response: serde_json::Value = client
        .get_json::<serde_json::Value>(CAPTCHA_API, &[])
        .await?;
    check_response(&response)?;
    let data = &response["data"];
    Ok(CaptchaInfo {
        token: data["token"].as_str().unwrap_or_default().to_owned(),
        challenge: data["geetest"]["challenge"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        gt: data["geetest"]["gt"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        seccode: String::new(),
        validate: String::new(),
        captcha_key: String::new(),
    })
}

/// QR login: generate returns the URL to render (ticket 23's pixmap) and
/// the `qrcode_key` to poll with.
pub async fn qr_generate(client: &HttpClient) -> Result<(String, String), AuthError> {
    let params = vec![
        ("source".to_owned(), "main-fe-header".to_owned()),
        ("go_url".to_owned(), "https://www.bilibili.com/".to_owned()),
        ("web_location".to_owned(), "333.1007".to_owned()),
    ];
    let response: serde_json::Value = client
        .get_json::<serde_json::Value>(QR_GENERATE_API, &params)
        .await?;
    check_response(&response)?;
    let data = &response["data"];
    Ok((
        data["url"].as_str().unwrap_or_default().to_owned(),
        data["qrcode_key"].as_str().unwrap_or_default().to_owned(),
    ))
}

/// Poll result: scan status; on [`QRCodeScanStatus::Success`] the login
/// cookies have already landed in the client jar and `session` captured
/// them into state (Python `update_cookies` in `on_poll_success`).
pub async fn qr_poll(
    client: &HttpClient,
    qrcode_key: &str,
    session: &mut Session<'_>,
) -> Result<QRCodeScanStatus, AuthError> {
    let params = vec![("qrcode_key".to_owned(), qrcode_key.to_owned())];
    let response: serde_json::Value = client
        .get_json::<serde_json::Value>(QR_POLL_API, &params)
        .await?;
    check_response(&response)?;
    let code = response["data"]["code"].as_i64().unwrap_or(86038);
    let status = QRCodeScanStatus::from_code(code);
    if status == QRCodeScanStatus::Success {
        session.capture_login_cookies();
    }
    Ok(status)
}

/// `SMSInfo` inputs for one send/login round.
#[derive(Debug, Clone, Default)]
pub struct SmsInfo {
    pub cid: String,
    pub tel: String,
    pub verification_code: String,
}

/// `SMS.send` — params must carry the GeeTest `token`/`challenge`/
/// `validate`/`seccode` exactly as Python assembles them.
pub async fn sms_send(
    client: &HttpClient,
    sms: &SmsInfo,
    captcha: &CaptchaInfo,
) -> Result<String, AuthError> {
    let params = vec![
        ("cid".to_owned(), sms.cid.clone()),
        ("tel".to_owned(), sms.tel.clone()),
        ("source".to_owned(), "main-fe-header".to_owned()),
        ("token".to_owned(), captcha.token.clone()),
        ("challenge".to_owned(), captcha.challenge.clone()),
        ("validate".to_owned(), captcha.validate.clone()),
        ("seccode".to_owned(), captcha.seccode.clone()),
    ];
    let response: serde_json::Value = client
        .post_json(SMS_SEND_API, &params, &serde_json::Value::Null)
        .await?;
    check_response(&response)?;
    Ok(response["data"]["captcha_key"]
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

/// `SMS.login` — exchanges the SMS code + captcha_key for login cookies.
/// On success the cookies are in the jar and captured into `session`.
pub async fn sms_login(
    client: &HttpClient,
    sms: &SmsInfo,
    captcha_key: &str,
    session: &mut Session<'_>,
) -> Result<(), AuthError> {
    let params = vec![
        ("cid".to_owned(), sms.cid.clone()),
        ("tel".to_owned(), sms.tel.clone()),
        ("code".to_owned(), sms.verification_code.clone()),
        ("source".to_owned(), "main-fe-header".to_owned()),
        ("captcha_key".to_owned(), captcha_key.to_owned()),
        ("go_url".to_owned(), "https://www.bilibili.com/".to_owned()),
    ];
    let response: serde_json::Value = client
        .post_json(SMS_LOGIN_API, &params, &serde_json::Value::Null)
        .await?;
    check_response(&response)?;
    session.capture_login_cookies();
    Ok(())
}

/// Cookie paste (`CookieLogin.login`): parse, require SESSDATA, apply the
/// four login keys, verify via nav. On success keys are captured; on
/// invalid cookies the previously persisted state is restored.
pub enum CookieLoginOutcome {
    /// nav said `isLogin: true` — cookies applied and captured.
    Success,
    /// nav replied but `isLogin` false — cookies rolled back.
    Invalid,
}

/// `parse_cookies` — accept a `Cookie:` header string, `k=v; …` lines,
/// or a JSON object (str:str only). Empty map when nothing parses.
pub fn parse_cookies(text: &str) -> Vec<(String, String)> {
    let text = text.trim();
    let text = text
        .strip_prefix("cookie:")
        .or_else(|| text.strip_prefix("Cookie:"))
        .unwrap_or(text);

    // JSON object with all-string values wins outright
    if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(text) {
        let cookies: Vec<(String, String)> = map
            .into_iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_owned())))
            .collect();
        return cookies;
    }

    let mut cookies = Vec::new();
    for part in text.replace('\n', ";").split(';') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"');
        if !key.is_empty() && !key.chars().any(char::is_whitespace) {
            cookies.push((key.to_owned(), value.to_owned()));
        }
    }
    cookies
}

/// Keys applied from a pasted cookie (Python `LOGIN_COOKIE_KEYS`).
pub const LOGIN_COOKIE_KEYS: [&str; 4] =
    ["SESSDATA", "bili_jct", "DedeUserID", "DedeUserID__ckMd5"];

/// Full cookie-paste login: parse → validate → apply → verify via nav →
/// capture or restore.
pub async fn cookie_login(
    client: &HttpClient,
    text: &str,
    wbi: &crate::wbi::WbiKeyStore,
    session: &mut Session<'_>,
) -> Result<CookieLoginOutcome, CookieLoginError> {
    let cookies = parse_cookies(text);
    if cookies.is_empty() {
        return Err(CookieLoginError::FormatInvalid);
    }
    let get = |name: &str| {
        cookies
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
            .unwrap_or_default()
    };
    if get("SESSDATA").is_empty() {
        return Err(CookieLoginError::MissingSessdata);
    }

    // Apply the login keys
    for key in LOGIN_COOKIE_KEYS {
        let value = get(key);
        if !value.is_empty() {
            client.set_cookie(key, value, session.domain, "/");
        }
    }

    let response: serde_json::Value = client
        .get_json::<serde_json::Value>(crate::wbi::NAV_API, &[])
        .await
        .map_err(CookieLoginError::Http)?;
    let data = response
        .get("data")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    wbi.store_from_nav(&data);

    if data
        .get("isLogin")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        session.capture_login_cookies();
        Ok(CookieLoginOutcome::Success)
    } else {
        // Restore: delete the applied login cookies, re-sync from state
        for key in LOGIN_COOKIE_KEYS {
            client.set_cookie(key, "", session.domain, "/");
        }
        session.sync_anonymous_cookies();
        if session.state.is_login {
            session.sync_login_cookies();
        }
        Ok(CookieLoginOutcome::Invalid)
    }
}

/// Cookie paste failures (Python `COOKIE_FORMAT_INVALID` /
/// `COOKIE_MISSING_SESSDATA` / `COOKIE_INVALID`).
#[derive(Debug, thiserror::Error)]
pub enum CookieLoginError {
    #[error("Cookie 格式无效")]
    FormatInvalid,
    #[error("缺少 SESSDATA")]
    MissingSessdata,
    #[error("请求失败: {0}")]
    Http(#[from] network::HttpError),
}

/// `UserManager.logout` — POST exit, then clear login state on success.
pub async fn logout(client: &HttpClient, session: &mut Session<'_>) -> Result<(), AuthError> {
    let params = vec![("biliCSRF".to_owned(), session.state.bili_jct.clone())];
    let response: serde_json::Value = client
        .post_json(LOGOUT_API, &params, &serde_json::Value::Null)
        .await?;
    check_response(&response)?;
    session.clear_login();
    Ok(())
}

fn check_response(response: &serde_json::Value) -> Result<(), AuthError> {
    let code = response.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if code != 0 {
        let message = response
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        return Err(AuthError::Response(message.to_owned()));
    }
    Ok(())
}
