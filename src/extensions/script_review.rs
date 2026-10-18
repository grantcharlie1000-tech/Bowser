//! Complete static content-script declarations for consent. This is a review representation,
//! not permission enforcement or a claim that the installed WebView2 implements each field.
use super::{ExtensionError, MAX_LIST_LEN, json, string_list};

const LISTS: &[&str] = &["matches", "exclude_matches", "include_globs", "exclude_globs", "js", "css"];
const FLAGS: &[&str] = &["all_frames", "match_about_blank", "match_origin_as_fallback"];

pub(super) fn declarations(fields: &[(String, json::Value)]) -> Result<Vec<String>, ExtensionError> {
    let Some(value) = json::get(fields, "content_scripts") else { return Ok(Vec::new()); };
    let json::Value::Array(scripts) = value else { return Err(invalid()); };
    if scripts.len() > MAX_LIST_LEN { return Err(invalid()); }
    scripts.iter().map(|script| {
        let json::Value::Object(fields) = script else { return Err(invalid()); };
        for key in LISTS { string_list(fields, key)?; }
        for key in FLAGS {
            if json::get(fields, key).is_some_and(|value| !matches!(value, json::Value::Bool(_))) { return Err(invalid()); }
        }
        for (key, choices) in [("run_at", &["document_start", "document_end", "document_idle"][..]), ("world", &["ISOLATED", "MAIN"][..])] {
            if let Some(value) = json::get(fields, key) {
                if !matches!(value, json::Value::String(text) if choices.contains(&text.as_str())) { return Err(invalid()); }
            }
        }
        let mut defaults = Vec::new();
        for key in FLAGS { if json::get(fields, key).is_none() { defaults.push(((*key).to_owned(), json::Value::Bool(false))); } }
        for (key, value) in [("run_at", "document_idle"), ("world", "ISOLATED")] {
            if json::get(fields, key).is_none() { defaults.push((key.to_owned(), json::Value::String(value.to_owned()))); }
        }
        let unknown = fields.iter().filter(|(key, _)| !LISTS.contains(&key.as_str()) && !FLAGS.contains(&key.as_str()) && !matches!(key.as_str(), "run_at" | "world"))
            .map(|(key, _)| json::Value::String(key.clone())).collect();
        let review = json::Value::Object(vec![
            ("declared".to_owned(), script.clone()),
            ("documented_defaults_for_omitted_fields".to_owned(), json::Value::Object(defaults)),
            ("unrecognized_fields_effect_not_assumed".to_owned(), json::Value::Array(unknown)),
        ]);
        let mut text = String::new(); canonical(&review, &mut text); Ok(text)
    }).collect()
}
fn invalid() -> ExtensionError { ExtensionError::InvalidManifest("Content-script declarations contain an invalid list, flag, execution world or timing value.") }

/// ASCII JSON makes invisible/control/non-ASCII characters visible in the review. Preserve every
/// unknown value, array ordering and numeric token; sort object keys only. This does not guess
/// host-pattern containment or collapse optional and required permission categories.
fn canonical(value: &json::Value, out: &mut String) {
    match value {
        json::Value::Null => out.push_str("null"),
        json::Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        json::Value::Number(value) => out.push_str(value),
        json::Value::String(value) => quoted(value, out),
        json::Value::Array(values) => { out.push('['); for (index, value) in values.iter().enumerate() { if index != 0 { out.push(','); } canonical(value, out); } out.push(']'); }
        json::Value::Object(fields) => {
            let mut fields: Vec<_> = fields.iter().collect(); fields.sort_by(|a, b| a.0.cmp(&b.0));
            out.push('{'); for (index, (key, value)) in fields.iter().enumerate() { if index != 0 { out.push(','); } quoted(key, out); out.push(':'); canonical(value, out); } out.push('}');
        }
    }
}
fn quoted(text: &str, out: &mut String) {
    out.push('"');
    for unit in text.encode_utf16() {
        match unit {
            0x22 => out.push_str("\\\""), 0x5c => out.push_str("\\\\"),
            0x20..=0x7e => out.push(char::from_u32(unit as u32).expect("ASCII unit")),
            _ => { use std::fmt::Write; let _ = write!(out, "\\u{unit:04x}"); }
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest(script: &str) -> super::super::Manifest {
        super::super::parse_manifest(format!(r#"{{"manifest_version":3,"name":"Review","version":"1","content_scripts":[{script}]}}"#).as_bytes()).unwrap()
    }
    #[test]
    fn content_script_review_preserves_full_scope_files_and_explicit_defaults() {
        let value = manifest(r#"{"matches":["https://example.test/*"],"exclude_matches":["https://example.test/private/*"],"include_globs":["*public*"],"exclude_globs":["*secret*"],"js":["first.js","second.js"],"css":["style.css"],"all_frames":true,"match_about_blank":true,"match_origin_as_fallback":true,"world":"MAIN","run_at":"document_start"}"#);
        let text = &value.content_script_declarations[0];
        for field in ["exclude_matches", "include_globs", "exclude_globs", "first.js", "second.js", "style.css", "all_frames", "match_about_blank", "match_origin_as_fallback", "MAIN", "document_start"] { assert!(text.contains(field), "{field}: {text}"); }
        assert!(text.contains("\"documented_defaults_for_omitted_fields\":{}"));
        let value = manifest(r#"{"matches":["https://example.test/*"],"js":["a.js"]}"#);
        let text = &value.content_script_declarations[0];
        for field in ["\"all_frames\":false", "\"match_about_blank\":false", "\"match_origin_as_fallback\":false", "\"world\":\"ISOLATED\"", "\"run_at\":\"document_idle\""] { assert!(text.contains(field), "{text}"); }
    }
    #[test]
    fn content_script_review_preserves_unknown_values_and_visible_unicode() {
        let value = manifest(r#"{"matches":["https://example.test/*"],"future":{"world":"\u202e","values":[1,true,null,"emoji😀"]}}"#);
        let text = &value.content_script_declarations[0]; assert!(text.is_ascii()); assert!(text.contains("\\u202e")); assert!(text.contains("\\ud83d\\ude00"));
        assert!(text.contains("\"unrecognized_fields_effect_not_assumed\":[\"future\"]")); assert!(json::parse(text).is_some());
    }
    #[test]
    fn content_script_review_canonicalizes_keys_without_erasing_declaration_or_array_changes() {
        let a = manifest(r#"{"matches":["https://example.test/*"],"js":["a.js","b.js"]}"#);
        let b = manifest(r#"{"js":["a.js","b.js"],"matches":["https://example.test/*"]}"#);
        assert_eq!(a.content_script_declarations, b.content_script_declarations);
        let c = manifest(r#"{"matches":["https://example.test/*"],"js":["b.js","a.js"]}"#);
        assert_ne!(a.content_script_declarations, c.content_script_declarations);
        let d = manifest(r#"{"matches":["https://example.test/*"],"js":["a.js","b.js"],"all_frames":false}"#);
        assert_ne!(a.content_script_declarations, d.content_script_declarations, "explicit and omitted defaults remain distinguishable");
    }
    #[test]
    fn content_script_review_refuses_an_oversize_complete_wire_review_without_truncation() {
        let normal = manifest(r#"{"matches":["https://example.test/*"],"js":["a.js"]}"#);
        assert!(super::super::store_review_fits(&normal));
        let padded = manifest(&format!(r#"{{"matches":["https://example.test/*"],"future":"{}"}}"#, "x".repeat(190 * 1024)));
        assert!(padded.content_script_declarations[0].len() > 190 * 1024);
        assert!(!super::super::store_review_fits(&padded));
        for (count, expected) in [(44 * 1024, true), (48 * 1024, false)] {
            let escaped = manifest(&format!(r#"{{"matches":["https://example.test/*"],"future":"{}"}}"#, r#"\\"#.repeat(count)));
            assert!(escaped.content_script_declarations[0].len() < 180 * 1024);
            assert_eq!(super::super::store_review_fits(&escaped), expected, "outer JSON escaping must count toward the review budget");
        }
    }
    #[test]
    fn content_script_review_rejects_invalid_scope_types_and_known_enum_values() {
        for script in [r#"{"all_frames":"false"}"#, r#"{"match_about_blank":1}"#, r#"{"match_origin_as_fallback":null}"#, r#"{"world":"UNKNOWN"}"#, r#"{"run_at":"later"}"#, r#"{"exclude_matches":[false]}"#, r#"{"include_globs":"*"}"#, r#"{"js":[42]}"#] {
            let bytes = format!(r#"{{"manifest_version":3,"name":"Review","version":"1","content_scripts":[{script}]}}"#);
            assert!(super::super::parse_manifest(bytes.as_bytes()).is_err(), "{script}");
        }
    }
}
