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
pub const RESPONSE_PATTERNS: [ResponsePattern; 3] = [
    ResponsePattern {
        url_pattern: "*://*.youtube.com/youtubei/v1/player\\?*",
        resource_type: None,
    },
    ResponsePattern {
        url_pattern: "*://*.youtube-nocookie.com/youtubei/v1/player\\?*",
        resource_type: None,
    },
    ResponsePattern {
        url_pattern: "*://*.youtube.com/watch\\?*",
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

/// Which player response `url` returns, if it is one this module rewrites. Only the exact paths
/// count: `/youtubei/v1/player/heartbeat` (live streams) and `/youtubei/v1/next` are not.
pub fn player_payload(url: &str) -> Option<PlayerPayload> {
    let (host, path) = super::split_url(url)?;
    let site = site_host(&host, &["youtube.com", "youtube-nocookie.com"])?;
    match path.split('?').next().unwrap_or(path) {
        "/youtubei/v1/player" => Some(PlayerPayload::Json),
        "/watch" if site == "youtube.com" => Some(PlayerPayload::WatchPage),
        _ => None,
    }
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
        let Some((object, end)) = without_ad_members(html, open) else {
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
    fn response_patterns_escape_the_query_mark() {
        for pattern in &RESPONSE_PATTERNS {
            assert!(pattern.url_pattern.contains("\\?*"), "{pattern:?}");
        }
    }
}
