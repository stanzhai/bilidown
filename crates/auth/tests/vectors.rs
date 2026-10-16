//! Ported verbatim from `test/test_wbi_signature.py` + generated
//! reference vectors.

use auth::*;

const TESTUA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36 Edg/147.0.0.0";

#[test]
fn mixin_key_permutation() {
    let img = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let sub = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let key = wbi::mixin_key(img, sub).expect("keys long enough");
    assert_eq!(key.len(), 32);
    // spot-check the permutation: first tab index 46 -> 'b'
    assert!(key.starts_with('b'));
}

#[test]
fn enc_wbi_reference_vector() {
    let img = "7cd084941338484aae1ad9425b84077c";
    let sub = "4932caff0ff746eab6f01bf08b70ac45";
    let params = vec![("foo".to_owned(), "bar'b(a)z!*".to_owned()), ("n".to_owned(), "12".to_owned())];
    let signed = wbi::enc_wbi(&params, img, sub, 1_772_841_600).expect("sign ok");
    assert_eq!(signed.w_rid, "ee8d391467694942682caea256bab88d");
    assert_eq!(signed.query(), "foo=barbaz&n=12&wts=1772841600&w_rid=ee8d391467694942682caea256bab88d");
}

#[test]
fn enc_wbi_second_vector() {
    let img = "a".repeat(32);
    let sub = "b".repeat(32);
    let params = vec![
        ("bvid".to_owned(), "BV1xx411c7mD".to_owned()),
        ("cid".to_owned(), "456".to_owned()),
    ];
    let signed = wbi::enc_wbi(&params, &img, &sub, 1_700_000_000).expect("sign ok");
    assert_eq!(signed.w_rid, "e920af68b8bd1253325ff8264c67c94a");
    assert_eq!(
        signed.query(),
        "bvid=BV1xx411c7mD&cid=456&wts=1700000000&w_rid=e920af68b8bd1253325ff8264c67c94a"
    );
}

#[test]
fn wbi_key_length_matches_table() {
    assert_eq!(wbi::WBI_KEY_LENGTH, *wbi::MIXIN_KEY_ENC_TAB.iter().max().unwrap() + 1);
}

#[test]
fn keys_unavailable_message() {
    let e = wbi::enc_wbi(&[], &"a".repeat(31), &"b".repeat(31), 0)
        .expect_err("short keys must fail");
    assert!(e.to_string().contains("signature key"), "got: {e}");
}

#[test]
fn wbi_keys_from_nav() {
    let data = serde_json::json!({
        "wbi_img": {
            "img_url": "https://i0.hdslb.com/bfs/wbi/7cd084941338484aae1ad9425b84077c.png",
            "sub_url": "https://i0.hdslb.com/bfs/wbi/4932caff0ff746eab6f01bf08b70ac45.png"
        }
    });
    let (img, sub) = wbi::wbi_keys_from_nav(&data).expect("keys present");
    assert_eq!(img, "7cd084941338484aae1ad9425b84077c");
    assert_eq!(sub, "4932caff0ff746eab6f01bf08b70ac45");
    assert!(wbi::wbi_keys_from_nav(&serde_json::json!({})).is_none());
}

#[test]
fn murmur3_buvid_fp_reference() {
    assert_eq!(
        murmur3::buvid_fp_hex(TESTUA, 31),
        "7fe1916b46b7235c95fda0c2387f3114"
    );
    assert_eq!(murmur3::buvid_fp_hex("abc", 31), "f04961ae49bab9266bb906b7e9ce9afb");
    assert_eq!(
        murmur3::buvid_fp_hex("0123456789ABCDEFG", 31),
        "db11b32c20e335317dbed7bc3b71dcc"
    );
    assert_eq!(murmur3::buvid_fp_hex(&"x".repeat(16), 31), "fc32dee9448418daceebbc07908613b4");
    assert_eq!(murmur3::buvid_fp_hex(&"y".repeat(33), 31), "f35db14c4b35b8c38f16293e0ea29463");
}

#[test]
fn gen_web_ticket_hexsign_reference() {
    assert_eq!(
        session::gen_web_ticket_hexsign(1_792_157_326),
        "b9e74108967effcebab92605ba985f4b726cdf26f225b63cec29df1b5e259335"
    );
    assert_eq!(
        session::gen_web_ticket_hexsign(1_700_000_000),
        "bb79f0d980ffbb51597aa1a3e8b55603025cc1322ac766f4c1a98852e6182514"
    );
}

#[test]
fn exclimbwuzhi_payload_structure() {
    let body = exclimbwuzhi::exclimbwuzhi_payload(TESTUA, "TESTUUID", 1_750_000_000_123);
    let outer: serde_json::Value = serde_json::from_str(&body).expect("outer JSON");
    let inner: serde_json::Value =
        serde_json::from_str(outer["payload"].as_str().expect("payload str")).expect("inner JSON");
    assert_eq!(inner["5062"], "1750000000123");
    assert_eq!(inner["3c43"]["b8ce"], TESTUA);
    assert_eq!(inner["df35"], "TESTUUID");
    assert_eq!(inner["5f45"], serde_json::Value::Null);
    assert_eq!(inner["3c43"]["a3c1"].as_array().map(Vec::len), Some(64));
    assert_eq!(inner["3c43"]["a658"].as_array().map(Vec::len), Some(48));
    assert_eq!(inner["3c43"]["80c9"].as_array().map(Vec::len), Some(5));
    assert_eq!(inner["3c43"].as_object().map(|m| m.len()), Some(30));
    // stdlib separators: ", " and ": " inside the payload string
    let payload_str = outer["payload"].as_str().unwrap();
    assert!(payload_str.contains("\": \""), "stdlib ': ' separator missing");
}

#[test]
fn auth_state_config_roundtrip() {
    // Key names must match config.json's Cookie section exactly.
    let state = AuthState {
        sessdata: "s".into(),
        dede_user_id: "42".into(),
        dede_user_id_ck_md5: "md5".into(),
        bili_jct: "jct".into(),
        is_login: true,
        ..Default::default()
    };
    let v = serde_json::to_value(&state).unwrap();
    assert_eq!(v["SESSDATA"], "s");
    assert_eq!(v["DedeUserID"], "42");
    assert_eq!(v["DedeUserID__ckMd5"], "md5");
    assert_eq!(v["is_login"], true);
    let back: AuthState = serde_json::from_value(v).unwrap();
    assert_eq!(back.sessdata, "s");
    assert_eq!(back.dede_user_id, "42");
    assert_eq!(back.dede_user_id_ck_md5, "md5");
}

#[test]
fn parse_cookies_matrix() {
    use login::parse_cookies;
    let header = parse_cookies("Cookie: SESSDATA=abc; bili_jct=xyz");
    assert_eq!(header[0], ("SESSDATA".into(), "abc".into()));
    let quoted = parse_cookies("SESSDATA=\"abc\"");
    assert_eq!(quoted[0].1, "abc");
    let json = parse_cookies("{\"SESSDATA\": \"abc\", \"bili_jct\": \"j\"}");
    assert_eq!(json.len(), 2);
    assert_eq!(json[0].1, "abc");
    let newline = parse_cookies("SESSDATA=abc\nbili_jct=j");
    assert_eq!(newline.len(), 2);
    assert!(parse_cookies("garbage").is_empty());
}

#[test]
fn timestamp_next_day_boundary() {
    assert_eq!(session::timestamp_next_day(86_400), 172_800);
    assert_eq!(session::timestamp_next_day(86_401), 172_800);
}

#[test]
fn qr_status_codes() {
    assert_eq!(login::QRCodeScanStatus::from_code(86101), login::QRCodeScanStatus::WaitingForScan);
    assert_eq!(login::QRCodeScanStatus::from_code(86090), login::QRCodeScanStatus::WaitingForConfirmation);
    assert_eq!(login::QRCodeScanStatus::from_code(0), login::QRCodeScanStatus::Success);
    assert_eq!(login::QRCodeScanStatus::from_code(86038), login::QRCodeScanStatus::Expired);
    assert_eq!(login::QRCodeScanStatus::from_code(9999), login::QRCodeScanStatus::Expired);
}
