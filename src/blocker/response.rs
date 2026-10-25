//! Pure response-stage CDP parsing, guards and fulfillment serialization.

/// Largest response body the player-ads rewrite will hold; anything bigger passes through.
const MAX_REWRITE_BODY_BYTES: usize = 16 * 1024 * 1024;

/// A `Fetch.requestPaused` event at the Response stage: the request's URL, its HTTP status
/// (`None` when the response failed) and the response headers.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct PausedResponse {
    pub url: String,
    pub status: Option<u16>,
    pub resource_type: Option<String>,
    pub headers: Vec<(String, String)>,
}

/// Which stage a `Fetch.requestPaused` event paused at. A Response-stage event that cannot be
/// read is still a response, so it is continued rather than failed as a blocked request.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum PausedStage {
    Request,
    Response(Option<PausedResponse>),
}

pub(crate) fn paused_stage(event: &str) -> PausedStage {
    use super::json;
    let Some(members) = json::whole_object(event) else {
        return if event.contains("\"responseStatusCode\"")
            || event.contains("\"responseErrorReason\"")
        {
            PausedStage::Response(None)
        } else {
            PausedStage::Request
        };
    };
    let status = json::field(&members, "responseStatusCode");
    if status.is_none() && json::field(&members, "responseErrorReason").is_none() {
        return PausedStage::Request;
    }
    let url = json::field(&members, "request")
        .and_then(json::whole_object)
        .and_then(|request| json::field(&request, "url").and_then(json::string_value));
    let headers = json::field(&members, "responseHeaders")
        .and_then(json::array_elements)
        .map(|entries| {
            entries
                .into_iter()
                .filter_map(|entry| {
                    let entry = json::whole_object(entry)?;
                    Some((
                        json::string_value(json::field(&entry, "name")?)?,
                        json::string_value(json::field(&entry, "value")?)?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    PausedStage::Response(url.map(|url| PausedResponse {
        url,
        resource_type: json::field(&members, "resourceType").and_then(json::string_value),
        status: if json::field(&members, "responseErrorReason").is_some() {
            None
        } else {
            status.and_then(|status| status.parse().ok())
        },
        headers,
    }))
}

/// True for a JSON or HTML response that is UTF-8 (stated, or the default for JSON and for
/// YouTube's pages): the only bodies the rewrite will read and re-encode.
pub(crate) fn utf8_text_response(headers: &[(String, String)]) -> bool {
    let Some(content_type) = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.to_ascii_lowercase())
    else {
        return false;
    };
    let mut parts = content_type.split(';');
    let essence = parts.next().unwrap_or("").trim();
    matches!(essence, "application/json" | "text/html")
        && parts.all(|part| match part.trim().split_once('=') {
            Some((key, value)) if key.trim() == "charset" => {
                value.trim().trim_matches('"') == "utf-8"
            }
            _ => part.trim() != "charset",
        })
}

/// Endpoint classification, MIME type and resource kind must agree; network failures, redirects,
/// errors, binary bodies and oversized responses continue without reading or changing the body.
pub(crate) fn response_can_rewrite(
    paused: &PausedResponse,
    payload: super::youtube::PlayerPayload,
) -> bool {
    if paused.status != Some(200) || !utf8_text_response(&paused.headers) {
        return false;
    }
    if paused.headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("content-length")
            && value
                .trim()
                .parse::<u64>()
                .is_ok_and(|len| len > MAX_REWRITE_BODY_BYTES as u64)
    }) {
        return false;
    }
    let content_type = paused
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.split(';').next().unwrap_or("").trim())
        .unwrap_or("");
    match payload {
        super::youtube::PlayerPayload::Json => {
            content_type.eq_ignore_ascii_case("application/json")
                && matches!(paused.resource_type.as_deref(), Some("Fetch" | "XHR"))
        }
        super::youtube::PlayerPayload::WatchPage => {
            content_type.eq_ignore_ascii_case("text/html")
                && paused.resource_type.as_deref() == Some("Document")
        }
    }
}

/// The text of a `Fetch.getResponseBody` result, or `None` when it is not UTF-8 or is too big.
pub(crate) fn response_body_text(result: &str) -> Option<String> {
    use super::json;
    let members = json::whole_object(result)?;
    if json::field(&members, "code").is_some() {
        return None;
    }
    let body = json::string_value(json::field(&members, "body")?)?;
    let text = match json::field(&members, "base64Encoded")? {
        "true" if body.len() <= MAX_REWRITE_BODY_BYTES.div_ceil(3) * 4 => {
            String::from_utf8(json::base64_decode(&body)?).ok()?
        }
        "false" => body,
        _ => return None,
    };
    (text.len() <= MAX_REWRITE_BODY_BYTES).then_some(text)
}

/// `Fetch.fulfillRequest` parameters for a rewritten body. DevTools hands the body over already
/// decoded, so the headers that described the network bytes (length, content and transfer
/// encoding) are dropped; every other header, `Set-Cookie` included, is passed on as received.
pub(crate) fn fulfill_params(
    request_id: &str,
    status: u16,
    headers: &[(String, String)],
    body: &[u8],
) -> String {
    let mut json = format!(
        "{{\"requestId\":{},\"responseCode\":{status},\"responseHeaders\":[",
        super::json::string_literal(request_id)
    );
    let kept = headers.iter().filter(|(name, _)| {
        !["content-length", "content-encoding", "transfer-encoding"]
            .iter()
            .any(|dropped| name.eq_ignore_ascii_case(dropped))
    });
    for (index, (name, value)) in kept.enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push_str("{\"name\":");
        json.push_str(&super::json::string_literal(name));
        json.push_str(",\"value\":");
        json.push_str(&super::json::string_literal(value));
        json.push('}');
    }
    json.push_str("],\"body\":\"");
    json.push_str(&super::json::base64_encode(body));
    json.push_str("\"}");
    json
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_redirects_wrong_types_and_oversized_bodies_continue_unchanged() {
        use super::super::youtube::PlayerPayload;
        let mut paused = PausedResponse {
            url: "https://www.youtube.com/youtubei/v1/player".into(),
            status: Some(200),
            resource_type: Some("Fetch".into()),
            headers: vec![(
                "Content-Type".into(),
                "application/json; charset = UTF-8".into(),
            )],
        };
        assert!(response_can_rewrite(&paused, PlayerPayload::Json));
        assert!(!response_can_rewrite(&paused, PlayerPayload::WatchPage));
        for status in [
            None,
            Some(204),
            Some(206),
            Some(302),
            Some(403),
            Some(429),
            Some(500),
        ] {
            paused.status = status;
            assert!(!response_can_rewrite(&paused, PlayerPayload::Json));
        }
        paused.status = Some(200);
        for mime in [
            "text/html",
            "video/mp4",
            "application/json; charset=windows-1252",
            "application/json; charset = windows-1252",
            "application/json; charset",
            "application/json; charset=",
        ] {
            paused.headers[0].1 = mime.into();
            assert!(
                !response_can_rewrite(&paused, PlayerPayload::Json),
                "{mime}"
            );
        }
        paused.headers[0].1 = "text/html; charset=UTF-8".into();
        paused.resource_type = Some("Document".into());
        assert!(response_can_rewrite(&paused, PlayerPayload::WatchPage));
        paused.headers.push((
            "Content-Length".into(),
            (MAX_REWRITE_BODY_BYTES + 1).to_string(),
        ));
        assert!(!response_can_rewrite(&paused, PlayerPayload::WatchPage));
        let failed = r#"{"request":{"url":"https://www.youtube.com/watch?v=fixture"},"resourceType":"Document","responseStatusCode":200,"responseErrorReason":"Aborted"}"#;
        assert!(matches!(
            paused_stage(failed),
            PausedStage::Response(Some(PausedResponse { status: None, .. }))
        ));
        for result in [
            r#"{"code":-32000,"message":"fixture network error"}"#,
            r#"{"body":"{}"}"#,
            r#"{"body":"{}","base64Encoded":null}"#,
            r#"{"body":"!invalid!","base64Encoded":true}"#,
        ] {
            assert_eq!(response_body_text(result), None);
        }
        let result = format!(
            "{{\"body\":\"{}\",\"base64Encoded\":false}}",
            "x".repeat(MAX_REWRITE_BODY_BYTES + 1)
        );
        assert_eq!(response_body_text(&result), None);
    }
    #[test]
    fn a_response_stage_pause_is_never_mistaken_for_a_blocked_request() {
        let request = r#"{"requestId":"interception-job-1.0","request":{"url":"https://doubleclick.net/x","headers":{"responseStatusCode":"1"}},"resourceType":"Fetch"}"#;
        assert_eq!(paused_stage(request), PausedStage::Request);
        let response = r#"{"requestId":"interception-job-2.0","request":{"url":"https://www.youtube.com/youtubei/v1/player?prettyPrint=false","method":"POST","headers":{}},"frameId":"F","resourceType":"Fetch","responseStatusCode":200,"responseHeaders":[{"name":"content-type","value":"application/json; charset=UTF-8"},{"name":"content-encoding","value":"br"},{"name":"set-cookie","value":"YSC=a; Path=/"}],"networkId":"N"}"#;
        assert_eq!(
            paused_stage(response),
            PausedStage::Response(Some(PausedResponse {
                url: "https://www.youtube.com/youtubei/v1/player?prettyPrint=false".to_owned(),
                status: Some(200),
                resource_type: Some("Fetch".to_owned()),
                headers: vec![
                    (
                        "content-type".to_owned(),
                        "application/json; charset=UTF-8".to_owned()
                    ),
                    ("content-encoding".to_owned(), "br".to_owned()),
                    ("set-cookie".to_owned(), "YSC=a; Path=/".to_owned()),
                ],
            }))
        );
        let failed = r#"{"requestId":"r","request":{"url":"https://www.youtube.com/watch?v=x"},"resourceType":"Document","responseErrorReason":"ConnectionReset"}"#;
        assert!(matches!(
            paused_stage(failed),
            PausedStage::Response(Some(PausedResponse { status: None, .. }))
        ));
        // Unreadable, but plainly a response: continued, not failed.
        assert_eq!(
            paused_stage(r#"{"requestId":"r","responseStatusCode":200,"#),
            PausedStage::Response(None)
        );
    }

    #[test]
    fn only_utf8_json_and_html_bodies_are_rewritten() {
        let headers = |value: &str| vec![("Content-Type".to_owned(), value.to_owned())];
        for ok in [
            "application/json; charset=UTF-8",
            "application/json",
            "text/html; charset=utf-8",
            "text/html; charset=\"utf-8\"",
        ] {
            assert!(utf8_text_response(&headers(ok)), "{ok}");
        }
        for not in [
            "text/html; charset=windows-1252",
            "application/x-protobuf",
            "video/mp4",
            "text/plain",
        ] {
            assert!(!utf8_text_response(&headers(not)), "{not}");
        }
        assert!(!utf8_text_response(&[]));
    }

    #[test]
    fn a_response_body_is_read_whether_or_not_devtools_base64_encoded_it() {
        assert_eq!(
            response_body_text(r#"{"body":"{\"adSlots\":[]}","base64Encoded":false}"#).as_deref(),
            Some(r#"{"adSlots":[]}"#)
        );
        assert_eq!(
            response_body_text(r#"{"body":"eyJhZFNsb3RzIjpbXX0=","base64Encoded":true}"#)
                .as_deref(),
            Some(r#"{"adSlots":[]}"#)
        );
        assert_eq!(
            response_body_text(r#"{"body":"/w==","base64Encoded":true}"#),
            None
        );
        assert_eq!(response_body_text(r#"{"base64Encoded":false}"#), None);
    }

    #[test]
    fn fulfill_params_keep_status_and_headers_but_drop_the_wire_encoding() {
        let headers = vec![
            (
                "content-type".to_owned(),
                "application/json; charset=UTF-8".to_owned(),
            ),
            ("Content-Encoding".to_owned(), "br".to_owned()),
            ("content-length".to_owned(), "999".to_owned()),
            ("set-cookie".to_owned(), "YSC=a; Path=/".to_owned()),
        ];
        assert_eq!(
            fulfill_params("job-1", 200, &headers, b"{}"),
            r#"{"requestId":"job-1","responseCode":200,"responseHeaders":[{"name":"content-type","value":"application/json; charset=UTF-8"},{"name":"set-cookie","value":"YSC=a; Path=/"}],"body":"e30="}"#
        );
    }
}
