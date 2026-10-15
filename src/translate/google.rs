//! Translation by Google Translate's public web endpoint (`translate.googleapis.com`, `client=gtx`):
//! no key, no account, no download. A batch of strings goes out as one form POST and comes back as a
//! JSON array in the same order. This module is the pure half (the request and the strict reading of
//! the answer); [`super::net`] does the HTTPS.
//!
//! The endpoint is Google's own web client's and is not a documented API, so it can change or start
//! refusing; every way it can fail ends in a [`GoogleError`] that the app turns into a plain message.

use super::{MAX_BATCH_BYTES, json, net};

/// The host that receives page text. The only address translation ever contacts.
pub const HOST: &str = "translate.googleapis.com";
/// The longest answer read (translations can be longer than their source).
const MAX_ANSWER: usize = 1024 * 1024;
/// The longest single translated string accepted (UTF-8 bytes).
const MAX_OUT_ITEM_BYTES: usize = 64 * 1024;

/// Why a batch could not be translated. Fixed words for the user's message; never page text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoogleError {
    /// Google could not be reached, or did not answer in time.
    Network,
    /// Google is refusing requests from this connection for now (HTTP 429 or a captcha page).
    Limited,
    /// The answer, or the request, was not what was expected.
    Bad,
}

/// The source language of a request: a plain language code, or `auto` to let Google detect it.
fn valid_language(code: &str) -> bool {
    code == "auto" || ((2..=3).contains(&code.len()) && code.bytes().all(|byte| byte.is_ascii_lowercase()))
}

/// The request line's path and query for translating `from` (a code or `auto`) into `to`.
pub fn request_path(from: &str, to: &str) -> Option<String> {
    (valid_language(from) && valid_language(to) && to != "auto")
        .then(|| format!("/translate_a/t?client=gtx&format=text&sl={from}&tl={to}"))
}

/// Percent-encodes everything but the unreserved characters, so any text is a safe form value.
fn form_encode(text: &str, out: &mut String) {
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
}

/// The form body: one `q=` per string, in order.
pub fn request_body(strings: &[String]) -> String {
    let mut body = String::with_capacity(strings.iter().map(|text| text.len() * 2 + 3).sum());
    for (index, text) in strings.iter().enumerate() {
        if index > 0 {
            body.push('&');
        }
        body.push_str("q=");
        form_encode(text, &mut body);
    }
    body
}

/// Reads Google's answer to a batch of `expected` strings. With a named source language the answer is
/// an array of strings; with `auto` it is an array of `[text, detected language]` pairs. Anything else
/// (a captcha page, a different count, a non-string) is refused.
pub fn parse_answer(raw: &[u8], expected: usize) -> Result<Vec<String>, GoogleError> {
    let text = std::str::from_utf8(raw).map_err(|_| GoogleError::Bad)?;
    let value = json::parse(text).ok_or(GoogleError::Bad)?;
    let items = value.as_array().ok_or(GoogleError::Bad)?;
    if items.len() != expected {
        return Err(GoogleError::Bad);
    }
    items
        .iter()
        .map(|item| {
            let translated = match item.as_str() {
                Some(text) => text,
                None => item.as_array().and_then(|pair| pair.first()).and_then(|first| first.as_str()).ok_or(GoogleError::Bad)?,
            };
            (translated.len() <= MAX_OUT_ITEM_BYTES).then(|| translated.to_owned()).ok_or(GoogleError::Bad)
        })
        .collect()
}

/// One POST: the path with its body in, the answer's bytes out. The real one is [`net::post_form`];
/// tests pass a fake.
pub type Post<'a> = dyn FnMut(&str, &[u8]) -> Result<Vec<u8>, net::NetError> + 'a;

fn from_net(error: net::NetError) -> GoogleError {
    match error {
        net::NetError::Status(429) => GoogleError::Limited,
        net::NetError::Status(400) | net::NetError::TooLarge => GoogleError::Bad,
        net::NetError::Status(_) | net::NetError::Network => GoogleError::Network,
    }
}

/// Translates `strings` from `from` into `to` through `post`. A network failure is tried once more,
/// since a first request on a cold connection is the one that most often fails.
pub fn translate_with(from: &str, to: &str, strings: &[String], post: &mut Post<'_>) -> Result<Vec<String>, GoogleError> {
    if strings.is_empty() {
        return Ok(Vec::new());
    }
    let path = request_path(from, to).ok_or(GoogleError::Bad)?;
    let body = request_body(strings);
    if body.len() > MAX_BATCH_BYTES * 3 + strings.len() * 3 {
        return Err(GoogleError::Bad);
    }
    let mut attempt = 0;
    loop {
        match post(&path, body.as_bytes()).map_err(from_net) {
            Ok(answer) => return parse_answer(&answer, strings.len()),
            Err(GoogleError::Network) if attempt == 0 => attempt += 1,
            Err(error) => return Err(error),
        }
    }
}

/// [`translate_with`] over the real connection. Blocks for the whole request: for a background thread.
pub fn translate(from: &str, to: &str, strings: &[String]) -> Result<Vec<String>, GoogleError> {
    translate_with(from, to, strings, &mut |path, body| net::post_form(HOST, path, body, MAX_ANSWER))
}

#[cfg(test)]
mod tests {
    use super::*;
    use net::NetError;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|text| (*text).to_owned()).collect()
    }

    #[test]
    fn the_request_names_only_language_codes() {
        assert_eq!(request_path("es", "en").as_deref(), Some("/translate_a/t?client=gtx&format=text&sl=es&tl=en"));
        assert_eq!(request_path("auto", "de").as_deref(), Some("/translate_a/t?client=gtx&format=text&sl=auto&tl=de"));
        for (from, to) in [("auto", "auto"), ("", "en"), ("en", ""), ("e", "en"), ("EN", "en"), ("en&x=1", "fr"), ("fr", "en/../x"), ("fr", "é")] {
            assert_eq!(request_path(from, to), None, "{from:?} {to:?}");
        }
    }

    #[test]
    fn every_string_is_a_percent_encoded_form_value() {
        assert_eq!(request_body(&strings(&["Hola mundo", "a&b=c", "ñ\n"])), "q=Hola%20mundo&q=a%26b%3Dc&q=%C3%B1%0A");
        assert_eq!(request_body(&[]), "");
        let body = request_body(&strings(&["</script> \"x\" +%"]));
        assert!(body.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_.~%=&".contains(&byte)), "{body}");
    }

    #[test]
    fn the_answer_is_strings_or_text_and_language_pairs_in_order() {
        assert_eq!(parse_answer(br#"["Hello world","Goodbye \"friend\" <b>x<\/b>"]"#, 2).unwrap(), strings(&["Hello world", "Goodbye \"friend\" <b>x</b>"]));
        assert_eq!(parse_answer(br#"[["Zeile eins\nZeile zwei","en"],["","en"]]"#, 2).unwrap(), strings(&["Zeile eins\nZeile zwei", ""]));
        assert_eq!(parse_answer(b"[]", 0).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn an_answer_that_is_not_one_string_per_question_is_refused() {
        for bad in [
            &b""[..],
            b"<html>captcha</html>",
            b"null",
            b"{}",
            b"\"text\"",
            br#"["a"]"#,
            br#"["a","b","c"]"#,
            br#"["a",1]"#,
            br#"["a",null]"#,
            br#"[["a"],[1]]"#,
            br#"[[],"b"]"#,
            br#"["a","b"] x"#,
            &[b'[', b'"', 0xff, b'"', b',', b'"', b'b', b'"', b']'],
        ] {
            assert_eq!(parse_answer(bad, 2), Err(GoogleError::Bad), "{:?}", String::from_utf8_lossy(bad));
        }
        let huge = format!("[\"{}\"]", "x".repeat(MAX_OUT_ITEM_BYTES + 1));
        assert_eq!(parse_answer(huge.as_bytes(), 1), Err(GoogleError::Bad), "one string over the limit");
    }

    #[test]
    fn a_batch_goes_out_as_one_post_and_comes_back_in_order() {
        let mut seen = Vec::new();
        let result = translate_with("es", "en", &strings(&["Hola", "Adiós"]), &mut |path, body| {
            seen.push((path.to_owned(), String::from_utf8(body.to_vec()).unwrap()));
            Ok(br#"["Hello","Goodbye"]"#.to_vec())
        });
        assert_eq!(result.unwrap(), strings(&["Hello", "Goodbye"]));
        assert_eq!(seen, [("/translate_a/t?client=gtx&format=text&sl=es&tl=en".to_owned(), "q=Hola&q=Adi%C3%B3s".to_owned())]);
        // Nothing to translate asks nothing.
        let mut asked = false;
        assert_eq!(translate_with("es", "en", &[], &mut |_, _| { asked = true; Ok(Vec::new()) }).unwrap(), Vec::<String>::new());
        assert!(!asked);
        // A language that is not a code never reaches the network.
        assert_eq!(translate_with("es", "en&x", &strings(&["a"]), &mut |_, _| { asked = true; Ok(Vec::new()) }), Err(GoogleError::Bad));
        assert!(!asked);
    }

    #[test]
    fn failures_map_to_fixed_kinds_and_only_network_trouble_is_retried() {
        let one = strings(&["a"]);
        let attempts = |error: NetError| {
            let mut count = 0;
            let result = translate_with("fr", "en", &one, &mut |_, _| {
                count += 1;
                Err(error)
            });
            (result, count)
        };
        assert_eq!(attempts(NetError::Network), (Err(GoogleError::Network), 2), "one retry");
        assert_eq!(attempts(NetError::Status(503)), (Err(GoogleError::Network), 2));
        assert_eq!(attempts(NetError::Status(429)), (Err(GoogleError::Limited), 1), "rate limiting is not hammered");
        assert_eq!(attempts(NetError::Status(400)), (Err(GoogleError::Bad), 1));
        assert_eq!(attempts(NetError::TooLarge), (Err(GoogleError::Bad), 1));
        // The retry can succeed.
        let mut count = 0;
        let result = translate_with("fr", "en", &one, &mut |_, _| {
            count += 1;
            if count == 1 { Err(NetError::Network) } else { Ok(br#"["b"]"#.to_vec()) }
        });
        assert_eq!(result.unwrap(), strings(&["b"]));
        // A refused answer is final too.
        assert_eq!(translate_with("fr", "en", &one, &mut |_, _| Ok(b"<html>".to_vec())), Err(GoogleError::Bad));
    }

    /// The real connection, on request only: `cargo test --lib -- --ignored google::tests::live`.
    #[test]
    #[ignore = "needs the network"]
    fn live_google_translates_a_batch_with_a_named_and_a_detected_source() {
        let batch = strings(&["Los profesores en huelga pintan de verde el centro de Madrid", "Hola"]);
        let named = translate("es", "en", &batch).expect("named source");
        assert_eq!(named.len(), 2);
        assert!(named[0].to_lowercase().contains("madrid") && named[0] != batch[0], "{named:?}");
        let detected = translate("auto", "en", &batch).expect("detected source");
        assert_eq!(detected.len(), 2);
        assert!(detected[1].to_lowercase().contains("hello") || detected[1].to_lowercase().contains("hi"), "{detected:?}");
    }
}
