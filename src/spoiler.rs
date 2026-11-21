//! Spoiler Firewall for GTA VI, built in and on by default (`extensions/spoiler-firewall`,
//! docs/spoiler-firewall.md).
//!
//! The detection engine is one shared JavaScript bundle. Bowser embeds it, substitutes the user's
//! settings into its single `/*__SFW_CONFIG__*/null` placeholder, and registers the result with
//! `AddScriptToExecuteOnDocumentCreated` on every tab (normal, private, guest, popups), where it
//! runs before page scripts in the top document and in child frames. This module owns the settings
//! model, the settings.txt keys, the config JSON and the script text. The engine call itself is in
//! `engine/webview2.rs`.
//!
//! settings.txt keys (all optional, unknown values never turn protection off):
//!
//! ```text
//! spoiler_protection=on          # off only for exactly off, false or 0
//! spoiler_mode=strict            # lockdown | strict | balanced; anything else means strict
//! spoiler_reduced_effects=false  # opaque shields, no blur or animation
//! spoiler_ocr=off                # legacy key; image scanning retired, always off
//! spoiler_feed=on                # download signed term updates (crate::spoiler_feed; off, false, 0)
//! ```

use std::sync::Mutex;

/// The committed engine bundle (`node extensions/spoiler-firewall/build/build.mjs` regenerates it;
/// `--check` fails when it is stale). Bowser's cargo build needs no Node.
pub const BUNDLE: &str = include_str!("../extensions/spoiler-firewall/dist/bowser/spoiler-firewall.js");

/// The one place in [`BUNDLE`] that receives the config object.
pub const PLACEHOLDER: &str = "/*__SFW_CONFIG__*/null";

pub const KEY_PROTECTION: &str = "spoiler_protection";
pub const KEY_MODE: &str = "spoiler_mode";
pub const KEY_REDUCED_EFFECTS: &str = "spoiler_reduced_effects";
pub const KEY_OCR: &str = "spoiler_ocr";
pub const KEY_FEED: &str = "spoiler_feed";

/// Name shown in the extensions list.
pub const DISPLAY_NAME: &str = "Spoiler Firewall for GTA VI";

/// How eagerly the engine shields. Strict is the default and the fail-closed fallback.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Mode {
    /// Shield anything that touches the franchise or reads like a spoiler; media stays shielded.
    Lockdown,
    #[default]
    Strict,
    /// Shield only clear spoilers; plain mentions pass.
    Balanced,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Self::Lockdown, Self::Strict, Self::Balanced];

    pub fn key(self) -> &'static str {
        match self {
            Self::Lockdown => "lockdown",
            Self::Strict => "strict",
            Self::Balanced => "balanced",
        }
    }

    /// Exact mode names in any letter case. `None` for everything else.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|mode| mode.key().eq_ignore_ascii_case(value.trim()))
    }

    /// One honest line for the settings page and the docs.
    pub fn summary(self) -> &'static str {
        match self {
            Self::Lockdown => "Hides anything that mentions GTA VI and keeps its images and video hidden until you reveal them. Hides the most, including some harmless posts.",
            Self::Strict => "Hides anything that reads like a GTA VI spoiler, and media on sites where spoilers are common. Recommended.",
            Self::Balanced => "Hides clear spoilers only. A plain mention of the game, or a spoiler written in code words, can get through.",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpoilerSettings {
    /// `spoiler_protection`. On unless the value is exactly off, false or 0.
    pub enabled: bool,
    pub mode: Mode,
    pub reduced_effects: bool,
    /// Legacy `spoiler_ocr` field kept for settings compatibility; image inspection is permanently off.
    pub ocr: bool,
    /// `spoiler_feed`. On unless the value is exactly off, false or 0. Controls only the DOWNLOAD of signed term
    /// updates (see `crate::spoiler_feed`); an update already held stays in force either way.
    pub feed: bool,
}

impl Default for SpoilerSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: Mode::Strict,
            reduced_effects: false,
            ocr: false,
            feed: true,
        }
    }
}

/// Protection turns off only for these exact words. Garbage, an empty value or `no` keep it on: a
/// typo must never silently remove a safety feature.
pub fn parse_enabled(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "off" | "false" | "0"
    )
}

fn parse_reduced_effects(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes" | "on"
    )
}

impl SpoilerSettings {
    /// Applies one settings.txt line. Returns whether `key` is one of ours. The last line wins, as
    /// with every other setting; a bad mode value means Strict, never the previous looser mode.
    pub fn apply(&mut self, key: &str, value: &str) -> bool {
        match key {
            KEY_PROTECTION => self.enabled = parse_enabled(value),
            KEY_MODE => self.mode = Mode::parse(value).unwrap_or(Mode::Strict),
            KEY_REDUCED_EFFECTS => self.reduced_effects = parse_reduced_effects(value),
            KEY_OCR => self.ocr = false, // ignore old profiles that enabled image inspection
            KEY_FEED => self.feed = parse_enabled(value),
            _ => return false,
        }
        true
    }

    /// The `(key, value)` lines `settings::render_updated` writes.
    pub fn lines(&self) -> [(&'static str, String); 5] {
        [
            (KEY_PROTECTION, if self.enabled { "on" } else { "off" }.to_owned()),
            (KEY_MODE, self.mode.key().to_owned()),
            (KEY_REDUCED_EFFECTS, if self.reduced_effects { "true" } else { "false" }.to_owned()),
            (KEY_OCR, "off".to_owned()), // migrate old profiles to text-only filtering
            (KEY_FEED, if self.feed { "on" } else { "off" }.to_owned()),
        ]
    }
}

/// `"<name>"` with JSON escapes. U+2028 and U+2029 are escaped too, so the text is also valid
/// JavaScript on engines that predate JSON-superset parsing.
pub(crate) fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The bundle's version, from its header comment (`/* Spoiler Firewall for GTA VI 1.0.0 - ...`).
pub fn bundle_version(bundle: &str) -> &str {
    const MARK: &str = "Spoiler Firewall for GTA VI ";
    bundle
        .lines()
        .next()
        .and_then(|line| line.split_once(MARK))
        .and_then(|(_, rest)| rest.split_whitespace().next())
        .filter(|version| {
            !version.is_empty()
                && version.len() <= 32
                && version.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '+')
        })
        .unwrap_or("unknown")
}

/// Per-tab secrets for the native OCR pull (docs/spoiler-firewall.md). `hook` names the one
/// non-enumerable `window` property the bundle defines; `key` must accompany every call to it, so a
/// page that lists its own property names still cannot drain requests or forge results.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OcrCredentials {
    pub hook: String,
    pub key: String,
}

impl OcrCredentials {
    /// 128 random bits each, from the system RNG. `None` when the RNG fails: OCR is then off.
    pub fn generate() -> Option<Self> {
        use windows::Win32::Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom};
        let mut bytes = [0u8; 32];
        if unsafe { BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }.0 < 0 {
            return None;
        }
        let hex = |chunk: &[u8]| chunk.iter().map(|b| format!("{b:02x}")).collect::<String>();
        Some(Self {
            hook: format!("__sfw_{}", hex(&bytes[..16])),
            key: hex(&bytes[16..]),
        })
    }

    /// Only the shape the native side generates is ever accepted into a script.
    pub fn is_well_formed(&self) -> bool {
        self.hook.len() == 6 + 32
            && self.hook.starts_with("__sfw_")
            && self.hook[6..].bytes().all(|b| b.is_ascii_hexdigit())
            && self.key.len() == 32
            && self.key.bytes().all(|b| b.is_ascii_hexdigit())
    }
}

/// The config object the engine reads: `{"enabled":true,"mode":"strict","reducedEffects":false,
/// "version":"1.0.0","private":false,"ocr":false}`; with credentials the last members become
/// `"ocr":true,"ocrHook":"__sfw_<hex>","ocrKey":"<hex>"`. OCR is on only when the setting is on, protection
/// is on and well-formed credentials are given.
pub fn config_json(
    settings: &SpoilerSettings,
    version: &str,
    private: bool,
    ocr: Option<&OcrCredentials>,
) -> String {
    config_json_with_feed(settings, version, private, ocr, None)
}

/// [`config_json`] plus, when a verified term update is held, a last `"feed":<lexicon object>` member: the
/// `lexicon` of the signed feed (`crate::spoiler_feed`), which `bowser-host.js` hands to
/// `SFW.lexicon.extend` before the engine starts. `feed_lexicon` is the already-validated, re-serialised
/// object (U+2028/U+2029 escaped by `json_string`); anything that is not a JSON object is dropped, so a bad
/// value can never break the script.
pub fn config_json_with_feed(
    settings: &SpoilerSettings,
    version: &str,
    private: bool,
    ocr: Option<&OcrCredentials>,
    feed_lexicon: Option<&str>,
) -> String {
    let mut json = format!(
        "{{\"enabled\":{},\"mode\":{},\"reducedEffects\":{},\"version\":{},\"private\":{}",
        settings.enabled,
        json_string(settings.mode.key()),
        settings.reduced_effects,
        json_string(version),
        private,
    );
    // The hook (a per-tab closure) exists whenever credentials do, even with OCR off: Bowser also uses it
    // to push a new term update into pages already open. "ocr" says whether image requests are served.
    match ocr.filter(|c| settings.enabled && c.is_well_formed()) {
        Some(c) => json.push_str(&format!(
            ",\"ocr\":{},\"ocrHook\":{},\"ocrKey\":{}}}",
            false, // retain the hook for signed term updates, but never allow image scanning
            json_string(&c.hook),
            json_string(&c.key)
        )),
        None => json.push_str(",\"ocr\":false}"),
    }
    if let Some(lexicon) = feed_lexicon.filter(|text| is_embeddable_object(text)) {
        json.pop(); // the closing brace
        json.push_str(",\"feed\":");
        json.push_str(lexicon);
        json.push('}');
    }
    json
}

/// A `{...}` text with no raw line separators or control characters and a balanced structure outside strings:
/// a cheap last line of defence, since the object is pasted into script text. (`spoiler_feed` already
/// produced it with `json_string`, so this only fails on a bug.)
pub(crate) fn is_embeddable_object(text: &str) -> bool {
    if !(text.starts_with('{') && text.ends_with('}')) || text.len() > 262_144 {
        return false;
    }
    let (mut depth, mut in_string, mut escaped) = (0i32, false, false);
    for c in text.chars() {
        if matches!(c, '\u{2028}' | '\u{2029}') || (c as u32) < 0x20 {
            return false;
        }
        if in_string {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
        } else {
            match c {
                '"' => in_string = true,
                '{' | '[' => depth += 1,
                '}' | ']' => {
                    depth -= 1;
                    if depth < 0 {
                        return false;
                    }
                }
                _ => {}
            }
        }
    }
    depth == 0 && !in_string
}

/// `bundle` with its placeholder replaced by the config. A bundle without exactly one placeholder
/// is a build error (a unit test fails); at run time it is injected unmodified, and the engine falls
/// back to its own defaults (enabled, Strict), which is the safe direction.
pub fn build_script(
    bundle: &str,
    settings: &SpoilerSettings,
    private: bool,
    ocr: Option<&OcrCredentials>,
) -> String {
    build_script_with_feed(bundle, settings, private, ocr, None)
}

/// [`build_script`] with a verified term update embedded (see [`config_json_with_feed`]).
pub fn build_script_with_feed(
    bundle: &str,
    settings: &SpoilerSettings,
    private: bool,
    ocr: Option<&OcrCredentials>,
    feed_lexicon: Option<&str>,
) -> String {
    if bundle.matches(PLACEHOLDER).count() != 1 {
        return bundle.to_owned();
    }
    bundle.replacen(
        PLACEHOLDER,
        &config_json_with_feed(settings, bundle_version(bundle), private, ocr, feed_lexicon),
        1,
    )
}

/// The bundle that is injected: [`BUNDLE`]. Debug builds (never release) can swap in another file
/// with `BOWSER_SPOILER_BUNDLE=<path>`, for live testing a bundle without rebuilding Bowser.
fn active_bundle() -> std::borrow::Cow<'static, str> {
    #[cfg(debug_assertions)]
    if let Some(text) = std::env::var_os("BOWSER_SPOILER_BUNDLE")
        .and_then(|path| std::fs::read_to_string(path).ok())
        .filter(|text| text.len() <= 8 * 1024 * 1024)
    {
        return std::borrow::Cow::Owned(text);
    }
    std::borrow::Cow::Borrowed(BUNDLE)
}

/// The script to register for a tab, or `None` when protection is off (the native kill switch).
pub fn script_for(
    settings: &SpoilerSettings,
    private: bool,
    ocr: Option<&OcrCredentials>,
) -> Option<String> {
    let feed = crate::spoiler_feed::current_lexicon_json();
    settings
        .enabled
        .then(|| build_script_with_feed(&active_bundle(), settings, private, ocr, feed.as_deref()))
}

// ---- the live setting, shared by every tab ----

static CURRENT: Mutex<Option<SpoilerSettings>> = Mutex::new(None);

/// The setting new documents are protected by. Defaults (on, Strict) until `publish` is first
/// called, so a tab created before settings load is protected.
pub fn current() -> SpoilerSettings {
    CURRENT
        .lock()
        .ok()
        .and_then(|guard| *guard)
        .unwrap_or_default()
}

/// Records the saved setting. Returns whether it differs from what was in effect.
pub fn publish(settings: SpoilerSettings) -> bool {
    let Ok(mut guard) = CURRENT.lock() else {
        return false;
    };
    let previous = guard.unwrap_or_default();
    *guard = Some(settings);
    previous != settings
}

/// The host of an address in any display form ("https://a.b/c", "a.b/c", "www.a.b"), lower-cased, or
/// `None` when no host-like text is found.
pub fn host_of(address: &str) -> Option<String> {
    let address = address.trim();
    if address.is_empty() || address.contains(char::is_whitespace) {
        return None;
    }
    let candidate = if address.contains("://") {
        address.to_owned()
    } else {
        format!("https://{address}")
    };
    let url = url::Url::parse(&candidate).ok()?;
    let host = url.host_str()?.trim_end_matches('.').to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Whether a title is only an address: empty, equal to the page URL, or URL-shaped ("https://...",
/// "www.x.y/path", "host.tld/path"). Such a title carries the slug the engine would have hidden.
pub fn title_is_address(title: &str, url: Option<&str>) -> bool {
    let title = title.trim();
    if title.is_empty() {
        return true;
    }
    if let Some(url) = url.map(str::trim).filter(|url| !url.is_empty()) {
        let strip = |text: &str| {
            text.trim_end_matches('/')
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .to_ascii_lowercase()
        };
        if strip(title) == strip(url) {
            return true;
        }
    }
    if title.contains(char::is_whitespace) {
        return false;
    }
    let lower = title.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.") {
        return true;
    }
    // "host.tld/path": no spaces, a dot in the part before the first slash, and a slash.
    match title.split_once('/') {
        Some((head, _)) => head.contains('.') && !head.starts_with('.') && !head.ends_with('.'),
        None => false,
    }
}

/// The title Bowser shows (tab strip, caption, taskbar, history) while protection is on: the page's own
/// title (already rewritten by the engine when risky), except that an empty, URL-equal or URL-shaped title
/// becomes the host only, so a URL slug never reaches those surfaces. With protection off the title is
/// returned unchanged.
pub fn protected_title(enabled: bool, title: &str, url: Option<&str>) -> String {
    if !enabled || !title_is_address(title, url) {
        return title.to_owned();
    }
    url.and_then(host_of)
        .or_else(|| host_of(title))
        .unwrap_or_default()
}

/// How many times a failed `AddScriptToExecuteOnDocumentCreated` is retried before the tab is put in
/// the protective state.
pub const INJECTION_RETRIES: u8 = 1;

/// What to do when the asynchronous script registration of a tab completes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InjectionStep {
    /// Registered: the tab is protected.
    Registered,
    /// Failed with attempts left: register again.
    Retry,
    /// Failed for good: stop the page and show the protective state. Never render unprotected.
    Protect,
}

/// The state machine of one registration: `attempt` is 0 for the first try.
pub fn injection_step(attempt: u8, succeeded: bool) -> InjectionStep {
    match (succeeded, attempt < INJECTION_RETRIES) {
        (true, _) => InjectionStep::Registered,
        (false, true) => InjectionStep::Retry,
        (false, false) => InjectionStep::Protect,
    }
}

/// Where a tab goes next when a navigation starts while it is in the protective state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtectiveNav {
    /// The protective page itself, loaded by Bowser (once): let it through.
    OwnPage,
    /// Anything else: cancel it, register the script again and resume at this address afterwards.
    RetryAndResume(Option<String>),
}

/// Decides a navigation while the tab is in the protective state. `own_page_pending` is the one-shot flag
/// Bowser sets right before loading its own page; `target` is the requested address; `last` the address
/// the tab was loading when protection failed (used when the protective page itself is reloaded).
pub fn protective_navigation(
    own_page_pending: bool,
    target: &str,
    last: Option<&str>,
) -> ProtectiveNav {
    if own_page_pending {
        return ProtectiveNav::OwnPage;
    }
    let web = target.starts_with("http://") || target.starts_with("https://");
    ProtectiveNav::RetryAndResume(if web {
        Some(target.to_owned())
    } else {
        last.map(str::to_owned)
    })
}

fn html_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// The Bowser-owned page shown in place of a page that could not be protected. It shows the host only
/// (never a path) and a Reload link to the address the tab was loading (http(s) only), which retries the
/// protection before loading anything. No script.
pub fn protective_page_html(address: Option<&str>) -> String {
    let host = address.and_then(host_of);
    let link = address
        .map(str::trim)
        .filter(|a| a.starts_with("http://") || a.starts_with("https://"))
        .map(html_escape);
    let site = host
        .map(|h| format!("<p class=\"site\">{}</p>", html_escape(&h)))
        .unwrap_or_default();
    let reload = link
        .map(|href| format!("<a class=\"reload\" href=\"{href}\">Reload</a>"))
        .unwrap_or_else(|| "<span class=\"hint\">Reload the tab to try again.</span>".to_owned());
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Spoiler protection</title>\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<style>html{{color-scheme:light dark}}body{{margin:0;min-height:100vh;display:grid;place-items:center;\
font:15px/1.5 system-ui,Segoe UI,sans-serif;background:Canvas;color:CanvasText}}\
main{{max-width:30rem;padding:2rem}}h1{{font-size:1.15rem;margin:0 0 .5rem}}.site{{opacity:.7;margin:.25rem 0 1rem}}\
.reload{{display:inline-block;padding:.45rem 1rem;border-radius:.5rem;border:1px solid currentColor;color:inherit;text-decoration:none}}\
.hint{{opacity:.7}}</style></head><body><main><h1>Spoiler protection could not start in this tab</h1>\
{site}<p>This page was not shown, so nothing unprotected is on screen.</p>{reload}</main></body></html>"
    )
}

/// Page titles are held this long before they reach the tab strip and history, so the engine's
/// rewritten title ("Spoiler-protected page") is the one that shows.
pub const TITLE_HOLD_MS: u32 = 150;

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> SpoilerSettings {
        let mut settings = SpoilerSettings::default();
        for line in text.lines() {
            if let Some((key, value)) = line.split_once('=') {
                settings.apply(key.trim(), value.trim());
            }
        }
        settings
    }

    #[test]
    fn first_launch_is_enabled_and_strict() {
        let defaults = SpoilerSettings::default();
        assert!(defaults.enabled);
        assert_eq!(defaults.mode, Mode::Strict);
        assert!(!defaults.reduced_effects);
        assert_eq!(parse(""), defaults);
        assert_eq!(parse("# note\nsleep_enabled=false\n"), defaults, "missing keys give defaults");
    }

    #[test]
    fn only_exact_off_words_disable_protection() {
        for off in ["off", "OFF", " false ", "False", "0"] {
            assert!(!parse(&format!("spoiler_protection={off}")).enabled, "{off}");
        }
        for on in ["on", "true", "1", "yes", "", "no", "disabled", "of", "offf", "0.0", "garbage", "\u{1F4A5}"] {
            assert!(parse(&format!("spoiler_protection={on}")).enabled, "{on:?}");
        }
        assert!(parse("spoiler_protection=off\nspoiler_protection=maybe").enabled, "last line wins");
    }

    #[test]
    fn garbage_mode_is_strict_never_looser() {
        assert_eq!(parse("spoiler_mode=LOCKDOWN").mode, Mode::Lockdown);
        assert_eq!(parse("spoiler_mode=balanced").mode, Mode::Balanced);
        for bad in ["", "off", "none", "relaxed", "stric", "balanced!"] {
            assert_eq!(parse(&format!("spoiler_mode={bad}")).mode, Mode::Strict, "{bad:?}");
        }
        assert_eq!(parse("spoiler_mode=balanced\nspoiler_mode=nonsense").mode, Mode::Strict);
        assert!(parse("spoiler_mode=nonsense").enabled, "a bad mode does not switch protection off");
    }

    #[test]
    fn reduced_effects_reads_booleans_and_defaults_off() {
        assert!(parse("spoiler_reduced_effects=true").reduced_effects);
        assert!(parse("spoiler_reduced_effects=ON").reduced_effects);
        assert!(!parse("spoiler_reduced_effects=false").reduced_effects);
        assert!(!parse("spoiler_reduced_effects=maybe").reduced_effects);
    }

    #[test]
    fn unknown_keys_are_not_ours() {
        let mut settings = SpoilerSettings::default();
        assert!(!settings.apply("spoiler_other", "x"));
        assert!(!settings.apply("tab_layout", "vertical"));
        assert!(settings.apply(KEY_MODE, "balanced"));
    }

    #[test]
    fn lines_round_trip_through_apply() {
        for enabled in [true, false] {
            for mode in Mode::ALL {
                for reduced_effects in [true, false] {
                    for ocr in [false] {
                        for feed in [true, false] {
                            let settings = SpoilerSettings { enabled, mode, reduced_effects, ocr, feed };
                            let text = settings
                                .lines()
                                .iter()
                                .map(|(key, value)| format!("{key}={value}\n"))
                                .collect::<String>();
                            assert_eq!(parse(&text), settings, "{text}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn image_scanning_stays_off_even_for_legacy_settings() {
        assert!(!SpoilerSettings::default().ocr);
        for old in ["on", "true", "1", "off", "false", "garbage", ""] {
            let settings = parse(&format!("spoiler_ocr={old}"));
            assert!(!settings.ocr, "{old:?}");
            assert!(settings.enabled, "retiring image scanning must not disable the firewall");
            let json = config_json(&settings, "1.0.0", false, Some(&creds()));
            assert!(json.contains(r#""ocr":false,"ocrHook":"#), "{json}");
        }
        let legacy = SpoilerSettings { ocr: true, ..SpoilerSettings::default() };
        assert!(legacy.lines().iter().any(|(k, v)| *k == KEY_OCR && v == "off"));
        assert!(config_json(&legacy, "1.0.0", false, Some(&creds())).contains(r#""ocr":false,"ocrHook":"#));
    }

    #[test]
    fn term_updates_default_on_and_only_exact_off_words_disable_the_download() {
        assert!(SpoilerSettings::default().feed);
        for off in ["off", "OFF", " false ", "0"] {
            assert!(!parse(&format!("spoiler_feed={off}")).feed, "{off}");
        }
        for on in ["on", "true", "", "no", "garbage", "offf"] {
            assert!(parse(&format!("spoiler_feed={on}")).feed, "{on:?}");
        }
        let off = parse("spoiler_feed=off");
        assert!(off.enabled && !off.ocr, "the term-update switch changes nothing else");
        assert!(SpoilerSettings::default().lines().iter().any(|(k, v)| *k == KEY_FEED && v == "on"));
    }

    const LEXICON: &str = r#"{"version":"f3","add":{"entityStrong":["harbor master sloane"],"cueStrong":["tab\there\u2028"]},"raise":{"pelican bay ferry":"cueStrong"}}"#;

    #[test]
    fn a_verified_term_update_is_embedded_as_the_last_config_member() {
        let s = SpoilerSettings::default();
        assert_eq!(
            config_json_with_feed(&s, "1.0.0", false, None, Some(LEXICON)),
            format!(r#"{{"enabled":true,"mode":"strict","reducedEffects":false,"version":"1.0.0","private":false,"ocr":false,"feed":{LEXICON}}}"#)
        );
        // after the OCR credentials too
        let json = config_json_with_feed(&s, "1.0.0", false, Some(&creds()), Some(LEXICON));
        assert!(json.contains(r#""ocrKey":""#) && json.ends_with(&format!(r#","feed":{LEXICON}}}"#)));
        // no feed, or an unusable one, leaves the documented shape exactly as before
        let plain = config_json(&s, "1.0.0", false, None);
        assert_eq!(config_json_with_feed(&s, "1.0.0", false, None, None), plain);
        for bad in ["", "null", "[]", "{", "}{", "{\"a\":\"unterminated}", "{\"a\":1}]", "{\"a\":\"\u{2028}\"}", "{\"a\":\"\n\"}", "\"text\"", "{\"a\":\"x\"}</script>{"] {
            assert_eq!(config_json_with_feed(&s, "1.0.0", false, None, Some(bad)), plain, "{bad:?}");
        }
        assert_eq!(config_json_with_feed(&s, "1.0.0", false, None, Some(&"{".repeat(300_000))), plain, "oversize");
    }

    #[test]
    fn the_embedded_term_update_reaches_the_script_once_with_no_line_separators() {
        let bundle = format!("/* Spoiler Firewall for GTA VI 9.9.9 */\nvar c = {PLACEHOLDER};\nrun(c);");
        let script = build_script_with_feed(&bundle, &SpoilerSettings::default(), false, None, Some(LEXICON));
        assert_eq!(script.matches(LEXICON).count(), 1);
        assert!(!script.contains(PLACEHOLDER));
        assert!(script.contains(r#""ocr":false,"feed":{"version":"f3""#));
        assert!(!script.contains('\u{2028}') && !script.contains('\u{2029}'));
        assert!(script.ends_with("run(c);"));
        assert_eq!(
            build_script_with_feed(&bundle, &SpoilerSettings::default(), false, None, None),
            build_script(&bundle, &SpoilerSettings::default(), false, None)
        );
    }

    #[test]
    fn the_host_script_hands_the_feed_to_the_engine_before_it_starts() {
        // bowser-host.js (committed bundle): extend() is called with config.feed, before runtime.start.
        let extend = BUNDLE.find("SFW.lexicon.extend(config.feed)").expect("bowser-host.js calls extend(config.feed)");
        let start = BUNDLE.find("SFW.runtime.start({").expect("runtime start");
        assert!(extend < start, "the feed must be applied before the engine starts");
    }

    #[test]
    fn the_host_script_takes_live_term_updates_through_the_per_tab_hook() {
        // The hook object carries feed(key, json); it applies extend() and then the runtime's config-change path, which
        // rescans when the lexicon version changed. Without the hook, a feed accepted while a page is open would wait for a reload.
        assert!(BUNDLE.contains("feed: feed"), "the hook exposes feed");
        assert!(BUNDLE.contains("onConfigChange: function (cb) { configCallback = cb; }"), "the runtime's config-change hook is kept");
        assert!(BUNDLE.contains("configCallback(config)"), "a pushed feed triggers the rescan path");
        assert!(BUNDLE.contains("appliedFeeds"), "the same feed is never applied twice");
    }

    #[test]
    fn the_hook_exists_with_ocr_off_so_feeds_can_still_be_pushed() {
        let off = SpoilerSettings { ocr: false, ..SpoilerSettings::default() };
        let json = config_json(&off, "1.0.0", false, Some(&creds()));
        assert!(json.contains(r#""ocr":false,"ocrHook":"__sfw_"#), "{json}");
        assert!(json.contains("ocrKey"));
    }

    fn creds() -> OcrCredentials {
        OcrCredentials {
            hook: format!("__sfw_{}", "ab".repeat(16)),
            key: "cd".repeat(16),
        }
    }

    #[test]
    fn generated_credentials_are_well_formed_and_unique() {
        let a = OcrCredentials::generate().unwrap();
        let b = OcrCredentials::generate().unwrap();
        assert!(a.is_well_formed() && b.is_well_formed());
        assert_ne!(a.hook, b.hook);
        assert_ne!(a.key, b.key);
        assert_ne!(a.hook[6..], a.key, "hook and key are independent");
        let bad = OcrCredentials { hook: "__sfw_x\"};alert(1)//".into(), key: creds().key };
        assert!(!bad.is_well_formed());
    }

    #[test]
    fn text_only_config_keeps_term_update_credentials_without_enabling_image_scanning() {
        let on = SpoilerSettings::default();
        let json = config_json(&on, "1.0.0", false, Some(&creds()));
        assert_eq!(
            json,
            format!(
                r#"{{"enabled":true,"mode":"strict","reducedEffects":false,"version":"1.0.0","private":false,"ocr":false,"ocrHook":"{}","ocrKey":"{}"}}"#,
                creds().hook,
                creds().key
            )
        );
        let off = SpoilerSettings { ocr: false, ..on };
        let off_json = config_json(&off, "1.0.0", false, Some(&creds()));
        assert!(off_json.contains(r#""ocr":false,"ocrHook":"#));
        let disabled = SpoilerSettings { enabled: false, ..on };
        assert!(!config_json(&disabled, "1.0.0", false, Some(&creds())).contains("ocrHook"));
        let malformed = OcrCredentials { hook: "evil".into(), key: "k".into() };
        assert!(config_json(&on, "1.0.0", false, Some(&malformed)).ends_with(r#""ocr":false}"#));
    }

    #[test]
    fn config_json_has_exactly_the_documented_shape() {
        let json = config_json(&SpoilerSettings::default(), "1.0.0", false, None);
        assert_eq!(
            json,
            r#"{"enabled":true,"mode":"strict","reducedEffects":false,"version":"1.0.0","private":false,"ocr":false}"#
        );
        let json = config_json(
            &SpoilerSettings { enabled: true, mode: Mode::Lockdown, reduced_effects: true, ocr: true, feed: true },
            "2.1.0-rc.1",
            true,
            None,
        );
        assert_eq!(
            json,
            r#"{"enabled":true,"mode":"lockdown","reducedEffects":true,"version":"2.1.0-rc.1","private":true,"ocr":false}"#
        );
    }

    #[test]
    fn json_strings_are_escaped_for_script_text() {
        let escaped = json_string("a\"b\\c\n\u{2028}\u{2029}\u{1}</script>");
        assert_eq!(escaped, "\"a\\\"b\\\\c\\n\\u2028\\u2029\\u0001</script>\"");
        assert!(!escaped.contains('\u{2028}') && !escaped.contains('\u{2029}'));
    }

    #[test]
    fn version_comes_from_the_header_comment() {
        assert_eq!(
            bundle_version("/* Spoiler Firewall for GTA VI 1.2.3 - unofficial */\n(function(){})()"),
            "1.2.3"
        );
        assert_eq!(bundle_version("(function(){})()"), "unknown");
        assert_eq!(bundle_version("/* Spoiler Firewall for GTA VI \"};alert(1) */"), "unknown");
    }

    #[test]
    fn substitution_replaces_only_the_placeholder() {
        let bundle = format!("/* Spoiler Firewall for GTA VI 9.9.9 */\nvar c = {PLACEHOLDER};\nrun(c);");
        let script = build_script(&bundle, &SpoilerSettings::default(), true, None);
        assert!(!script.contains(PLACEHOLDER));
        assert!(script.contains(r#"var c = {"enabled":true,"mode":"strict","reducedEffects":false,"version":"9.9.9","private":true,"ocr":false};"#));
        assert!(script.starts_with("/* Spoiler Firewall for GTA VI 9.9.9 */"));
        assert!(script.ends_with("run(c);"));
    }

    #[test]
    fn a_bundle_without_exactly_one_placeholder_is_injected_unmodified() {
        let none = "(function(){ /* no placeholder */ })();";
        assert_eq!(build_script(none, &SpoilerSettings::default(), false, None), none);
        let two = format!("{PLACEHOLDER};{PLACEHOLDER}");
        assert_eq!(build_script(&two, &SpoilerSettings::default(), false, None), two);
    }

    #[test]
    fn nothing_is_injected_when_protection_is_off() {
        let off = SpoilerSettings { enabled: false, ..SpoilerSettings::default() };
        assert!(script_for(&off, false, None).is_none());
        assert!(script_for(&SpoilerSettings::default(), false, None).is_some());
    }

    #[test]
    fn publishing_reports_changes_and_current_follows() {
        // The only test that publishes, so it cannot race another over the process-wide value.
        let looser = SpoilerSettings { mode: Mode::Balanced, ..SpoilerSettings::default() };
        assert_eq!(current(), SpoilerSettings::default(), "before anything is published");
        publish(SpoilerSettings::default());
        assert!(!publish(SpoilerSettings::default()));
        assert!(publish(looser));
        assert_eq!(current(), looser);
        assert!(publish(SpoilerSettings::default()));
        assert_eq!(current(), SpoilerSettings::default());
    }

    // ---- the embedded bundle (no Node is run) ----

    #[test]
    fn the_embedded_bundle_is_present() {
        assert!(BUNDLE.len() > 1000, "bundle is {} bytes", BUNDLE.len());
        assert!(BUNDLE.contains("Spoiler Firewall for GTA VI"));
        assert_ne!(bundle_version(BUNDLE), "unknown", "the header comment names a version");
    }

    #[test]
    fn the_embedded_bundle_has_exactly_one_config_placeholder() {
        assert_eq!(
            BUNDLE.matches(PLACEHOLDER).count(),
            1,
            "run `node extensions/spoiler-firewall/build/build.mjs` and commit dist/bowser/spoiler-firewall.js"
        );
    }

    #[test]
    fn the_embedded_bundle_substitutes_to_a_script_with_the_config() {
        if BUNDLE.matches(PLACEHOLDER).count() != 1 {
            return; // reported by the placeholder test above
        }
        for private in [false, true] {
            let script = script_for(&SpoilerSettings::default(), private, None).unwrap();
            assert!(!script.contains(PLACEHOLDER));
            let json = config_json(&SpoilerSettings::default(), bundle_version(BUNDLE), private, None);
            assert!(script.contains(&json));
            assert_eq!(script.len(), BUNDLE.len() - PLACEHOLDER.len() + json.len());
            assert!(!script.contains('\u{2028}') || BUNDLE.contains('\u{2028}'));
        }
    }

    #[test]
    fn address_shaped_titles_become_the_host_only_while_protected() {
        let url = Some("https://www.example.com/gta6-ending-explained");
        assert_eq!(protected_title(true, "", url), "www.example.com");
        assert_eq!(protected_title(true, "https://www.example.com/gta6-ending-explained", url), "www.example.com");
        assert_eq!(protected_title(true, "www.example.com/gta6-ending-explained", url), "www.example.com");
        assert_eq!(protected_title(true, "example.org/some/slug", url), "www.example.com");
        assert_eq!(protected_title(true, "Spoiler-protected page", url), "Spoiler-protected page");
        assert_eq!(protected_title(true, "Weather today", url), "Weather today");
        assert_eq!(protected_title(false, "https://x.test/a", Some("https://x.test/a")), "https://x.test/a");
        assert_eq!(protected_title(true, "https://a.test/path", None), "a.test");
        assert_eq!(protected_title(true, "v1.2 released", None), "v1.2 released");
    }

    #[test]
    fn injection_state_machine_retries_once_then_protects() {
        assert_eq!(injection_step(0, true), InjectionStep::Registered);
        assert_eq!(injection_step(0, false), InjectionStep::Retry);
        assert_eq!(injection_step(1, true), InjectionStep::Registered);
        assert_eq!(injection_step(1, false), InjectionStep::Protect);
        assert_eq!(injection_step(9, false), InjectionStep::Protect);
    }

    #[test]
    fn protective_state_only_lets_its_own_page_through() {
        assert_eq!(protective_navigation(true, "data:text/html;base64,AAA", None), ProtectiveNav::OwnPage);
        assert_eq!(
            protective_navigation(false, "https://a.test/x", Some("https://b.test/")),
            ProtectiveNav::RetryAndResume(Some("https://a.test/x".into()))
        );
        assert_eq!(
            protective_navigation(false, "data:text/html;base64,AAA", Some("https://b.test/")),
            ProtectiveNav::RetryAndResume(Some("https://b.test/".into()))
        );
        assert_eq!(protective_navigation(false, "data:x", None), ProtectiveNav::RetryAndResume(None));
    }

    #[test]
    fn protective_page_shows_host_only_escapes_and_has_no_script() {
        let page = protective_page_html(Some("https://Example.com/a/b?q=\"><script>x</script>"));
        assert!(page.contains("Spoiler protection could not start in this tab"));
        assert!(page.contains("<p class=\"site\">example.com</p>"));
        assert!(!page.contains("<script"));
        assert!(page.contains("&quot;&gt;&lt;script&gt;"));
        assert!(!protective_page_html(None).contains("href="));
        assert!(!protective_page_html(Some("javascript:alert(1)")).contains("href="));
    }
}
