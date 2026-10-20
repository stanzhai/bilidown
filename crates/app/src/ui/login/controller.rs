//! Login controller — the network half of `gui/dialog/login.py`, ported
//! onto `auth::login`. Endpoints are injectable so wiremock tests can
//! swap hosts (auth's consts are the defaults; the crate stays untouched).

use auth::login::{self, CaptchaInfo, CookieLoginOutcome};
use auth::session::{config_path, AuthState, Session};
use auth::wbi::WbiKeyStore;
use network::HttpClient;

use super::server::{CaptchaServer, CaptchaSolved};

/// Default login endpoints = `auth::login` consts. Tests replace hosts.
#[derive(Debug, Clone)]
pub struct LoginEndpoints {
    pub qr_generate: String,
    pub qr_poll: String,
    /// Test seam (wiremock host rewrite); production reaches the captcha
    /// API through `login::fetch_captcha` directly.
    #[allow(dead_code)]
    pub captcha: String,
    pub sms_send: String,
    pub sms_login: String,
    pub nav: String,
}

impl Default for LoginEndpoints {
    fn default() -> Self {
        Self {
            qr_generate: login::QR_GENERATE_API.to_owned(),
            qr_poll: login::QR_POLL_API.to_owned(),
            captcha: login::CAPTCHA_API.to_owned(),
            sms_send: login::SMS_SEND_API.to_owned(),
            sms_login: login::SMS_LOGIN_API.to_owned(),
            nav: auth::wbi::NAV_API.to_owned(),
        }
    }
}

/// `parsers::list::swap_host` — replace the `https://host` of `api` with
/// `base`, keeping path + query. Empty base = no-op.
#[cfg_attr(not(test), allow(dead_code))] // test seam: wiremock host rewrite
pub fn swap_host(api: &str, base: &str) -> String {
    if base.is_empty() {
        return api.to_owned();
    }
    let path = api
        .strip_prefix("https://")
        .and_then(|rest| rest.find('/').map(|i| &rest[i..]))
        .unwrap_or(api);
    format!("{base}{path}")
}

/// Host[:port] of an `http(s)://` URL — where its `Set-Cookie`s live.
fn url_host(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    Some(rest.split('/').next()?.to_owned())
}

/// One QR round: generate + paint. Returns `(png, qrcode_key)`.
pub async fn qr_start(
    client: &HttpClient,
    endpoints: &LoginEndpoints,
) -> Result<(Vec<u8>, String), String> {
    let (url, key) = fetch_qr(client, endpoints).await?;
    let png = super::qrcode_png::qr_png(&url)?;
    Ok((png, key))
}

async fn fetch_qr(
    client: &HttpClient,
    endpoints: &LoginEndpoints,
) -> Result<(String, String), String> {
    let params = vec![
        ("source".to_owned(), "main-fe-header".to_owned()),
        ("go_url".to_owned(), "https://www.bilibili.com/".to_owned()),
        ("web_location".to_owned(), "333.1007".to_owned()),
    ];
    let response: serde_json::Value = client
        .get_json(&endpoints.qr_generate, &params)
        .await
        .map_err(|e| e.to_string())?;
    login_check(&response)?;
    let data = &response["data"];
    Ok((
        data["url"].as_str().unwrap_or_default().to_owned(),
        data["qrcode_key"].as_str().unwrap_or_default().to_owned(),
    ))
}

/// One QR poll tick; on Success cookies are captured into `state`.
pub async fn qr_poll_once(
    client: &HttpClient,
    endpoints: &LoginEndpoints,
    qrcode_key: &str,
    state: &mut AuthState,
    user_agent: &str,
) -> Result<login::QRCodeScanStatus, String> {
    let params = vec![("qrcode_key".to_owned(), qrcode_key.to_owned())];
    let response: serde_json::Value = client
        .get_json(&endpoints.qr_poll, &params)
        .await
        .map_err(|e| e.to_string())?;
    login_check(&response)?;
    let code = response["data"]["code"].as_i64().unwrap_or(86038);
    let status = login::QRCodeScanStatus::from_code(code);
    if status == login::QRCodeScanStatus::Success {
        // Harvest at the host the poll response (and its Set-Cookies)
        // came from — `session_for`'s default assumes bilibili.com.
        let host = url_host(&endpoints.qr_poll).unwrap_or_else(|| "bilibili.com".into());
        let mut session = session_for_domain(client, state, user_agent, &host);
        session.capture_login_cookies();
    }
    Ok(status)
}

/// `init_geetest` — fetch captcha info, start the loopback server, open
/// the page. Returns the server handle for the callback wait.
pub async fn captcha_start(client: &HttpClient) -> Result<CaptchaServer, String> {
    let info: CaptchaInfo = login::fetch_captcha(client)
        .await
        .map_err(|e| e.to_string())?;
    let server = CaptchaServer::start().await.map_err(|e| e.to_string())?;
    server.set_captcha(info);
    super::captcha_page::open_captcha_page().map_err(|e| e.to_string())?;
    Ok(server)
}

/// Drive `sms_send` with the solved GeeTest values (Python: the server
/// thread fired `signal_bus.login.send_sms`). `captcha_token` comes from
/// the original `fetch_captcha` that seeded the challenge.
pub async fn sms_send_with_captcha(
    client: &HttpClient,
    endpoints: &LoginEndpoints,
    cid: &str,
    tel: &str,
    solved: CaptchaSolved,
    captcha_token: &str,
) -> Result<String, String> {
    let captcha = CaptchaInfo {
        token: captcha_token.to_owned(),
        challenge: solved.challenge,
        gt: String::new(),
        seccode: solved.seccode,
        validate: solved.validate,
        captcha_key: String::new(),
    };
    let params = vec![
        ("cid".to_owned(), cid.to_owned()),
        ("tel".to_owned(), tel.to_owned()),
        ("source".to_owned(), "main-fe-header".to_owned()),
        ("token".to_owned(), captcha.token.clone()),
        ("challenge".to_owned(), captcha.challenge.clone()),
        ("validate".to_owned(), captcha.validate.clone()),
        ("seccode".to_owned(), captcha.seccode.clone()),
    ];
    let response: serde_json::Value = client
        .post_json(&endpoints.sms_send, &params, &serde_json::Value::Null)
        .await
        .map_err(|e| e.to_string())?;
    login_check(&response)?;
    Ok(response["data"]["captcha_key"]
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

/// `on_login` — exchange the SMS code for login cookies. `cid/tel/code/
/// captcha_key` mirror the Python request form fields.
pub async fn sms_login_with(
    client: &HttpClient,
    endpoints: &LoginEndpoints,
    form: &SmsLoginForm<'_>,
    state: &mut AuthState,
    user_agent: &str,
) -> Result<(), String> {
    let params = vec![
        ("cid".to_owned(), form.cid.to_owned()),
        ("tel".to_owned(), form.tel.to_owned()),
        ("code".to_owned(), form.code.to_owned()),
        ("source".to_owned(), "main-fe-header".to_owned()),
        ("captcha_key".to_owned(), form.captcha_key.to_owned()),
        ("go_url".to_owned(), "https://www.bilibili.com/".to_owned()),
    ];
    let response: serde_json::Value = client
        .post_json(&endpoints.sms_login, &params, &serde_json::Value::Null)
        .await
        .map_err(|e| e.to_string())?;
    login_check(&response)?;
    let mut session = session_for(client, state, user_agent);
    session.capture_login_cookies();
    Ok(())
}

/// The four SMS login form fields (`on_login` request body).
pub struct SmsLoginForm<'a> {
    pub cid: &'a str,
    pub tel: &'a str,
    pub code: &'a str,
    pub captcha_key: &'a str,
}

/// Cookie paste — full parse → apply → verify → capture/restore cycle
/// against an injectable nav endpoint (auth's own `cookie_login` pins the
/// real nav URL; this port keeps the wire contract and adds the seam).
pub async fn cookie_verify(
    client: &HttpClient,
    endpoints: &LoginEndpoints,
    text: &str,
    wbi: &WbiKeyStore,
    state: &mut AuthState,
    user_agent: &str,
) -> Result<CookieLoginOutcome, login::CookieLoginError> {
    let cookies = login::parse_cookies(text);
    if cookies.is_empty() {
        return Err(login::CookieLoginError::FormatInvalid);
    }
    let get = |name: &str| {
        cookies
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
            .unwrap_or_default()
    };
    if get("SESSDATA").is_empty() {
        return Err(login::CookieLoginError::MissingSessdata);
    }
    let domain = "bilibili.com";
    for key in login::LOGIN_COOKIE_KEYS {
        let value = get(key);
        if !value.is_empty() {
            client.set_cookie(key, value, domain, "/");
        }
    }
    let response: serde_json::Value = client
        .get_json(&endpoints.nav, &[])
        .await
        .map_err(login::CookieLoginError::Http)?;
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
        let mut session = session_for(client, state, user_agent);
        session.capture_login_cookies();
        Ok(CookieLoginOutcome::Success)
    } else {
        for key in login::LOGIN_COOKIE_KEYS {
            client.set_cookie(key, "", domain, "/");
        }
        let session = session_for(client, state, user_agent);
        session.sync_anonymous_cookies();
        if session.state.is_login {
            session.sync_login_cookies();
        }
        Ok(CookieLoginOutcome::Invalid)
    }
}

/// `check_response` — bilibili's `{"code": 0}` envelope.
fn login_check(response: &serde_json::Value) -> Result<(), String> {
    let code = response.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
    if code == 0 {
        Ok(())
    } else {
        Err(response
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("request failed")
            .to_owned())
    }
}

pub fn session_for<'a>(
    client: &'a HttpClient,
    state: &'a mut AuthState,
    user_agent: &str,
) -> Session<'a> {
    session_for_domain(client, state, user_agent, "bilibili.com")
}

/// `session_for` with the cookie-capture domain made explicit: tests
/// point it at the mock host so host-only `Set-Cookie`s land where the
/// harvest looks.
pub fn session_for_domain<'a>(
    client: &'a HttpClient,
    state: &'a mut AuthState,
    user_agent: &str,
    domain: &'a str,
) -> Session<'a> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Session {
        client,
        state,
        user_agent: user_agent.to_owned(),
        domain,
        now,
        now_ms,
        rng_seed: 0,
    }
}

/// Persist `state` into the config.json next to `user_data_dir` (Python:
/// `Config` saves `Cookie` after every login success).
pub fn persist(state: &AuthState, user_data_dir: &std::path::Path) -> std::io::Result<()> {
    state.save_to_config_file(&config_path(user_data_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_host_keeps_path_and_query() {
        assert_eq!(
            swap_host(
                "https://passport.bilibili.com/x/passport-login/web/qrcode/poll?qrcode_key=k",
                "http://localhost:1"
            ),
            "http://localhost:1/x/passport-login/web/qrcode/poll?qrcode_key=k"
        );
        assert_eq!(swap_host("https://a.b/c", ""), "https://a.b/c");
    }

    #[test]
    fn defaults_pin_the_auth_consts() {
        let e = LoginEndpoints::default();
        assert_eq!(e.qr_generate, login::QR_GENERATE_API);
        assert_eq!(e.sms_send, login::SMS_SEND_API);
        assert_eq!(e.nav, auth::wbi::NAV_API);
    }

    // ---- wiremock flows (spec: Testing Decisions — mock_client pattern
    // mirrors `parsers/tests/list_parsers.rs`) ----

    use network::{ClientOptions, HttpClient, ProxyConfig};
    use serde_json::json;

    fn mock_client() -> HttpClient {
        HttpClient::new(ClientOptions {
            user_agent: String::new(),
            proxy: ProxyConfig::none(),
            cookies_from_persistence: Vec::new(),
            signer: None,
        })
        .expect("rustls client builds")
    }

    fn endpoints_for(server: &wiremock::MockServer) -> LoginEndpoints {
        let base = server.uri();
        let mut e = LoginEndpoints::default();
        e.qr_generate = swap_host(&e.qr_generate, &base);
        e.qr_poll = swap_host(&e.qr_poll, &base);
        e.sms_send = swap_host(&e.sms_send, &base);
        e.sms_login = swap_host(&e.sms_login, &base);
        e.nav = swap_host(&e.nav, &base);
        e
    }

    #[tokio::test]
    async fn qr_flow_generates_polls_captures_and_persists() {
        let server = wiremock::MockServer::start().await;
        let endpoints = endpoints_for(&server);

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(
                "/x/passport-login/web/qrcode/generate",
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "code": 0,
                "data": {
                    "url": "https://passport.bilibili.com/h5-app/passport/login/scan?qrcode_key=deadbeef",
                    "qrcode_key": "deadbeef"
                }
            })))
            .mount(&server)
            .await;

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/x/passport-login/web/qrcode/poll"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(json!({
                        "code": 0,
                        "data": {"code": 0, "url": "https://www.bilibili.com/", "refresh_token": "rt"}
                    }))
                    .append_header("set-cookie", "SESSDATA=abc; Path=/")
                    .append_header("set-cookie", "bili_jct=csrf; Path=/")
                    .append_header("set-cookie", "DedeUserID=42; Path=/")
                    .append_header("set-cookie", "DedeUserID__ckMd5=md5; Path=/"),
            )
            .mount(&server)
            .await;

        let client = mock_client();
        let (png, key) = controller_qr_start(&client, &endpoints).await;
        assert_eq!(key, "deadbeef");
        assert_eq!(&png[..4], b"\x89PNG");

        let mut state = AuthState::default();
        let status = qr_poll_once(&client, &endpoints, &key, &mut state, "ua-test")
            .await
            .expect("poll");
        assert_eq!(status, login::QRCodeScanStatus::Success);
        assert!(state.is_login);

        let dir = tempfile::tempdir().expect("tempdir");
        persist(&state, dir.path()).expect("persist");
        let reloaded = AuthState::from_config_file(&config_path(dir.path())).expect("reload");
        assert!(reloaded.is_login);
        assert_eq!(reloaded.sessdata, state.sessdata);
    }

    async fn controller_qr_start(
        client: &HttpClient,
        endpoints: &LoginEndpoints,
    ) -> (Vec<u8>, String) {
        qr_start(client, endpoints).await.expect("qr_start")
    }

    #[tokio::test]
    async fn qr_poll_reports_waiting_then_expires() {
        let server = wiremock::MockServer::start().await;
        let endpoints = endpoints_for(&server);
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(
                "/x/passport-login/web/qrcode/poll",
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "code": 0, "data": {"code": 86090}
            })))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(
                "/x/passport-login/web/qrcode/poll",
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "code": 0, "data": {"code": 86038}
            })))
            .mount(&server)
            .await;

        let client = mock_client();
        let mut state = AuthState::default();
        let s = qr_poll_once(&client, &endpoints, "k", &mut state, "ua")
            .await
            .unwrap();
        assert_eq!(s, login::QRCodeScanStatus::WaitingForConfirmation);
        assert!(!state.is_login);

        let s = qr_poll_once(&client, &endpoints, "k", &mut state, "ua")
            .await
            .unwrap();
        assert_eq!(s, login::QRCodeScanStatus::Expired);
    }

    #[tokio::test]
    async fn cookie_verify_matrix() {
        let server = wiremock::MockServer::start().await;
        let endpoints = endpoints_for(&server);
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/x/web-interface/nav"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "code": 0,
                "data": {"isLogin": true, "wbi_img": {"img_url": "i", "sub_url": "s"}}
            })))
            .mount(&server)
            .await;

        let client = mock_client();
        let wbi = WbiKeyStore::new();

        // semicolon-separated → Success
        let mut state = AuthState::default();
        let r = cookie_verify(
            &client,
            &endpoints,
            "SESSDATA=sd;bili_jct=jct;DedeUserID=7;DedeUserID__ckMd5=ck",
            &wbi,
            &mut state,
            "ua",
        )
        .await
        .unwrap();
        assert!(matches!(r, CookieLoginOutcome::Success));
        assert!(state.is_login);

        // JSON dict → Success
        let mut state = AuthState::default();
        let j =
            r#"{"SESSDATA":"sd2","bili_jct":"jct2","DedeUserID":"8","DedeUserID__ckMd5":"ck2"}"#;
        let r = cookie_verify(&client, &endpoints, j, &wbi, &mut state, "ua")
            .await
            .unwrap();
        assert!(matches!(r, CookieLoginOutcome::Success));

        // missing SESSDATA
        let mut state = AuthState::default();
        let r = cookie_verify(&client, &endpoints, "bili_jct=x", &wbi, &mut state, "ua").await;
        assert!(matches!(r, Err(login::CookieLoginError::MissingSessdata)));

        // garbage format
        let mut state = AuthState::default();
        let r = cookie_verify(&client, &endpoints, "garbage", &wbi, &mut state, "ua").await;
        assert!(matches!(r, Err(login::CookieLoginError::FormatInvalid)));
    }

    #[tokio::test]
    async fn cookie_verify_rolls_back_when_nav_says_logged_out() {
        let server = wiremock::MockServer::start().await;
        let endpoints = endpoints_for(&server);
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/x/web-interface/nav"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "code": 0, "data": {"isLogin": false}
            })))
            .mount(&server)
            .await;

        let client = mock_client();
        let wbi = WbiKeyStore::new();
        let mut state = AuthState::default();
        let r = cookie_verify(
            &client,
            &endpoints,
            "SESSDATA=sd;bili_jct=jct;DedeUserID=7;DedeUserID__ckMd5=ck",
            &wbi,
            &mut state,
            "ua",
        )
        .await
        .unwrap();
        assert!(matches!(r, CookieLoginOutcome::Invalid));
        assert!(!state.is_login);
    }

    #[tokio::test]
    async fn sms_send_posts_the_geetest_params() {
        let server = wiremock::MockServer::start().await;
        let endpoints = endpoints_for(&server);
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/x/passport-login/web/sms/send"))
            .and(wiremock::matchers::query_param("cid", "86"))
            .and(wiremock::matchers::query_param("tel", "13800000000"))
            .and(wiremock::matchers::query_param("token", "tok1"))
            .and(wiremock::matchers::query_param("challenge", "ch1"))
            .and(wiremock::matchers::query_param("validate", "val1"))
            .and(wiremock::matchers::query_param("seccode", "val1"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "code": 0, "data": {"captcha_key": "ck-1"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client();
        let solved = CaptchaSolved {
            challenge: "ch1".into(),
            seccode: "val1".into(),
            validate: "val1".into(),
        };
        let key = sms_send_with_captcha(&client, &endpoints, "86", "13800000000", solved, "tok1")
            .await
            .expect("send");
        assert_eq!(key, "ck-1");
    }

    #[tokio::test]
    async fn sms_login_exchanges_code_for_cookies() {
        let server = wiremock::MockServer::start().await;
        let endpoints = endpoints_for(&server);
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/x/passport-login/web/login/sms"))
            .and(wiremock::matchers::query_param("cid", "86"))
            .and(wiremock::matchers::query_param("tel", "13800000000"))
            .and(wiremock::matchers::query_param("code", "123456"))
            .and(wiremock::matchers::query_param("captcha_key", "ck-1"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(json!({
                        "code": 0,
                        "data": {"url": "https://www.bilibili.com/", "refresh_token": "rt"}
                    }))
                    .append_header("set-cookie", "SESSDATA=sd3; Path=/; Domain=.bilibili.com")
                    .append_header("set-cookie", "bili_jct=jct3; Path=/; Domain=.bilibili.com")
                    .append_header("set-cookie", "DedeUserID=9; Path=/; Domain=.bilibili.com")
                    .append_header(
                        "set-cookie",
                        "DedeUserID__ckMd5=ckm3; Path=/; Domain=.bilibili.com",
                    ),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client();
        let mut state = AuthState::default();
        sms_login_with(
            &client,
            &endpoints,
            &SmsLoginForm {
                cid: "86",
                tel: "13800000000",
                code: "123456",
                captcha_key: "ck-1",
            },
            &mut state,
            "ua",
        )
        .await
        .expect("sms login");
        assert!(state.is_login);
    }

    #[tokio::test]
    async fn login_check_surfaces_bilibili_error_messages() {
        let server = wiremock::MockServer::start().await;
        let endpoints = endpoints_for(&server);
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/x/passport-login/web/login/sms"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "code": -449, "message": "wrong sms code"
            })))
            .mount(&server)
            .await;

        let client = mock_client();
        let mut state = AuthState::default();
        let err = sms_login_with(
            &client,
            &endpoints,
            &SmsLoginForm {
                cid: "86",
                tel: "13800000000",
                code: "000000",
                captcha_key: "ck-x",
            },
            &mut state,
            "ua",
        )
        .await
        .unwrap_err();
        assert_eq!(err, "wrong sms code");
        assert!(!state.is_login);
    }
}
