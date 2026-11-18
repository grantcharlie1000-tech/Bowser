//! YouTube: take the ad schedule out of the player response, and nothing else.
//!
//! YouTube serves its ads from its own domain and decides when to play them from members of the
//! player response, so no request rule can stop a pre-roll without also stopping the video. The
//! engine pauses those responses at the DevTools `Fetch` Response stage (see
//! [`RESPONSE_PATTERNS`]) and fulfills them with [`strip_player_ads`]'s output: the same response
//! minus the [`PLAYER_AD_KEYS`] members of the player response. Every other member is copied byte
//! for byte, so `streamingData` (formats, adaptiveFormats, itags) and the quality the player picks
//! from it are exactly what the server sent. Video and audio bytes come from googlevideo.com; they
//! are never paused, rewritten or proxied.
//!
//! The player response reaches the page three ways, and all three are stripped:
//! * the first watch page inlines it as `ytInitialPlayerResponse` in the HTML;
//! * `/youtubei/v1/player` returns it as the whole body, with or without a query string;
//! * `/youtubei/v1/get_watch`, which the site uses for in-app navigation to a video, returns it as
//!   a `playerResponse` member, of the body or of each element of a body that is an array.
//!
//! The ad beacons and ad-break requests themselves are ordinary rules in `src/blocklist.txt`, and
//! the ad cards in the home feed, search results, Shorts and watch sidebar are hidden by [`CSS`].

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
    /// `/youtubei/v1/player` or `/youtubei/v1/get_watch`: a JSON body that is the player response
    /// object, an object holding it as `playerResponse`, or an array of such objects.
    Json,
    /// A `/watch` page: the player response is the inline `ytInitialPlayerResponse` object.
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
///
/// Each endpoint is listed twice, bare and with a query string, because YouTube sends both: a
/// pattern ending in `\?*` alone never matches `/youtubei/v1/player` with no query, and a pattern
/// ending in `player*` would also pause every `/player/heartbeat` of a live stream.
pub const RESPONSE_PATTERNS: [ResponsePattern; 9] = [
    ResponsePattern {
        url_pattern: "*://*.youtube.com/youtubei/v1/player",
        resource_type: None,
    },
    ResponsePattern {
        url_pattern: "*://*.youtube.com/youtubei/v1/player\\?*",
        resource_type: None,
    },
    ResponsePattern {
        url_pattern: "*://*.youtube.com/youtubei/v1/get_watch",
        resource_type: None,
    },
    ResponsePattern {
        url_pattern: "*://*.youtube.com/youtubei/v1/get_watch\\?*",
        resource_type: None,
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
        url_pattern: "*://*.youtube-nocookie.com/youtubei/v1/get_watch",
        resource_type: None,
    },
    ResponsePattern {
        url_pattern: "*://*.youtube-nocookie.com/youtubei/v1/get_watch\\?*",
        resource_type: None,
    },
    ResponsePattern {
        url_pattern: "*://*.youtube.com/watch\\?*",
        resource_type: Some("Document"),
    },
];

/// The cosmetic rules for YouTube pages: the ad cards YouTube puts in the home feed, search
/// results, Shorts and the watch sidebar, the masthead and companion ads, and the in-player overlay
/// ads. They arrive in ordinary `/browse`, `/search` and `/next` responses beside real videos, so
/// no request rule can drop them, and the list grammar (`.class` / `#id` only) cannot name them.
/// Each selector is its own rule, so one an engine cannot parse costs only itself.
pub const CSS: &str = concat!(
    "ytd-ad-slot-renderer{display:none!important}",
    "ytd-rich-item-renderer:has(> #content > ytd-ad-slot-renderer){display:none!important}",
    "ytd-in-feed-ad-layout-renderer{display:none!important}",
    "ytd-display-ad-renderer{display:none!important}",
    "ytd-promoted-sparkles-web-renderer{display:none!important}",
    "ytd-promoted-sparkles-text-search-renderer{display:none!important}",
    "ytd-promoted-video-renderer{display:none!important}",
    "ytd-compact-promoted-video-renderer{display:none!important}",
    "ytd-search-pyv-renderer{display:none!important}",
    "ytd-companion-slot-renderer{display:none!important}",
    "ytd-action-companion-ad-renderer{display:none!important}",
    "ytd-player-legacy-desktop-watch-ads-renderer{display:none!important}",
    "ytd-engagement-panel-section-list-renderer[target-id=engagement-panel-ads]{display:none!important}",
    "#masthead-ad{display:none!important}",
    "#player-ads{display:none!important}",
    ".ytp-ad-overlay-container{display:none!important}",
    ".ytd-shorts:has(> .ytd-reel-video-renderer > ytd-ad-slot-renderer){display:none!important}",
    "ytm-promoted-sparkles-web-renderer{display:none!important}",
    "ytm-companion-ad-renderer{display:none!important}",
    "ad-slot-renderer{display:none!important}",
);

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
    match path.split('?').next().unwrap_or(path) {
        "/youtubei/v1/player" | "/youtubei/v1/get_watch" => Some(PlayerPayload::Json),
        "/watch" if site == "youtube.com" => Some(PlayerPayload::WatchPage),
        _ => None,
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
    let mut last = LAST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let (Some((cached, cached_len, cached_modified, blocked)), Some(modified)) = (&*last, modified) {
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

/// The member of a `get_watch` response object (and of each element of one that is an array)
/// that holds the player response.
const NESTED_PLAYER_RESPONSE: &str = "playerResponse";

/// The object whose `{` is at `start` with its top-level ad members left out, and, when `nested`
/// is set, with the ad members of its `playerResponse` member object left out too; and the index
/// just past the original object. `None` when there was nothing to leave out or it cannot be read.
fn without_ad_members(text: &str, start: usize, nested: bool) -> Option<(String, usize)> {
    let (members, end) = json::object_members(text, start)?;
    let mut object = String::with_capacity(end - start);
    let mut changed = false;
    object.push('{');
    for member in &members {
        if is_ad_key(member.key) {
            changed = true;
            continue;
        }
        let value_start = member.span.end - member.value.len();
        let inner = (nested && member.key == NESTED_PLAYER_RESPONSE)
            .then(|| without_ad_members(text, value_start, false))
            .flatten();
        if object.len() > 1 {
            object.push(',');
        }
        match inner {
            Some((stripped, _)) => {
                changed = true;
                object.push_str(&text[member.span.start..value_start]);
                object.push_str(&stripped);
            }
            None => object.push_str(&text[member.span.clone()]),
        }
    }
    object.push('}');
    changed.then_some((object, end))
}

/// A JSON response body that is one object, or one array of values, with the player response's
/// ad members left out wherever [`without_ad_members`] finds them: the body itself
/// (`/youtubei/v1/player`), its `playerResponse` member, or the same in each element of an array
/// (`/youtubei/v1/get_watch`). `None` when nothing changed or the body cannot be read.
fn strip_json_body(body: &str) -> Option<String> {
    let body = body.trim_matches([' ', '\t', '\n', '\r']);
    match body.as_bytes().first()? {
        b'{' => {
            let (object, end) = without_ad_members(body, 0, true)?;
            (end == body.len()).then_some(object)
        }
        b'[' => {
            let elements = json::array_elements(body)?;
            let mut changed = false;
            let mut array = String::with_capacity(body.len());
            array.push('[');
            for (index, element) in elements.iter().enumerate() {
                if index > 0 {
                    array.push(',');
                }
                let stripped = element
                    .starts_with('{')
                    .then(|| without_ad_members(element, 0, true))
                    .flatten();
                match stripped {
                    Some((object, _)) => {
                        changed = true;
                        array.push_str(&object);
                    }
                    None => array.push_str(element),
                }
            }
            array.push(']');
            changed.then_some(array)
        }
        _ => None,
    }
}

const INLINE_PLAYER_RESPONSE: &str = "ytInitialPlayerResponse";

/// Every inline `ytInitialPlayerResponse = {...}` in a watch page, stripped in place.
fn strip_watch_page(html: &str) -> Option<String> {
    let bytes = html.as_bytes();
    let mut out = String::new();
    let mut copied = 0;
    let mut search = 0;
    while let Some(found) = html[search..].find(INLINE_PLAYER_RESPONSE) {
        let mut i = search + found + INLINE_PLAYER_RESPONSE.len();
        search = i;
        // `var ytInitialPlayerResponse = {` and `window["ytInitialPlayerResponse"] = {`.
        if bytes[i..].starts_with(b"\"]") || bytes[i..].starts_with(b"']") {
            i += 2;
        }
        let equals = json::skip_ws(bytes, i);
        if bytes.get(equals) != Some(&b'=') || bytes.get(equals + 1) == Some(&b'=') {
            continue;
        }
        let open = json::skip_ws(bytes, equals + 1);
        let Some((object, end)) = without_ad_members(html, open, false) else {
            continue;
        };
        out.push_str(&html[copied..open]);
        out.push_str(&object);
        copied = end;
        search = end;
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
        PlayerPayload::Json => strip_json_body(body),
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
            "[{\"adPlacements\":[]}] []",
            "[{\"adPlacements\":[]},]",
            "[{\"x\":1},{\"playerResponse\":{\"playabilityStatus\":{}}}]",
            "\"adPlacements\"",
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
            "window[\"ytInitialPlayerResponse\"] = {};",
            player_response()
        );
        assert_eq!(
            strip_player_ads(&bracketed, PlayerPayload::WatchPage),
            Some(format!("window[\"ytInitialPlayerResponse\"] = {expected};"))
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
                "https://www.youtube.com/youtubei/v1/get_watch?prettyPrint=false",
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
        let dir = std::env::temp_dir().join(format!("bowser-youtube-setting-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.txt");
        assert!(video_ads_blocked_in_file(&path, 1024), "no file means on");
        std::fs::write(&path, "block_youtube_video_ads=false\n").unwrap();
        let stamp = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert!(!video_ads_blocked_in_file(&path, 1024));
        // Same length and modified time: the answer comes from memory, not from the new bytes.
        std::fs::write(&path, "block_youtube_video_ads=maybe\n").unwrap();
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(stamp).unwrap();
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
    fn response_patterns_escape_the_query_mark() {
        for pattern in &RESPONSE_PATTERNS {
            let bytes = pattern.url_pattern.as_bytes();
            for (index, byte) in bytes.iter().enumerate() {
                if *byte == b'?' {
                    assert_eq!(index.checked_sub(1).map(|i| bytes[i]), Some(b'\\'), "{pattern:?}");
                }
            }
        }
    }

    /// DevTools `Fetch.RequestPattern.urlPattern` matching: `*` is any run, `?` any one
    /// character, and a backslash makes the next character literal.
    fn devtools_pattern_matches(pattern: &[u8], url: &[u8]) -> bool {
        match pattern.split_first() {
            None => url.is_empty(),
            Some((b'*', rest)) => (0..=url.len()).any(|skip| devtools_pattern_matches(rest, &url[skip..])),
            Some((b'?', rest)) => !url.is_empty() && devtools_pattern_matches(rest, &url[1..]),
            Some((b'\\', rest)) if !rest.is_empty() => {
                url.first() == Some(&rest[0]) && devtools_pattern_matches(&rest[1..], &url[1..])
            }
            Some((byte, rest)) => url.first() == Some(byte) && devtools_pattern_matches(rest, &url[1..]),
        }
    }

    fn paused(url: &str, resource_type: &str) -> bool {
        RESPONSE_PATTERNS.iter().any(|pattern| {
            pattern.resource_type.is_none_or(|kind| kind == resource_type)
                && devtools_pattern_matches(pattern.url_pattern.as_bytes(), url.as_bytes())
        })
    }

    #[test]
    fn every_player_response_the_rewrite_accepts_is_actually_paused() {
        // The XHR/fetch player responses, as the desktop, mobile, music and embedded players send
        // them: with a query string and without one. A pattern that missed any of these let that
        // response through with its full ad schedule.
        for url in [
            "https://www.youtube.com/youtubei/v1/player?prettyPrint=false",
            "https://www.youtube.com/youtubei/v1/player",
            "https://m.youtube.com/youtubei/v1/player?key=x",
            "https://music.youtube.com/youtubei/v1/player",
            "https://www.youtube.com/youtubei/v1/get_watch?prettyPrint=false",
            "https://www.youtube.com/youtubei/v1/get_watch",
            "https://m.youtube.com/youtubei/v1/get_watch?prettyPrint=false",
            "https://www.youtube-nocookie.com/youtubei/v1/player?prettyPrint=false",
            "https://www.youtube-nocookie.com/youtubei/v1/player",
        ] {
            assert_eq!(player_payload(url), Some(PlayerPayload::Json), "{url}");
            assert!(paused(url, "Fetch"), "{url} is never paused");
            assert!(paused(url, "XHR"), "{url} is never paused");
        }
        for url in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ&list=PL1",
        ] {
            assert_eq!(player_payload(url), Some(PlayerPayload::WatchPage), "{url}");
            assert!(paused(url, "Document"), "{url} is never paused");
            assert!(!paused(url, "Fetch"), "{url} only pauses as a document");
        }
        // Live-stream heartbeats, the next/browse feeds and the video bytes never wait.
        for url in [
            "https://www.youtube.com/youtubei/v1/player/heartbeat?alt=json",
            "https://www.youtube.com/youtubei/v1/player/ad_break",
            "https://www.youtube.com/youtubei/v1/next?prettyPrint=false",
            "https://www.youtube.com/youtubei/v1/browse?prettyPrint=false",
            "https://www.youtube.com/youtubei/v1/get_watch_later",
            "https://rr1---sn-x.googlevideo.com/videoplayback?itag=137",
            "https://news.example/youtubei/v1/player?x",
        ] {
            assert!(!paused(url, "Fetch"), "{url}");
        }
    }

    #[test]
    fn get_watch_loses_the_ad_schedule_of_its_player_response() {
        let player = player_response();
        let stripped_player = strip_player_ads(&player, PlayerPayload::Json).unwrap();
        let next = r#"{"responseContext":{},"contents":{"twoColumnWatchNextResults":{"adSlots":"next, left alone"}}}"#;
        // The array shape: one element per response, the player response nested.
        let array = format!("[{{\"playerResponse\":{player}}},\n {{\"response\":{next}}}]\n");
        assert_eq!(
            strip_player_ads(&array, PlayerPayload::Json),
            Some(format!("[{{\"playerResponse\":{stripped_player}}},{{\"response\":{next}}}]"))
        );
        // The object shape, with ad members of its own as well.
        let object = format!("{{\"adSlots\":[],\"playerResponse\" : {player},\"response\":{next}}}");
        assert_eq!(
            strip_player_ads(&object, PlayerPayload::Json),
            Some(format!("{{\"playerResponse\" : {stripped_player},\"response\":{next}}}"))
        );
        // A plain player response is unchanged by the nested handling.
        assert_eq!(
            strip_player_ads(&player, PlayerPayload::Json),
            Some(stripped_player.clone())
        );
        // Only one level: a playerResponse inside the player response is not the schedule.
        let deeper = r#"{"x":{"playerResponse":{"adSlots":[]}}}"#;
        assert_eq!(strip_player_ads(deeper, PlayerPayload::Json), None);
        // Nothing to strip anywhere, or a non-object playerResponse: left alone.
        let clean = format!(r#"[{{"playerResponse":{{"playabilityStatus":{{"status":"OK"}},{STREAMING}}}}},{{"response":{next}}}]"#);
        assert_eq!(strip_player_ads(&clean, PlayerPayload::Json), None);
        assert_eq!(strip_player_ads(r#"{"playerResponse":null}"#, PlayerPayload::Json), None);
        // The result is still one well-formed array of the same length.
        let stripped = strip_player_ads(&array, PlayerPayload::Json).unwrap();
        let elements = json::array_elements(&stripped).expect("still one JSON array");
        assert_eq!(elements.len(), 2);
        let first = json::whole_object(elements[0]).unwrap();
        let inner = json::whole_object(json::field(&first, "playerResponse").unwrap()).unwrap();
        for key in PLAYER_AD_KEYS {
            assert_eq!(json::field(&inner, key), None, "{key}");
        }
        assert!(stripped.contains(STREAMING));
    }

    #[test]
    fn the_cosmetic_rules_are_one_rule_per_selector() {
        let rules: Vec<&str> = CSS.split_inclusive('}').collect();
        assert!(rules.len() > 10);
        for rule in &rules {
            let (selector, body) = rule.split_once('{').expect("a rule");
            assert_eq!(body, "display:none!important}", "{rule}");
            assert!(!selector.is_empty() && !selector.contains(',') && !selector.contains('{'), "{rule}");
            assert!(!selector.contains(['"', '\'', '\\', '<']), "{rule}");
            assert_eq!(selector.matches('(').count(), selector.matches(')').count(), "{rule}");
        }
        for needed in ["ytd-ad-slot-renderer{", "#masthead-ad{", "#player-ads{", "ytd-in-feed-ad-layout-renderer{"] {
            assert!(CSS.contains(needed), "{needed}");
        }
    }
}
