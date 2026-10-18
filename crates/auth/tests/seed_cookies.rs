//! Ticket 34: playurl/parse clients must carry the persisted visitor
//! cookie set (and the login set when `is_login`) — the seeding helper's
//! public contract.

use auth::session::seed_client_cookies;
use network::HttpClient;

fn client() -> HttpClient {
    HttpClient::new(network::ClientOptions {
        user_agent: "bilidown-test".into(),
        proxy: network::ProxyConfig::none(),
        cookies_from_persistence: Vec::new(),
        signer: None,
    })
    .unwrap()
}

fn value_of<'a>(jar: &'a [network::PersistedCookie], name: &str) -> Option<&'a str> {
    jar.iter().find(|c| c.name == name).map(|c| c.value.as_str())
}

fn fake_state(is_login: bool) -> auth::AuthState {
    auth::AuthState {
        buvid3: "bu3".into(),
        buvid4: "bu4".into(),
        uuid: "uuid-1".into(),
        b_lsid: "LSID".into(),
        b_nut: 1_729_000_000,
        buvid_fp: "fp".into(),
        bili_ticket: "ticket".into(),
        bili_ticket_expires: 1_729_100_000,
        is_login,
        bili_jct: "jct".into(),
        dede_user_id: "42".into(),
        dede_user_id_ck_md5: "md5".into(),
        sessdata: "sess".into(),
        ..Default::default()
    }
}

#[test]
fn seeds_visitor_set_with_current_fnval_quality() {
    let client = client();
    seed_client_cookies(&client, &fake_state(false));
    let jar = client.persisted_cookie_snapshot();
    for (name, want) in [
        ("buvid3", "bu3"),
        ("buvid4", "bu4"),
        ("_uuid", "uuid-1"),
        ("b_lsid", "LSID"),
        ("b_nut", "1729000000"),
        ("buvid_fp", "fp"),
        ("bili_ticket", "ticket"),
        ("bili_ticket_expires", "1729100000"),
        ("CURRENT_FNVAL", "4048"),
        ("CURRENT_QUALITY", "0"),
    ] {
        assert_eq!(value_of(&jar, name), Some(want), "{name}");
    }
    // Every entry installed for the bilibili.com domain, path "/".
    for cookie in &jar {
        assert_eq!(cookie.domain, auth::session::BILIBILI_DOMAIN);
        assert_eq!(cookie.path, "/");
    }
}

#[test]
fn login_set_present_iff_is_login() {
    let logged_out = client();
    seed_client_cookies(&logged_out, &fake_state(false));
    let jar = logged_out.persisted_cookie_snapshot();
    for name in ["SESSDATA", "bili_jct", "DedeUserID", "DedeUserID__ckMd5"] {
        assert_eq!(value_of(&jar, name), None, "{name} must be absent");
    }

    let logged_in = client();
    seed_client_cookies(&logged_in, &fake_state(true));
    let jar = logged_in.persisted_cookie_snapshot();
    assert_eq!(value_of(&jar, "SESSDATA"), Some("sess"));
    assert_eq!(value_of(&jar, "bili_jct"), Some("jct"));
    assert_eq!(value_of(&jar, "DedeUserID"), Some("42"));
    assert_eq!(value_of(&jar, "DedeUserID__ckMd5"), Some("md5"));
}
