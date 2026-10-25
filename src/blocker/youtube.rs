//! YouTube: take the ad schedule out of the player response, and nothing else.
//!
//! YouTube serves its ads from its own domain and decides when to play them from members of the
//! player response, so no request rule can stop a pre-roll without also stopping the video. The
//! engine pauses that one response at the DevTools `Fetch` Response stage (see
//! [`RESPONSE_PATTERNS`]) and fulfills it with [`strip_player_ads`]'s output: the same response
//! minus the top-level [`PLAYER_AD_KEYS`] members. Every other member is copied byte for byte, so
//! `streamingData` (formats, adaptiveFormats, itags) and the quality the player picks from it are
//! exactly what the server sent. Video and audio bytes come from googlevideo.com; they are never
//! paused, rewritten or proxied.
//!
//! The ad beacons and ad-break requests themselves are ordinary rules in `src/blocklist.txt`.

use super::json;

/// Player response members that schedule ads: pre/mid/post-roll placements, the companion and
/// overlay ads, the slot-based ad layouts, and the live ad-break heartbeat.
pub const PLAYER_AD_KEYS: [&str; 4] = [
    "adPlacements",
    "playerAds",
    "adSlots",
    "adBreakHeartbeatParams",
];

/// Where a player response sits in a response body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerPayload {
    /// `/youtubei/v1/player`: the body is the player response object.
    Json,
    /// A watch, embed, Shorts or live document with an inline player response.
    WatchPage,
}

/// One DevTools `Fetch.RequestPattern` paused at the Response stage.
#[derive(Debug)]
pub struct ResponsePattern {
    pub url_pattern: &'static str,
    pub resource_type: Option<&'static str>,
}

/// The responses the engine pauses for [`strip_player_ads`]. In these patterns `*` is any run,
/// an unescaped `?` is any one character and `\?` is a literal `?`. They only narrow what pauses:
/// [`player_payload`] makes the exact host and path check, and anything else continues untouched.
pub const RESPONSE_PATTERNS: &[ResponsePattern] = &[
    ResponsePattern {
        url_pattern: "*://youtube.com/youtubei/v1/player",
        resource_type: None,
    },
    ResponsePattern {
        url_pattern: "*://youtube.com/youtubei/v1/player\\?*",
        resource_type: None,
    },
    ResponsePattern {
        url_pattern: "*://youtube.com/embed/*",
        resource_type: Some("Document"),
    },
    ResponsePattern {
        url_pattern: "*://youtube.com/watch*",
        resource_type: Some("Document"),
    },
    ResponsePattern {
        url_pattern: "*://youtube.com/shorts/*",
        resource_type: Some("Document"),
    },
    ResponsePattern {
        url_pattern: "*://youtube.com/live/*",
        resource_type: Some("Document"),
    },
    ResponsePattern {
        url_pattern: "*://*.youtube.com/youtubei/v1/player",
        resource_type: None,
    },
    ResponsePattern {
        url_pattern: "*://*.youtube.com/youtubei/v1/player\\?*",
        resource_type: None,
    },
    ResponsePattern {
        url_pattern: "*://*.youtube.com/embed/*",
        resource_type: Some("Document"),
    },
    ResponsePattern {
        url_pattern: "*://*.youtube.com/watch*",
        resource_type: Some("Document"),
    },
    ResponsePattern {
        url_pattern: "*://*.youtube.com/shorts/*",
        resource_type: Some("Document"),
    },
    ResponsePattern {
        url_pattern: "*://*.youtube.com/live/*",
        resource_type: Some("Document"),
    },
    ResponsePattern {
        url_pattern: "*://youtube-nocookie.com/youtubei/v1/player",
        resource_type: None,
    },
    ResponsePattern {
        url_pattern: "*://youtube-nocookie.com/youtubei/v1/player\\?*",
        resource_type: None,
    },
    ResponsePattern {
        url_pattern: "*://youtube-nocookie.com/embed/*",
        resource_type: Some("Document"),
    },
    ResponsePattern {
        url_pattern: "*://*.youtube-nocookie.com/youtubei/v1/player",
        resource_type: None,
    },
    ResponsePattern {
        url_pattern: "*://*.youtube-nocookie.com/youtubei/v1/player\\?*",
        resource_type: None,
    },
    ResponsePattern {
        url_pattern: "*://*.youtube-nocookie.com/embed/*",
        resource_type: Some("Document"),
    },
];

/// `youtube.com` / `youtube-nocookie.com` or one of their subdomains, by exact suffix.
fn site_host<'a>(host: &str, sites: &[&'a str]) -> Option<&'a str> {
    sites.iter().copied().find(|site| {
        host.strip_suffix(site)
            .is_some_and(|rest| rest.is_empty() || rest.ends_with('.'))
    })
}

/// True when a lowercase page host is `youtube.com` / `youtube-nocookie.com` or a subdomain of
/// one: the only pages whose player responses are paused at all.
pub fn is_youtube_host(host: &str) -> bool {
    site_host(host, &["youtube.com", "youtube-nocookie.com"]).is_some()
}

/// Which player response `url` returns, if it is one this module rewrites. Only the exact paths
/// count: `/youtubei/v1/player/heartbeat` (live streams) and `/youtubei/v1/next` are not.
pub fn player_payload(url: &str) -> Option<PlayerPayload> {
    let (host, path) = super::split_url(url)?;
    let site = site_host(&host, &["youtube.com", "youtube-nocookie.com"])?;
    let path = path.split('?').next().unwrap_or(path);
    match path {
        "/youtubei/v1/player" => Some(PlayerPayload::Json),
        "/watch" if site == "youtube.com" => Some(PlayerPayload::WatchPage),
        _ => {
            let (kind, id) = path.strip_prefix('/')?.split_once('/')?;
            let document =
                kind == "embed" || (site == "youtube.com" && matches!(kind, "shorts" | "live"));
            (document
                && !matches!(id, "videoseries" | "live_stream")
                && id.len() == 11
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')))
            .then_some(PlayerPayload::WatchPage)
        }
    }
}

/// The profile `settings.txt` key that turns YouTube video-ad stripping on or off. Its value is
/// exactly `true` or `false`; a missing line means on, the default and the behaviour Bowser has
/// always had.
pub const BLOCK_VIDEO_ADS_KEY: &str = "block_youtube_video_ads";

/// Reads [`BLOCK_VIDEO_ADS_KEY`] from the text of a profile `settings.txt`. Other lines and
/// invalid values are ignored; among valid `true`/`false` values the last one wins; with no valid
/// line at all the result is `true`. Callers treat an unreadable or missing file as empty text, so
/// a failed read also means on.
pub fn video_ads_blocked(settings_text: &str) -> bool {
    let mut blocked = true;
    for line in settings_text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != BLOCK_VIDEO_ADS_KEY {
            continue;
        }
        match value.trim() {
            "true" => blocked = true,
            "false" => blocked = false,
            _ => {}
        }
    }
    blocked
}

/// [`video_ads_blocked`] for the `settings.txt` at `path`, as the blocker asks on every navigation
/// and player response. The file is reread only when its length or modified time changed since the
/// last answer, so the settings page's immediate save still takes effect. No file, a file over
/// `cap` bytes, or one that cannot be read means on.
pub fn video_ads_blocked_in_file(path: &std::path::Path, cap: usize) -> bool {
    use std::{path::PathBuf, sync::Mutex, time::SystemTime};
    static LAST: Mutex<Option<(PathBuf, u64, SystemTime, bool)>> = Mutex::new(None);
    let Ok(meta) = std::fs::metadata(path) else {
        return true;
    };
    let (len, modified) = (meta.len(), meta.modified().ok());
    if len > cap as u64 {
        return true;
    }
    let mut last = LAST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let (Some((cached, cached_len, cached_modified, blocked)), Some(modified)) =
        (&*last, modified)
    {
        if cached == path && *cached_len == len && *cached_modified == modified {
            return *blocked;
        }
    }
    let blocked = std::fs::read(path)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .filter(|text| text.len() <= cap)
        .is_none_or(|text| video_ads_blocked(&text));
    *last = modified.map(|modified| (path.to_path_buf(), len, modified, blocked));
    blocked
}

fn is_ad_key(key: &str) -> bool {
    PLAYER_AD_KEYS.contains(&key)
}

/// The object whose `{` is at `start` with its top-level ad members left out, and the index just
/// past the original object. `None` when it has no ad member or cannot be read.
fn without_ad_members(text: &str, start: usize) -> Option<(String, usize)> {
    let (members, end) = json::object_members(text, start)?;
    if !members.iter().any(|member| is_ad_key(member.key)) {
        return None;
    }
    let mut object = String::with_capacity(end - start);
    object.push('{');
    for member in members.iter().filter(|member| !is_ad_key(member.key)) {
        if object.len() > 1 {
            object.push(',');
        }
        object.push_str(&text[member.span.clone()]);
    }
    object.push('}');
    Some((object, end))
}

const INLINE_PLAYER_RESPONSE: &str = "ytInitialPlayerResponse";

// Only a first-statement global assignment is accepted. This deliberately avoids interpreting
// arbitrary JavaScript (strings, templates, regular expressions, functions or property chains).
fn strip_inline_script(script: &str) -> Option<String> {
    let bytes = script.as_bytes();
    let mut start = json::skip_ws(bytes, 0);
    for declaration in ["var", "let", "const"] {
        if script[start..].starts_with(declaration)
            && bytes
                .get(start + declaration.len())
                .is_some_and(u8::is_ascii_whitespace)
        {
            start = json::skip_ws(bytes, start + declaration.len());
            break;
        }
    }
    let target = [
        INLINE_PLAYER_RESPONSE,
        "window.ytInitialPlayerResponse",
        "window[\"ytInitialPlayerResponse\"]",
        "window['ytInitialPlayerResponse']",
    ]
    .iter()
    .find(|target| script[start..].starts_with(**target))?;
    let equals = json::skip_ws(bytes, start + target.len());
    if bytes.get(equals) != Some(&b'=') || matches!(bytes.get(equals + 1), Some(b'=' | b'>')) {
        return None;
    }
    let open = json::skip_ws(bytes, equals + 1);
    let (object, end) = without_ad_members(script, open)?;
    let tail = json::skip_ws(bytes, end);
    if tail < bytes.len() && bytes[tail] != b';' {
        return None;
    }
    Some(format!("{}{object}{}", &script[..open], &script[end..]))
}

// Walk HTML tag boundaries respecting quoted attributes. Unknown or truncated markup fails open.
fn tag_end(bytes: &[u8], mut index: usize) -> Option<usize> {
    let mut quote = None;
    while let Some(&byte) = bytes.get(index) {
        match (quote, byte) {
            (Some(q), b) if q == b => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'>') => return Some(index + 1),
            _ => {}
        }
        index += 1;
    }
    None
}

fn executable_inline_script(attributes: &str) -> bool {
    let bytes = attributes.as_bytes();
    let mut i = 0;
    let mut saw_type = false;
    while i < bytes.len() {
        i = json::skip_ws(bytes, i);
        if i == bytes.len() {
            break;
        }
        if bytes[i] == b'/' {
            return false;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && !matches!(bytes[i], b'=' | b'/')
        {
            i += 1;
        }
        if i == start {
            return false;
        }
        let name = &attributes[start..i];
        i = json::skip_ws(bytes, i);
        let mut value = "";
        if bytes.get(i) == Some(&b'=') {
            i = json::skip_ws(bytes, i + 1);
            let Some(&first) = bytes.get(i) else {
                return false;
            };
            if matches!(first, b'\'' | b'"') {
                let start = i + 1;
                let Some(end) = bytes[start..]
                    .iter()
                    .position(|b| *b == first)
                    .map(|n| start + n)
                else {
                    return false;
                };
                value = &attributes[start..end];
                i = end + 1;
            } else {
                let start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                value = &attributes[start..i];
            }
        }
        if ["src", "nomodule", "language"]
            .iter()
            .any(|excluded| name.eq_ignore_ascii_case(excluded))
        {
            return false;
        }
        if name.eq_ignore_ascii_case("type") {
            if saw_type
                || !["", "text/javascript", "application/javascript"]
                    .iter()
                    .any(|kind| value.trim().eq_ignore_ascii_case(kind))
            {
                return false;
            }
            saw_type = true;
        }
    }
    true
}

fn raw_text_close(lower: &str, start: usize, name: &str) -> Option<(usize, usize)> {
    let pattern = format!("</{name}");
    let mut search = start;
    while let Some(found) = lower[search..].find(&pattern) {
        let close = search + found;
        let after_name = close + pattern.len();
        if lower
            .as_bytes()
            .get(after_name)
            .is_some_and(|b| b.is_ascii_whitespace() || matches!(b, b'/' | b'>'))
        {
            return Some((close, tag_end(lower.as_bytes(), after_name)?));
        }
        search = after_name;
    }
    None
}

/// Rewrite executable inline scripts only. Skip comments, quoted tag attributes and inert/raw
/// text containers, and retain every byte outside the player object (including script nonces).
fn strip_watch_page(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut out = String::new();
    let mut copied = 0;
    let mut search = 0;
    let mut template_depth = 0usize;
    while let Some(found) = lower[search..].find('<') {
        let tag = search + found;
        if lower[tag..].starts_with("<!--") {
            let Some(end) = lower[tag + 4..].find("-->") else {
                break;
            };
            search = tag + 4 + end + 3;
            continue;
        }
        let Some(end) = tag_end(bytes, tag + 1) else {
            break;
        };
        search = end;
        let closing = bytes.get(tag + 1) == Some(&b'/');
        let start = tag + if closing { 2 } else { 1 };
        let mut name_end = start;
        while bytes.get(name_end).is_some_and(u8::is_ascii_alphanumeric) {
            name_end += 1;
        }
        if !bytes
            .get(name_end)
            .is_some_and(|b| b.is_ascii_whitespace() || matches!(b, b'/' | b'>'))
        {
            continue;
        }
        let name = &lower[start..name_end];
        if name == "template" {
            if closing {
                template_depth = template_depth.saturating_sub(1);
            } else {
                template_depth += 1;
            }
            continue;
        }
        if closing {
            continue;
        }
        if name == "plaintext" {
            break;
        }
        if matches!(
            name,
            "script"
                | "style"
                | "textarea"
                | "title"
                | "xmp"
                | "iframe"
                | "noembed"
                | "noframes"
                | "noscript"
        ) {
            let Some((close, close_end)) = raw_text_close(&lower, end, name) else {
                break;
            };
            search = close_end;
            if name == "script"
                && template_depth == 0
                && executable_inline_script(&html[name_end..end - 1])
            {
                if let Some(script) = strip_inline_script(&html[end..close]) {
                    out.push_str(&html[copied..end]);
                    out.push_str(&script);
                    copied = close;
                }
            }
        }
    }
    if copied == 0 {
        return None;
    }
    out.push_str(&html[copied..]);
    Some(out)
}

/// `body` with the player response's ad members removed, or `None` to leave the response as it
/// is: no ad member was present, or the body is not a player response this can read.
pub fn strip_player_ads(body: &str, payload: PlayerPayload) -> Option<String> {
    match payload {
        PlayerPayload::Json => {
            let start = json::skip_ws(body.as_bytes(), 0);
            let (object, end) = without_ad_members(body, start)?;
            body[end..].trim().is_empty().then_some(object)
        }
        PlayerPayload::WatchPage => strip_watch_page(body),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STREAMING: &str = r#""streamingData":{"expiresInSeconds":"21540","formats":[{"itag":18,"url":"https://rr1---sn-x.googlevideo.com/videoplayback?itag=18&mime=video%2Fmp4","qualityLabel":"360p"}],"adaptiveFormats":[{"itag":137,"bitrate":4400000,"qualityLabel":"1080p","fps":30,"loudnessDb":-2.53},{"itag":251,"audioQuality":"AUDIO_QUALITY_MEDIUM"}]}"#;

    fn player_response() -> String {
        format!(
            r#"{{"responseContext":{{"a":1}},"playabilityStatus":{{"status":"OK"}},{STREAMING},"adPlacements":[{{"adPlacementRenderer":{{"config":{{"kind":"AD_PLACEMENT_KIND_START"}}}}}}],"playerAds":[{{"playerLegacyDesktopWatchAdsRenderer":{{}}}}],"videoDetails":{{"videoId":"dQw4w9WgXcQ","title":"Say \"adPlacements\":[] {{ }}"}},"adSlots":[{{"adSlotRenderer":{{}}}}],"adBreakHeartbeatParams":"Q0FB","playerConfig":{{"adPlacements":"nested, left alone"}}}}"#
        )
    }

    #[test]
    fn the_player_response_loses_only_its_ad_members() {
        let stripped = strip_player_ads(&player_response(), PlayerPayload::Json).expect("stripped");
        assert_eq!(
            stripped,
            format!(
                r#"{{"responseContext":{{"a":1}},"playabilityStatus":{{"status":"OK"}},{STREAMING},"videoDetails":{{"videoId":"dQw4w9WgXcQ","title":"Say \"adPlacements\":[] {{ }}"}},"playerConfig":{{"adPlacements":"nested, left alone"}}}}"#
            )
        );
        // Formats, itags and quality labels are byte for byte what the server sent.
        assert!(stripped.contains(STREAMING));
        let members = json::whole_object(&stripped).expect("still one JSON object");
        for key in PLAYER_AD_KEYS {
            assert_eq!(json::field(&members, key), None, "{key}");
        }
    }

    #[test]
    fn a_response_with_no_ads_or_that_cannot_be_read_is_left_alone() {
        let clean = format!(r#"{{"playabilityStatus":{{"status":"OK"}},{STREAMING}}}"#);
        assert_eq!(strip_player_ads(&clean, PlayerPayload::Json), None);
        for body in [
            "",
            ")]}'\n{\"adPlacements\":[]}",
            "{\"adPlacements\":[}",
            "{\"adPlacements\":[]} {}",
            "[{\"adPlacements\":[]}]",
        ] {
            assert_eq!(
                strip_player_ads(body, PlayerPayload::Json),
                None,
                "{body:?}"
            );
        }
        assert_eq!(
            strip_player_ads(" {\"adSlots\":[],\"x\":1}\n", PlayerPayload::Json).as_deref(),
            Some("{\"x\":1}")
        );
        assert_eq!(
            strip_player_ads("{\"playerAds\":[]}", PlayerPayload::Json).as_deref(),
            Some("{}")
        );
    }

    #[test]
    fn the_inline_watch_page_response_is_stripped_in_place() {
        let head = r#"<html><script nonce="n">var ytInitialData = {"adPlacements":"not the player"};</script><script nonce="n">var ytInitialPlayerResponse = "#;
        let tail = r#";var meta = document.createElement('meta');</script></html>"#;
        let page = format!("{head}{}{tail}", player_response());
        let stripped = strip_player_ads(&page, PlayerPayload::WatchPage).expect("stripped");
        let expected = strip_player_ads(&player_response(), PlayerPayload::Json).unwrap();
        assert_eq!(stripped, format!("{head}{expected}{tail}"));

        let bracketed = format!(
            "<script>window[\"ytInitialPlayerResponse\"] = {};</script>",
            player_response()
        );
        assert_eq!(
            strip_player_ads(&bracketed, PlayerPayload::WatchPage),
            Some(format!(
                "<script>window[\"ytInitialPlayerResponse\"] = {expected};</script>"
            ))
        );
    }

    #[test]
    fn a_watch_page_without_an_inline_ad_schedule_is_left_alone() {
        for page in [
            "<html>no player here</html>",
            "if (window.ytInitialPlayerResponse == {\"adSlots\":[]}) {}",
            "var ytInitialPlayerResponse = null;",
            "var ytInitialPlayerResponse = {\"playabilityStatus\":{}};",
            "var ytInitialPlayerResponse = {\"adSlots\":[;",
        ] {
            assert_eq!(
                strip_player_ads(page, PlayerPayload::WatchPage),
                None,
                "{page:?}"
            );
        }
    }

    #[test]
    fn only_youtube_player_endpoints_and_watch_pages_are_rewritten() {
        for (url, payload) in [
            (
                "https://www.youtube.com/youtubei/v1/player?prettyPrint=false",
                PlayerPayload::Json,
            ),
            (
                "https://m.youtube.com/youtubei/v1/player?key=x",
                PlayerPayload::Json,
            ),
            (
                "https://music.youtube.com/youtubei/v1/player",
                PlayerPayload::Json,
            ),
            (
                "https://www.youtube-nocookie.com/youtubei/v1/player?prettyPrint=false",
                PlayerPayload::Json,
            ),
            (
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                PlayerPayload::WatchPage,
            ),
            (
                "https://WWW.YouTube.com/watch?v=dQw4w9WgXcQ&t=1",
                PlayerPayload::WatchPage,
            ),
        ] {
            assert_eq!(player_payload(url), Some(payload), "{url}");
        }
        for url in [
            "https://www.youtube.com/youtubei/v1/player/ad_break?prettyPrint=false",
            "https://www.youtube.com/youtubei/v1/player/heartbeat?alt=json",
            "https://www.youtube.com/youtubei/v1/next?prettyPrint=false",
            "https://www.youtube.com/watchlater",
            "https://www.youtube-nocookie.com/watch?v=x",
            "https://notyoutube.com/youtubei/v1/player",
            "https://www.youtube.com.evil.example/youtubei/v1/player",
            "https://evil.example/x.youtube.com/youtubei/v1/player?x",
            "https://rr1---sn-x.googlevideo.com/videoplayback?itag=137",
        ] {
            assert_eq!(player_payload(url), None, "{url}");
        }
    }

    #[test]
    fn video_ads_default_on_and_the_key_toggles_it() {
        // Missing line, empty text, and any unknown/invalid value all mean on.
        for text in [
            "",
            "sleep_enabled=false\nblocker_allow=a.com\n",
            "block_youtube_video_ads=\n",
            "block_youtube_video_ads=1\n",
            "block_youtube_video_ads=TRUE\n",
            "block_youtube_video_ads=yes\n",
            "block_youtube_video_ads\n",
        ] {
            assert!(video_ads_blocked(text), "{text:?}");
        }
        // The exact persisted lines.
        assert!(video_ads_blocked("block_youtube_video_ads=true"));
        assert!(!video_ads_blocked("block_youtube_video_ads=false"));
        assert!(!video_ads_blocked(
            "sleep_enabled=true\nblock_youtube_video_ads=false\nblocker_allow=a.com\n"
        ));
        // Surrounding whitespace on the line is tolerated; an invalid value does not reset a
        // valid earlier one, and a later valid value wins.
        assert!(!video_ads_blocked("  block_youtube_video_ads = false \n"));
        assert!(!video_ads_blocked(
            "block_youtube_video_ads=false\nblock_youtube_video_ads=maybe\n"
        ));
        assert!(video_ads_blocked(
            "block_youtube_video_ads=false\nblock_youtube_video_ads=true\n"
        ));
    }

    #[test]
    fn the_settings_file_is_reread_only_when_it_changes() {
        let dir =
            std::env::temp_dir().join(format!("bowser-youtube-setting-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.txt");
        assert!(video_ads_blocked_in_file(&path, 1024), "no file means on");
        std::fs::write(&path, "block_youtube_video_ads=false\n").unwrap();
        let stamp = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert!(!video_ads_blocked_in_file(&path, 1024));
        // Same length and modified time: the answer comes from memory, not from the new bytes.
        std::fs::write(&path, "block_youtube_video_ads=maybe\n").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(stamp)
            .unwrap();
        assert!(!video_ads_blocked_in_file(&path, 1024));
        // A changed length is noticed at once.
        std::fs::write(&path, "block_youtube_video_ads=true\n").unwrap();
        assert!(video_ads_blocked_in_file(&path, 1024));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn only_youtube_and_its_subdomains_are_youtube_pages() {
        for host in [
            "youtube.com",
            "www.youtube.com",
            "m.youtube.com",
            "youtube-nocookie.com",
            "www.youtube-nocookie.com",
        ] {
            assert!(is_youtube_host(host), "{host}");
        }
        for host in [
            "",
            "news.example",
            "notyoutube.com",
            "youtube.com.evil.example",
            "youtube.co.uk",
            "googlevideo.com",
        ] {
            assert!(!is_youtube_host(host), "{host}");
        }
    }

    #[test]
    fn response_patterns_escape_query_marks_and_scope_documents() {
        for pattern in RESPONSE_PATTERNS {
            if pattern.url_pattern.contains('?') {
                assert!(pattern.url_pattern.contains("\\?*"), "{pattern:?}");
            }
            if !pattern.url_pattern.contains("/youtubei/v1/player") {
                assert_eq!(pattern.resource_type, Some("Document"));
            }
        }
    }

    // CDP's documented wildcard semantics, including escaped literal query marks. Exercise the
    // registration patterns as well as the exact parser: the old parser-only tests missed bare URLs.
    fn pattern_matches(pattern: &[u8], text: &[u8]) -> bool {
        match pattern.split_first() {
            None => text.is_empty(),
            Some((b'*', rest)) => (0..=text.len()).any(|n| pattern_matches(rest, &text[n..])),
            Some((b'\\', rest)) => rest.first().is_some_and(|literal| {
                text.first() == Some(literal) && pattern_matches(&rest[1..], &text[1..])
            }),
            Some((b'?', rest)) => !text.is_empty() && pattern_matches(rest, &text[1..]),
            Some((literal, rest)) => {
                text.first() == Some(literal) && pattern_matches(rest, &text[1..])
            }
        }
    }

    #[test]
    fn bare_query_player_and_inline_document_routes_are_both_registered_and_classified() {
        for host in [
            "youtube.com",
            "www.youtube.com",
            "m.youtube.com",
            "music.youtube.com",
            "youtube-nocookie.com",
            "www.youtube-nocookie.com",
        ] {
            let mut routes = vec![
                "/youtubei/v1/player",
                "/youtubei/v1/player?prettyPrint=false",
                "/embed/dQw4w9WgXcQ",
                "/embed/dQw4w9WgXcQ?start=12",
            ];
            if !host.contains("nocookie") {
                routes.extend([
                    "/watch",
                    "/watch?v=dQw4w9WgXcQ",
                    "/shorts/dQw4w9WgXcQ",
                    "/live/dQw4w9WgXcQ?feature=share",
                ]);
            }
            for route in routes {
                let url = format!("https://{host}{route}");
                let payload = player_payload(&url).expect(&url);
                assert!(
                    RESPONSE_PATTERNS
                        .iter()
                        .any(|p| pattern_matches(p.url_pattern.as_bytes(), url.as_bytes())),
                    "unregistered {url}"
                );
                if !route.contains("youtubei") {
                    assert_eq!(payload, PlayerPayload::WatchPage);
                }
            }
        }
        for route in [
            "/embed/",
            "/embed/videoseries?list=fixture",
            "/embed/dQw4w9WgXcQ/extra",
            "/shorts",
            "/shorts/invalid-long-id",
            "/live/chat",
            "/live_chat?v=dQw4w9WgXcQ",
            "/watchlater",
            "/youtubei/v1/player/heartbeat",
            "/youtubei/v1/player_extra",
        ] {
            assert_eq!(
                player_payload(&format!("https://www.youtube.com{route}")),
                None,
                "{route}"
            );
        }
    }

    #[test]
    fn inline_rewrite_keeps_nonplayer_text_literals_comments_and_other_objects() {
        for script in [
            r#"const text = 'ytInitialPlayerResponse = {"adSlots":[]}';"#,
            "const text = `ytInitialPlayerResponse = {\"adSlots\":[]}`;",
            "// ytInitialPlayerResponse = {\"adSlots\":[]};",
            "/* ytInitialPlayerResponse = {\"adSlots\":[]}; */",
            "var notytInitialPlayerResponse = {\"adSlots\":[]};",
            "other.ytInitialPlayerResponse = {\"adSlots\":[]};",
            "other. ytInitialPlayerResponse = {\"adSlots\":[]};",
            "other.window.ytInitialPlayerResponse = {\"adSlots\":[]};",
            "window.ytInitialPlayerResponseExtra = {\"adSlots\":[]};",
            "if (ytInitialPlayerResponse == {\"adSlots\":[]}) {}",
            "const regex = /ytInitialPlayerResponse = {\"adSlots\":[]}/;",
            "const text = `${`ytInitialPlayerResponse = {\"adSlots\":[]}`}`;",
        ] {
            assert_eq!(
                strip_player_ads(
                    &format!("<script>{script}</script>"),
                    PlayerPayload::WatchPage
                ),
                None,
                "{script}"
            );
        }
        for html in [
            "<p>ytInitialPlayerResponse = {\"adSlots\":[]}</p>",
            "<div title='ytInitialPlayerResponse = {\"adSlots\":[]}'></div>",
            "<!-- <script>ytInitialPlayerResponse = {\"adSlots\":[]};</script> -->",
            "<div title='<script>ytInitialPlayerResponse = {\"adSlots\":[]};</script>'></div>",
            "<script type='text/plain'>ytInitialPlayerResponse = {\"adSlots\":[]};</script>",
            "<script type='application/json'>ytInitialPlayerResponse = {\"adSlots\":[]};</script>",
            "<script src='/fixture.js'>ytInitialPlayerResponse = {\"adSlots\":[]};</script>",
            "<textarea><script>ytInitialPlayerResponse = {\"adSlots\":[]};</script></textarea>",
            "<style><script>ytInitialPlayerResponse = {\"adSlots\":[]};</script></style>",
            "<template><template></template><script>ytInitialPlayerResponse = {\"adSlots\":[]};</script></template>",
            "<noscript><script>ytInitialPlayerResponse = {\"adSlots\":[]};</script></noscript>",
            "<textarea></textarea-extra><script>ytInitialPlayerResponse={\"adSlots\":[],\"keep\":1};</script></textarea>",
            "<script / type='text/plain'>ytInitialPlayerResponse={\"adSlots\":[]};</script>",
            "<script nomodule>ytInitialPlayerResponse={\"adSlots\":[]};</script>",
            "<script language='vbscript'>ytInitialPlayerResponse={\"adSlots\":[]};</script>",
            "<script>var ytInitialPlayerResponse = {\"adSlots\":[]}.unrelated;</script>",
        ] {
            assert_eq!(strip_player_ads(html, PlayerPayload::WatchPage), None);
        }
    }

    #[test]
    fn multiple_inline_responses_preserve_playability_errors_streams_and_document_bytes() {
        let blocked = r#"{"playabilityStatus":{"status":"LOGIN_REQUIRED","reason":"Age restricted"},"adSlots":[],"videoDetails":{"videoId":"fixture"}}"#;
        let body = format!(
            "<html><script nonce='fixture'>window.ytInitialPlayerResponse={blocked};</script><script>window['ytInitialPlayerResponse']={};</script></html>",
            player_response()
        );
        let expected = format!(
            "<html><script nonce='fixture'>window.ytInitialPlayerResponse={};</script><script>window['ytInitialPlayerResponse']={};</script></html>",
            strip_player_ads(blocked, PlayerPayload::Json).unwrap(),
            strip_player_ads(&player_response(), PlayerPayload::Json).unwrap()
        );
        let result = strip_player_ads(&body, PlayerPayload::WatchPage).unwrap();
        assert_eq!(result, expected);
        assert!(result.contains(
            r#""playabilityStatus":{"status":"LOGIN_REQUIRED","reason":"Age restricted"}"#
        ));
        assert!(result.contains(STREAMING));
        assert_eq!(strip_player_ads(&result, PlayerPayload::WatchPage), None);
    }
}
