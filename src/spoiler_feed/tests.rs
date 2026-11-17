use super::*;
use crate::blocker::json::base64_encode;
use ed25519_compact::{KeyPair, Seed};

/// Fixtures made by the Node tooling (`feed/tools/sign.mjs`, the committed TEST key): proves a feed signed
/// by node:crypto verifies here, i.e. both implementations agree on the wire format.
const SAMPLE_FEED: &str = include_str!("../../extensions/spoiler-firewall/feed/sample/feed.json");
const TEST_PUBLIC_KEY: &str = include_str!("../../extensions/spoiler-firewall/feed/test-keys/test-public-key.txt");
/// The Chrome host's constants, which must stay identical to ours.
const CHROME_CONFIG: &str = include_str!("../../extensions/spoiler-firewall/hosts/chrome/feed-config.js");

fn keypair(tag: u8) -> KeyPair {
    KeyPair::from_seed(Seed::new([tag; 32]))
}

fn raw_public(pair: &KeyPair) -> [u8; 32] {
    let mut key = [0u8; 32];
    key.copy_from_slice(pair.pk.as_ref());
    key
}

fn payload(seq: u32) -> String {
    format!(
        r#"{{"format":1,"seq":{seq},"issued":"2026-11-17","expires":"2099-01-01","lexicon":{{"add":{{"entityStrong":["harbor master sloane"]}}}}}}"#
    )
}

fn envelope_of(payload: &str, pair: &KeyPair) -> Vec<u8> {
    let sig = pair.sk.sign(payload.as_bytes(), None);
    format!(
        r#"{{"format":1,"payload":"{}","sig":"{}"}}"#,
        base64_encode(payload.as_bytes()),
        base64_encode(sig.as_ref())
    )
    .into_bytes()
}

fn good(seq: u32) -> (Vec<u8>, [u8; 32]) {
    let pair = keypair(1);
    (envelope_of(&payload(seq), &pair), raw_public(&pair))
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bowser-spoiler-feed-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ---- interop with the Node tooling ----------------------------------------------------------

#[test]
fn a_feed_signed_by_the_node_tooling_verifies_with_the_test_key_and_not_the_production_key() {
    let test_keys = parse_keys(TEST_PUBLIC_KEY);
    assert_eq!(test_keys.len(), 1);
    let verified = verify_envelope(SAMPLE_FEED.as_bytes(), &test_keys, 0).expect("node-signed sample verifies");
    assert_eq!(verified.feed.seq, 1);
    assert_eq!(verified.feed.issued, "2026-11-17");
    assert_eq!(verified.feed.terms, 4);
    assert_eq!(
        verified.feed.lexicon_json,
        r#"{"version":"f1","add":{"entityStrong":["sfw sample entity"],"cueStrong":["sfw sample cue"],"entityWeak":["sfw sample name"]},"raise":{"sfw sample raised":"cueStrong"}}"#
    );
    assert_eq!(verify_envelope(SAMPLE_FEED.as_bytes(), &production_keys(), 0), Err(Rejection::Signature));
}

#[test]
fn production_keys_match_the_chrome_host_and_the_test_key_is_never_one() {
    let keys = production_keys();
    assert!(!keys.is_empty(), "feed/public-key.txt holds at least one key");
    for key in &keys {
        assert!(CHROME_CONFIG.contains(&base64_encode(key)), "Chrome feed-config.js lacks a production key");
        assert!(!parse_keys(TEST_PUBLIC_KEY).contains(key), "the throwaway test key must never be a production key");
    }
    assert_eq!(CHROME_CONFIG.matches("PUBLIC_KEYS:").count(), 1);
    let listed = CHROME_CONFIG.split("PUBLIC_KEYS: [").nth(1).unwrap().split(']').next().unwrap();
    assert_eq!(listed.matches('"').count() / 2, keys.len(), "same number of keys on both sides");
}

#[test]
fn the_url_and_the_builtin_version_agree_with_the_chrome_host_and_the_bundle() {
    assert!(CHROME_CONFIG.contains(&format!("FEED_URL: \"{FEED_URL}\"")), "one URL for both hosts");
    assert!(FEED_URL.starts_with("https://") && !FEED_URL.contains('?'));
    let version = builtin_lexicon_version(crate::spoiler::BUNDLE);
    assert_ne!(version, "built-in", "the bundle names its lexicon version");
    assert!(CHROME_CONFIG.contains(&format!("BUILTIN_VERSION: \"{version}\"")), "feed-config.js BUILTIN_VERSION must be {version}");
}

// ---- verification ---------------------------------------------------------------------------

#[test]
fn a_correctly_signed_feed_verifies_and_the_lexicon_is_re_serialised_compactly() {
    let (body, key) = good(5);
    let v = verify_envelope(&body, &[key], 0).unwrap();
    assert_eq!((v.feed.seq, v.current, v.feed.terms), (5, false, 1));
    assert_eq!(v.feed.expires, "2099-01-01");
    assert_eq!(v.feed.lexicon_json, r#"{"add":{"entityStrong":["harbor master sloane"]}}"#);
}

#[test]
fn any_change_to_the_payload_the_signature_or_the_key_is_refused() {
    let (body, key) = good(5);
    let text = String::from_utf8(body.clone()).unwrap();
    let env = json::parse(&text).unwrap();
    let object = env.as_object().unwrap();
    let payload_b64 = object.get("payload").unwrap().as_str().unwrap().to_owned();
    let sig_b64 = object.get("sig").unwrap().as_str().unwrap().to_owned();
    let raw = crate::blocker::json::base64_decode(&payload_b64).unwrap();
    for i in (0..raw.len()).step_by(7) {
        let mut tampered = raw.clone();
        tampered[i] ^= 1;
        let altered = text.replace(&payload_b64, &base64_encode(&tampered));
        let result = verify_envelope(altered.as_bytes(), &[key], 0);
        assert!(matches!(result, Err(Rejection::Signature | Rejection::Envelope)), "byte {i}: {result:?}");
    }
    let mut sig = crate::blocker::json::base64_decode(&sig_b64).unwrap();
    sig[3] ^= 0x10;
    let bad_sig = text.replace(&sig_b64, &base64_encode(&sig));
    assert_eq!(verify_envelope(bad_sig.as_bytes(), &[key], 0), Err(Rejection::Signature));
    assert_eq!(verify_envelope(&body, &[raw_public(&keypair(2))], 0), Err(Rejection::Signature), "wrong key");
    assert_eq!(verify_envelope(&body, &[], 0), Err(Rejection::NoKey));
    // either listed key may have signed (key rotation)
    assert!(verify_envelope(&body, &[raw_public(&keypair(2)), key], 0).is_ok());
}

#[test]
fn unsigned_misshapen_and_garbage_input_is_refused_without_panicking() {
    let (body, key) = good(5);
    let env = String::from_utf8(body).unwrap();
    let payload_b64 = env.split("\"payload\":\"").nth(1).unwrap().split('"').next().unwrap().to_owned();
    let sig_b64 = env.split("\"sig\":\"").nth(1).unwrap().split('"').next().unwrap().to_owned();
    let cases: Vec<(String, Rejection)> = vec![
        (String::new(), Rejection::Envelope),
        ("   ".into(), Rejection::Envelope),
        ("null".into(), Rejection::Envelope),
        ("[]".into(), Rejection::Envelope),
        ("{}".into(), Rejection::Envelope),
        ("<html><body>404: Not Found</body></html>".into(), Rejection::Envelope),
        ("{".repeat(10_000), Rejection::Envelope),
        (format!(r#"{{"format":1,"payload":"{payload_b64}"}}"#), Rejection::Envelope), // unsigned
        (format!(r#"{{"format":1,"payload":"{payload_b64}","sig":""}}"#), Rejection::Envelope),
        (format!(r#"{{"format":2,"payload":"{payload_b64}","sig":"{sig_b64}"}}"#), Rejection::Format),
        (format!(r#"{{"payload":"{payload_b64}","sig":"{sig_b64}"}}"#), Rejection::Format),
        (format!(r#"{{"format":1,"payload":"{payload_b64}","sig":"{}"}}"#, &sig_b64[..sig_b64.len() - 4]), Rejection::Envelope),
        (format!(r#"{{"format":1,"payload":" {payload_b64}","sig":"{sig_b64}"}}"#), Rejection::Envelope),
        (format!(r#"{{"format":1,"payload":"{payload_b64}\n","sig":"{sig_b64}"}}"#), Rejection::Envelope),
        (format!(r#"{{"format":1,"payload":5,"sig":"{sig_b64}"}}"#), Rejection::Envelope),
        (payload(5), Rejection::Envelope), // the bare payload, no envelope
        (format!("{env} trailing"), Rejection::Envelope),
    ];
    for (body, want) in cases {
        assert_eq!(verify_envelope(body.as_bytes(), &[key], 0), Err(want), "{:?}", &body[..body.len().min(60)]);
    }
    assert_eq!(verify_envelope(&[0xff, 0xfe, 0x00], &[key], 0), Err(Rejection::Envelope), "not UTF-8");
}

#[test]
fn a_signed_payload_that_is_not_a_valid_feed_is_refused_after_the_signature_check() {
    let pair = keypair(1);
    let key = raw_public(&pair);
    let with = |over: &str| {
        // `over` replaces members of an otherwise valid payload; later duplicate keys win, as in JSON.parse
        let base = r#""format":1,"seq":5,"issued":"2026-11-17","expires":"2026-12-17","lexicon":{"add":{"cueStrong":["a term"]}}"#;
        format!("{{{base},{over}}}")
    };
    let cases: Vec<(String, Rejection)> = vec![
        (with(r#""format":2"#), Rejection::Format),
        (with(r#""format":"1""#), Rejection::Format),
        (with(r#""seq":0"#), Rejection::Seq),
        (with(r#""seq":-3"#), Rejection::Seq),
        (with(r#""seq":1.5"#), Rejection::Seq),
        (with(r#""seq":"7""#), Rejection::Seq),
        (with(r#""seq":2147483648"#), Rejection::Seq),
        (with(r#""seq":null"#), Rejection::Seq),
        (with(r#""seq":1e999"#), Rejection::Seq),
        (with(r#""issued":"yesterday""#), Rejection::Dates),
        (with(r#""expires":12345"#), Rejection::Dates),
        (with(r#""issued":"2026-12-30","expires":"2026-12-01""#), Rejection::Dates),
        (with(r#""issued":"2026-02-30""#), Rejection::Dates),
        (with(r#""issued":"2026-02-30T99:00:00Z""#), Rejection::Dates),
        (with(r#""expires":"2026-12-17T10:00""#), Rejection::Dates), // a time needs an offset
        (with(r#""lexicon":null"#), Rejection::Lexicon),
        (with(r#""lexicon":[]"#), Rejection::Lexicon),
        (with(r#""lexicon":"x""#), Rejection::Lexicon),
        (with(r#""lexicon":{"remove":["spoiler"]}"#), Rejection::Lexicon),
        (with(r#""lexicon":{"negative":["gta v"]}"#), Rejection::Lexicon),
        (with(r#""lexicon":{"add":{"safe":["release date"]}}"#), Rejection::Lexicon),
        (with(r#""lexicon":{"add":{"negative":["x y"]}}"#), Rejection::Lexicon),
        (with(r#""lexicon":{"add":{"cueStrong":"oops"}}"#), Rejection::Lexicon),
        (with(r#""lexicon":{"add":{"cueStrong":[5]}}"#), Rejection::Lexicon),
        (with(r#""lexicon":{"raise":{"some term":"negative"}}"#), Rejection::Lexicon),
        (with(r#""lexicon":{"raise":{"some term":3}}"#), Rejection::Lexicon),
        (with(r#""lexicon":{"version":"has space"}"#), Rejection::Lexicon),
        (with(&format!(r#""lexicon":{{"version":"{}"}}"#, "x".repeat(41))), Rejection::Lexicon),
    ];
    for (text, want) in cases {
        assert_eq!(verify_envelope(&envelope_of(&text, &pair), &[key], 0), Err(want), "{text}");
    }
    assert_eq!(verify_envelope(&envelope_of("not json at all", &pair), &[key], 0), Err(Rejection::Payload));
    assert_eq!(verify_envelope(&envelope_of("[1,2,3]", &pair), &[key], 0), Err(Rejection::Payload));
    // a lone surrogate escape is not representable: refused, not panicked on
    assert_eq!(verify_envelope(&envelope_of(&with(r#""lexicon":{"add":{"cueStrong":["\ud800"]}}"#), &pair), &[key], 0), Err(Rejection::Payload));
    let valid_dates = [r#""issued":"2026-11-17T12:30Z""#, r#""expires":"2026-12-17T12:30:45.123+02:00""#, r#""issued":"2024-02-29""#];
    for over in valid_dates {
        assert!(verify_envelope(&envelope_of(&with(over), &pair), &[key], 0).is_ok(), "{over}");
    }
}

#[test]
fn more_terms_than_the_engine_accepts_is_refused() {
    let pair = keypair(1);
    let terms = |n: usize| (0..n).map(|i| format!("\"zz term {i}\"")).collect::<Vec<_>>().join(",");
    let feed = |n: usize| format!(r#"{{"format":1,"seq":2,"issued":"2026-11-17","expires":"2099-01-01","lexicon":{{"add":{{"cueWeak":[{}]}}}}}}"#, terms(n));
    let key = raw_public(&pair);
    assert!(verify_envelope(&envelope_of(&feed(5000), &pair), &[key], 0).is_ok());
    assert_eq!(verify_envelope(&envelope_of(&feed(5001), &pair), &[key], 0), Err(Rejection::Lexicon));
    // adds and raises count together
    let mixed = format!(
        r#"{{"format":1,"seq":2,"issued":"2026-11-17","expires":"2099-01-01","lexicon":{{"add":{{"cueWeak":[{}]}},"raise":{{{}}}}}}}"#,
        terms(3000),
        (0..2001).map(|i| format!("\"zz raised {i}\":\"cueStrong\"")).collect::<Vec<_>>().join(",")
    );
    assert_eq!(verify_envelope(&envelope_of(&mixed, &pair), &[key], 0), Err(Rejection::Lexicon));
}

#[test]
fn size_caps_apply_before_anything_is_parsed() {
    let (_, key) = good(1);
    assert_eq!(MAX_BODY_BYTES, 256 * 1024);
    assert_eq!(verify_envelope(&vec![b' '; MAX_BODY_BYTES + 1], &[key], 0), Err(Rejection::Size));
    assert_eq!(verify_envelope(&vec![b'x'; 300_000], &[key], 0), Err(Rejection::Size));
    // exactly at the cap is parsed (and fails as an envelope, not as a size)
    assert_eq!(verify_envelope(&vec![b' '; MAX_BODY_BYTES], &[key], 0), Err(Rejection::Envelope));
    // a large valid feed (4900 terms, about 150 KB) is fine
    let pair = keypair(1);
    let terms = (0..4900).map(|i| format!("\"zz term number {i} filler\"")).collect::<Vec<_>>().join(",");
    let big = format!(r#"{{"format":1,"seq":2,"issued":"2026-11-17","expires":"2099-01-01","lexicon":{{"add":{{"cueWeak":[{terms}]}}}}}}"#);
    let body = envelope_of(&big, &pair);
    assert!(body.len() > 150_000 && body.len() < MAX_BODY_BYTES, "{}", body.len());
    assert_eq!(verify_envelope(&body, &[raw_public(&pair)], 0).unwrap().feed.terms, 4900);
    // a payload that is only just under the envelope cap once wrapped in base64 is refused as oversize
    let huge = format!(r#"{{"format":1,"seq":2,"issued":"2026-11-17","expires":"2099-01-01","lexicon":{{"add":{{"cueWeak":["{}"]}}}}}}"#, "y".repeat(200_000));
    assert_eq!(verify_envelope(&envelope_of(&huge, &pair), &[raw_public(&pair)], 0), Err(Rejection::Size));
}

#[test]
fn rollback_is_refused_the_same_seq_is_a_no_op_and_a_higher_seq_is_accepted() {
    let (body4, key) = good(4);
    let (body5, _) = good(5);
    let (body6, _) = good(6);
    assert_eq!(verify_envelope(&body4, &[key], 5), Err(Rejection::Rollback));
    let same = verify_envelope(&body5, &[key], 5).unwrap();
    assert!(same.current);
    let up = verify_envelope(&body6, &[key], 5).unwrap();
    assert!(!up.current);
    assert_eq!(up.feed.seq, 6);
    assert!(verify_envelope(&body4, &[key], 0).is_ok(), "nothing stored yet");
}

#[test]
fn an_expired_feed_still_verifies_but_the_summary_says_it_is_out_of_date() {
    let pair = keypair(1);
    let key = raw_public(&pair);
    let text = r#"{"format":1,"seq":5,"issued":"2026-01-01","expires":"2026-01-31","lexicon":{"add":{"cueStrong":["a term"]}}}"#;
    let feed = verify_envelope(&envelope_of(text, &pair), &[key], 0).unwrap().feed;
    let day = parse_date("2026-11-17T12:00:00Z", false).unwrap();
    assert!(feed.is_expired(day));
    assert!(!feed.is_expired(parse_date("2026-01-31T23:59:59Z", false).unwrap()), "a bare expiry date lasts through that UTC day");
    assert!(feed.is_expired(parse_date("2026-02-01T00:00:01Z", false).unwrap()));
    let dir = temp_dir("expired");
    let mut manager = Manager::load(&dir, vec![key]);
    assert!(manager.handle(Ok(envelope_of(text, &pair))), "the expired feed is applied");
    assert_eq!(
        manager.summary("2026.11.17-1", true, day),
        "Term list: built-in 2026.11.17-1 + update #5 (2026-01-01). Term updates out of date."
    );
    std::fs::remove_dir_all(dir).ok();
}

// ---- dates --------------------------------------------------------------------------------------

#[test]
fn dates_parse_to_the_same_milliseconds_as_javascript() {
    // values from Date.parse in Node
    for (text, ms) in [
        ("2026-11-17", 1794873600000i64),
        ("2026-12-17", 1797465600000),
        ("1970-01-01", 0),
        ("2000-02-29", 951782400000),
        ("2026-11-17T12:30Z", 1794918600000),
        ("2026-11-17T12:30:45Z", 1794918645000),
        ("2026-11-17T12:30:45.5Z", 1794918645500),
        ("2026-11-17T12:30:45.123+02:00", 1794911445123),
        ("2026-11-17T23:59-05:30", 1794979740000),
    ] {
        assert_eq!(parse_date(text, false), Some(ms), "{text}");
    }
    assert_eq!(parse_date("2026-11-17", true), Some(1794873600000 + 86_399_999));
    assert_eq!(parse_date("2026-11-17T12:30Z", true), Some(1794918600000), "a timestamp is exact");
    for bad in [
        "", "2026", "2026-1-17", "2026-13-01", "2026-00-10", "2026-04-31", "2025-02-29", "1900-02-29", "2026-11-17T", "2026-11-17T24:00Z",
        "2026-11-17T12:60Z", "2026-11-17T12:30:60Z", "2026-11-17T12:30", "2026-11-17 12:30Z", "2026-11-17T12:30:45.Z", "2026-11-17T12:30:45.1234Z",
        "2026-11-17T12:30+0200", "2026-11-17T12:30+24:00", "20261117", "2026-11-17Z", "2026-11-17T12:30Z ", "\u{e9}026-11-17", "2026-11-17T12:30\u{e9}",
    ] {
        assert_eq!(parse_date(bad, false), None, "{bad:?}");
    }
}

// ---- the JSON reader ----------------------------------------------------------------------------

#[test]
fn the_json_reader_is_strict() {
    use json::parse;
    assert!(parse(" {\"a\" : [1, 2.5e1, -0, true, false, null, \"x\"] } ").is_some());
    let escapes = parse(r#""a\"b\\c\/d\b\f\n\r\t\u00e9\ud83d\ude00""#).unwrap();
    assert_eq!(escapes.as_str(), Some("a\"b\\c/d\u{8}\u{c}\n\r\t\u{e9}\u{1F600}"));
    for bad in [
        "", "{", "}", "[1,]", "{\"a\":1,}", "{\"a\" 1}", "{a:1}", "{'a':1}", "[01]", "[1.]", "[.5]", "[+1]", "[1e]", "nul", "tru", "NaN", "Infinity",
        "\"unterminated", "\"bad\\q\"", "\"\\u12\"", "\"\\ud800\"", "\"\\udc00\"", "\"\\ud800\\u0041\"", "\"tab\there\"", "{\"a\":1} x", "[1] [2]",
    ] {
        assert!(parse(bad).is_none(), "{bad:?}");
    }
    let deep = format!("{}1{}", "[".repeat(40), "]".repeat(40));
    assert!(parse(&deep).is_none(), "deeply nested input is refused");
    let last_wins = parse(r#"{"a":1,"a":2}"#).unwrap();
    assert_eq!(last_wins.as_object().unwrap().get("a").and_then(Json::as_f64), Some(2.0));
    assert_eq!(last_wins.as_object().unwrap().keys().count(), 1);
}

#[test]
fn re_serialised_terms_are_script_safe() {
    let pair = keypair(1);
    let text = "{\"format\":1,\"seq\":2,\"issued\":\"2026-11-17\",\"expires\":\"2099-01-01\",\"lexicon\":{\"add\":{\"cueStrong\":[\"quote\\\" back\\\\slash\\u2028sep\\u2029\\u0001 caf\\u00e9 </script>\"]}}}";
    let v = verify_envelope(&envelope_of(text, &pair), &[raw_public(&pair)], 0).unwrap();
    let out = &v.feed.lexicon_json;
    assert!(!out.contains('\u{2028}') && !out.contains('\u{2029}'));
    assert!(out.contains("\\u2028") && out.contains("\\u2029") && out.contains("\\u0001"));
    assert!(out.contains("caf\u{e9}"));
    // and the JSON reader agrees it is the same string
    let back = json::parse(out).unwrap();
    let term = back.as_object().unwrap().get("add").unwrap().as_object().unwrap().get("cueStrong").unwrap().as_array().unwrap()[0].as_str().unwrap().to_owned();
    assert_eq!(term, "quote\" back\\slash\u{2028}sep\u{2029}\u{1} caf\u{e9} </script>");
    assert!(crate::spoiler::config_json_with_feed(&crate::spoiler::SpoilerSettings::default(), "1.0.0", false, None, Some(out)).contains("\"feed\":{"));
}

// ---- the stored copy and the status ---------------------------------------------------------------

#[test]
fn a_verified_download_is_stored_applied_and_reloaded_after_a_restart() {
    let dir = temp_dir("persist");
    let (body, key) = good(3);
    let mut manager = Manager::load(&dir, vec![key]);
    assert!(manager.feed().is_none());
    assert!(manager.handle(Ok(body.clone())));
    assert_eq!(manager.status, Status::Ok { current: false });
    assert_eq!(manager.highest_seq(), 3);
    assert_eq!(std::fs::read(dir.join(FILE_NAME)).unwrap(), body, "the exact envelope is kept (re-verified on every load)");
    let reloaded = Manager::load(&dir, vec![key]);
    assert_eq!(reloaded.feed().map(|f| f.seq), Some(3));
    assert_eq!(reloaded.feed().unwrap().lexicon_json, manager.feed().unwrap().lexicon_json);
    // the same seq again changes nothing
    assert!(!manager.handle(Ok(body)));
    assert_eq!(manager.status, Status::Ok { current: true });
    // a lower seq is a rollback: refused, stored feed untouched, restart still on seq 3
    let (older, _) = good(2);
    assert!(!manager.handle(Ok(older)));
    assert_eq!(manager.status, Status::Rejected(Rejection::Rollback));
    assert_eq!(manager.highest_seq(), 3);
    assert_eq!(Manager::load(&dir, vec![key]).highest_seq(), 3);
    // a higher seq replaces it, on disk too
    let (newer, _) = good(4);
    assert!(manager.handle(Ok(newer)));
    assert_eq!(Manager::load(&dir, vec![key]).highest_seq(), 4);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn rejected_and_unreachable_downloads_change_nothing_and_say_so() {
    let dir = temp_dir("status");
    let (body, key) = good(3);
    let mut manager = Manager::load(&dir, vec![key]);
    assert!(manager.handle(Ok(body.clone())));
    let before = manager.feed().cloned();
    let stranger = envelope_of(&payload(9), &keypair(9));
    for (result, status) in [
        (Ok(stranger), Status::Rejected(Rejection::Signature)),
        (Ok(b"<html>nope</html>".to_vec()), Status::Rejected(Rejection::Envelope)),
        (Ok(vec![b'x'; MAX_BODY_BYTES + 1]), Status::Rejected(Rejection::Size)),
        (Err(FetchError::TooLarge), Status::Rejected(Rejection::Size)),
        (Err(FetchError::NotFound), Status::NotPublished),
        (Err(FetchError::Unavailable), Status::Unavailable),
    ] {
        assert!(!manager.handle(result.clone()), "{result:?}");
        assert_eq!(manager.status, status);
        assert_eq!(manager.feed().cloned(), before, "the applied feed must not change");
        assert_eq!(std::fs::read(dir.join(FILE_NAME)).unwrap(), body, "the stored envelope must not change");
    }
    manager.status = Status::Ok { current: true };
    assert_eq!(manager.summary("2026.11.17-1", true, 0), "Term list: built-in 2026.11.17-1 + update #3 (2026-11-17)");
    manager.status = Status::Rejected(Rejection::Signature);
    assert_eq!(manager.summary("B", true, 0), "Term list: built-in B + update #3 (2026-11-17). Term update rejected.");
    manager.status = Status::Unavailable;
    assert_eq!(manager.summary("B", true, 0), "Term list: built-in B + update #3 (2026-11-17). Could not check for term updates.");
    assert_eq!(manager.summary("B", false, 0), "Term list: built-in B + update #3 (2026-11-17). Term updates are off.");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_missing_feed_is_quiet() {
    let dir = temp_dir("quiet");
    let mut manager = Manager::load(&dir, vec![raw_public(&keypair(1))]);
    assert!(!manager.handle(Err(FetchError::NotFound)));
    assert_eq!(manager.summary("2026.11.17-1", true, 0), "Term list: built-in 2026.11.17-1. No term update has been published yet.");
    assert_eq!(manager.summary("2026.11.17-1", false, 0), "Term list: built-in 2026.11.17-1. Term updates are off.");
    manager.status = Status::Idle;
    assert_eq!(manager.summary("2026.11.17-1", true, 0), "Term list: built-in 2026.11.17-1");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_tampered_unsigned_or_oversize_stored_file_is_ignored_on_load() {
    let dir = temp_dir("tamper");
    let (body, key) = good(3);
    let path = dir.join(FILE_NAME);
    for stored in [
        b"garbage".to_vec(),
        Vec::new(),
        envelope_of(&payload(3), &keypair(8)), // someone else's key
        body.iter().map(|b| if *b == b'1' { b'2' } else { *b }).collect(), // altered text
        vec![b' '; MAX_BODY_BYTES + 10],
    ] {
        std::fs::write(&path, &stored).unwrap();
        let manager = Manager::load(&dir, vec![key]);
        assert!(manager.feed().is_none());
        assert_eq!(manager.highest_seq(), 0);
    }
    // and with no file at all
    std::fs::remove_file(&path).unwrap();
    assert!(Manager::load(&dir, vec![key]).feed().is_none());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn an_unwritable_store_still_applies_the_feed_for_this_run() {
    // the directory does not exist: the write fails, the verified feed is still held in memory
    let missing = std::env::temp_dir().join(format!("bowser-spoiler-feed-missing-{}", std::process::id())).join("nope");
    let (body, key) = good(7);
    let mut manager = Manager::load(&missing, vec![key]);
    assert!(manager.handle(Ok(body)));
    assert_eq!(manager.highest_seq(), 7);
}

#[test]
fn the_builtin_version_is_read_from_the_bundle_source() {
    assert_eq!(builtin_lexicon_version("SFW.lexicon = (function () {\n  const VERSION = \"2026.11.17-1\";\n"), "2026.11.17-1");
    assert_eq!(builtin_lexicon_version("nothing here"), "built-in");
    assert_eq!(builtin_lexicon_version("SFW.lexicon = (function () {\n  const VERSION = \"x y\";\n"), "built-in");
}

#[test]
fn the_check_interval_and_url_are_the_documented_ones() {
    assert_eq!(CHECK_INTERVAL, Duration::from_secs(43_200));
    assert!(crate::updater::validate_https_url(FEED_URL).is_ok(), "the feed URL passes the updater's HTTPS-only rule");
}
