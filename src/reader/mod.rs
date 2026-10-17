//! Reader view and distraction hiding: the pure decisions around an on-demand page script.
//!
//! Nothing here runs in a page until the user asks (Ctrl+Alt+R, the main menu, the command palette or
//! the toolbar button). The one automatic step is a read-only probe once the active tab has finished
//! loading, which decides whether the toolbar button is shown. No script is ever injected at document
//! creation, so pages that are not being read pay nothing.
//!
//! The script (`reader.js`) restyles the live document in place and reverses cleanly, so the address
//! and history are untouched. It makes no request, and the only thing it returns is the fixed-shape
//! string `r2|code|state|words|link|theme|size|lang`, which [`parse_result`] validates strictly: anything
//! else (including anything a hostile page could have put there) is discarded. See
//! `docs/reader.md`.

use url::Url;

use crate::chrome_host::{css_colour, protocol::{is_safe_css_colour, json_string}};
use crate::theme::Palette;

/// The extraction, restyle and cleanup script: a function expression taking one options object.
pub const SCRIPT: &str = include_str!("reader.js");
/// Reader styles: a common part, the marker `/*R*/`, then the reader-only part.
const CSS: &str = include_str!("reader.css");
/// Styles of the in-page control bar (inside a closed shadow root).
const BAR_CSS: &str = include_str!("bar.css");

/// The toolbar button appears when the page's best content block has at least this many words...
pub const MIN_ARTICLE_WORDS: u32 = 250;
/// ... and no more than this share of its text is link text (so link lists and search results do
/// not qualify).
pub const MAX_LINK_PERCENT: u32 = 35;
/// Longest result string accepted back from a page.
const MAX_RESULT_LEN: usize = 56;

pub const MIN_SIZE: u8 = 14;
pub const MAX_SIZE: u8 = 30;
pub const DEFAULT_SIZE: u8 = 19;

/// What the page is asked to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Restyle the article in a clean typographic layout and hide everything else.
    Reader,
    /// Only hide fixed or sticky overlays, cookie banners and dialogs, and free scrolling.
    Calm,
}

impl Mode {
    fn key(self) -> &'static str {
        match self {
            Self::Reader => "reader",
            Self::Calm => "calm",
        }
    }
}

/// What the page is currently showing, as it reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Active {
    #[default]
    Off,
    Reader,
    Calm,
}

impl Active {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "off" => Some(Self::Off),
            "reader" => Some(Self::Reader),
            "calm" => Some(Self::Calm),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    /// Follows Bowser's own theme.
    #[default]
    Auto,
    Light,
    Sepia,
    Dark,
}

impl Theme {
    fn key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Light => "light",
            Self::Sepia => "sepia",
            Self::Dark => "dark",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "auto" => Some(Self::Auto),
            "light" => Some(Self::Light),
            "sepia" => Some(Self::Sepia),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }
}

/// The reader controls' last settings, kept for the session and passed back in on the next use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prefs {
    pub theme: Theme,
    pub size: u8,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { theme: Theme::Auto, size: DEFAULT_SIZE }
    }
}

/// How reader view sets its text, from Settings > Page tools. These are defaults for every page, not
/// per-session choices: the colour scheme and size controls on the page (`Prefs`) adjust for the
/// session, and the next page starts from here again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Typography {
    pub font: crate::page_tools::ReaderFont,
    /// Line height in tenths of the font size (16 is 1.6).
    pub line_tenths: u8,
    /// Column width in `ch`.
    pub width_ch: u8,
    /// Text runs top to bottom, lines from right to left (as in traditional Chinese and Japanese).
    pub vertical: bool,
    /// The control bar (the settings icon) is on the page. Off, only the reading time may be.
    pub controls: bool,
    /// How long the page takes to read, in the bar.
    pub reading_time: bool,
}

impl Default for Typography {
    fn default() -> Self {
        Self { font: crate::page_tools::ReaderFont::Serif, line_tenths: 16, width_ch: 68, vertical: false, controls: true, reading_time: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Read-only: how article-like is this page, or what state is it in?
    Probe,
    /// Enter `Mode`, or leave if the page is already in it.
    Toggle(Mode),
    /// Leave whatever mode the page is in. Safe to run on a page that is not in one.
    Exit,
}

impl Action {
    fn key(self) -> &'static str {
        match self {
            Self::Probe => "probe",
            Self::Toggle(_) => "toggle",
            Self::Exit => "exit",
        }
    }
}

/// The Bowser colours the reader borrows, as CSS colour strings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Colours {
    pub bg: String,
    pub fg: String,
    pub accent: String,
    pub border: String,
    pub fill: String,
}

impl Colours {
    /// From a solid (opaque) palette, so the page never shows the window backdrop through it.
    pub fn from_palette(palette: &Palette) -> Self {
        Self {
            bg: css_colour(palette.window),
            fg: css_colour(palette.omnibox_text),
            accent: css_colour(palette.omnibox_focus_border),
            border: css_colour(palette.omnibox_border),
            fill: css_colour(palette.omnibox_fill),
        }
    }
}

/// Only plain web pages are ever scripted. Chrome surfaces (`bowser-ui:`), `about:`, `file:`,
/// `data:` and every other scheme are refused.
pub fn url_allowed(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https") && url.host_str().is_some_and(|host| !host.is_empty())
}

/// Whether the probe's numbers describe a page worth offering the reader for.
pub fn looks_like_article(words: u32, link_percent: u32) -> bool {
    words >= MIN_ARTICLE_WORDS && link_percent <= MAX_LINK_PERCENT
}

/// The script call for `action`, ready for `ExecuteScript`: `((function (o) {...}))({...})`. Colours
/// that are not plain CSS colours are left out, and the page script validates them again.
pub fn invocation(action: Action, prefs: Prefs, dark: bool, colours: &Colours) -> String {
    invocation_with(action, prefs, Typography::default(), dark, colours)
}

/// [`invocation`] with the typography Settings > Page tools chose.
pub fn invocation_with(action: Action, prefs: Prefs, typography: Typography, dark: bool, colours: &Colours) -> String {
    let mode = match action {
        Action::Toggle(mode) => mode,
        Action::Probe | Action::Exit => Mode::Reader,
    };
    let mut options = format!(
        "{{\"a\":\"{}\",\"m\":\"{}\",\"t\":\"{}\",\"s\":{},\"d\":{}",
        action.key(),
        mode.key(),
        prefs.theme.key(),
        prefs.size.clamp(MIN_SIZE, MAX_SIZE),
        dark
    );
    options.push_str(&format!(
        ",\"f\":\"{}\",\"l\":{},\"w\":{},\"v\":{},\"g\":{},\"r\":{}",
        typography.font.key(),
        typography.line_tenths.clamp(12, 24),
        typography.width_ch.clamp(40, 120),
        typography.vertical,
        typography.controls,
        typography.reading_time
    ));
    if matches!(action, Action::Toggle(_)) {
        options.push_str(",\"c\":{");
        let mut first = true;
        for (key, value) in [
            ("bg", &colours.bg),
            ("fg", &colours.fg),
            ("ac", &colours.accent),
            ("bd", &colours.border),
            ("fl", &colours.fill),
        ] {
            if !is_safe_css_colour(value) {
                continue;
            }
            if !first {
                options.push(',');
            }
            first = false;
            options.push_str(&format!("\"{key}\":{}", json_string(value)));
        }
        options.push_str("},\"k\":");
        options.push_str(&json_string(CSS));
        options.push_str(",\"b\":");
        options.push_str(&json_string(BAR_CSS));
    }
    options.push('}');
    format!("({})({options})", SCRIPT.trim())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    Ok,
    /// Reader view found no article-like content, so it changed nothing.
    None,
    /// The script failed; the page was returned to how it was where possible.
    Failed,
}

/// A validated script result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub code: Code,
    pub state: Active,
    pub words: u32,
    pub link_percent: u32,
    pub prefs: Prefs,
    /// The page's declared language (2 to 3 lower-case letters), or none. Used only to offer
    /// translation (`crate::translate`).
    pub lang: crate::translate::Lang,
}

fn digits(text: &str, max_len: usize) -> Option<u32> {
    if text.is_empty() || text.len() > max_len || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Parses what `ExecuteScript` returned: a JSON string literal holding
/// `r2|code|state|words|link|theme|size|lang`. Strict on purpose. The text came from a page's world, so
/// any other shape, length or character is refused rather than interpreted.
pub fn parse_result(raw: &str) -> Option<Outcome> {
    let text = raw.trim().strip_prefix('"')?.strip_suffix('"')?;
    if text.len() > MAX_RESULT_LEN || !text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'|') {
        return None;
    }
    let mut parts = text.split('|');
    let (version, code, state, words, link, theme, size, lang) = (
        parts.next()?,
        parts.next()?,
        parts.next()?,
        parts.next()?,
        parts.next()?,
        parts.next()?,
        parts.next()?,
        parts.next()?,
    );
    if parts.next().is_some() || version != "r2" {
        return None;
    }
    let code = match code {
        "ok" => Code::Ok,
        "none" => Code::None,
        "err" => Code::Failed,
        _ => return None,
    };
    let link_percent = digits(link, 3).filter(|percent| *percent <= 100)?;
    let size = digits(size, 2).filter(|size| (u32::from(MIN_SIZE)..=u32::from(MAX_SIZE)).contains(size))?;
    Some(Outcome {
        code,
        state: Active::parse(state)?,
        words: digits(words, 7)?,
        link_percent,
        prefs: Prefs { theme: Theme::parse(theme)?, size: size as u8 },
        lang: crate::translate::parse_lang(lang)?,
    })
}

/// What the toolbar button shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pill {
    /// Not shown.
    Hidden,
    /// The page looks like an article and reader view can be started.
    Available,
    /// Reader view is on; the button leaves it.
    On,
}

impl Pill {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hidden => "none",
            Self::Available => "available",
            Self::On => "on",
        }
    }
}

/// Per-tab reader state, owned by the app. It is a hint about the page; the page itself is the
/// authority and every command re-reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabState {
    /// The last probe found article-like content.
    pub article: bool,
    /// A probe has been answered (or is not needed) for the current document.
    pub probed: bool,
    pub active: Active,
    /// A script call is in flight; further commands for this tab wait for it.
    pub busy: bool,
    /// Bumped whenever the document changes, so the answer to a call made for an earlier document
    /// is recognised and dropped.
    pub epoch: u32,
}

impl TabState {
    pub fn pill(&self) -> Pill {
        match (self.active, self.article) {
            (Active::Reader, _) => Pill::On,
            (_, true) => Pill::Available,
            _ => Pill::Hidden,
        }
    }

    /// A new document (or a same-document address change) invalidates what was known.
    pub fn reset(&mut self) {
        *self = Self { epoch: self.epoch.wrapping_add(1), ..Self::default() };
    }

    /// Folds in the outcome of a probe.
    pub fn apply_probe(&mut self, outcome: &Outcome) {
        self.probed = true;
        self.active = outcome.state;
        self.article = outcome.code == Code::Ok && looks_like_article(outcome.words, outcome.link_percent);
    }

    /// Folds in the outcome of a toggle or exit. `Failed` leaves the page as the script left it,
    /// which is how it was.
    pub fn apply_toggle(&mut self, outcome: &Outcome) {
        match outcome.code {
            Code::Ok => {
                self.active = outcome.state;
                self.article |= outcome.state == Active::Reader
                    && looks_like_article(outcome.words, outcome.link_percent);
            }
            Code::None => {
                self.active = Active::Off;
                self.article = false;
            }
            Code::Failed => self.active = Active::Off,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colours() -> Colours {
        Colours {
            bg: "#101010".into(),
            fg: "#eeeeee".into(),
            accent: "rgba(138, 180, 248, 1)".into(),
            border: "#444444".into(),
            fill: "#222".into(),
        }
    }

    #[test]
    fn only_plain_web_pages_are_eligible() {
        for good in ["https://example.com/a", "http://localhost:8080/", "http://127.0.0.1/x?y=1#z"] {
            assert!(url_allowed(&Url::parse(good).unwrap()), "{good}");
        }
        for bad in [
            "bowser-ui://app/index.html?view=panel",
            "about:blank",
            "file:///C:/page.html",
            "data:text/html,hi",
            "javascript:alert(1)",
            "ftp://example.com/",
            "view-source:https://example.com/",
            "blob:https://example.com/id",
            "chrome://settings",
            "edge://flags",
        ] {
            let parsed = Url::parse(bad).unwrap();
            assert!(!url_allowed(&parsed), "{bad}");
        }
    }

    #[test]
    fn article_eligibility_needs_enough_text_and_few_links() {
        assert!(looks_like_article(250, 35));
        assert!(looks_like_article(5000, 0));
        assert!(!looks_like_article(249, 0), "too short");
        assert!(!looks_like_article(5000, 36), "mostly links");
        assert!(!looks_like_article(0, 0));
    }

    #[test]
    fn result_parser_accepts_exactly_the_documented_shape() {
        let outcome = parse_result("\"r2|ok|reader|1234|7|sepia|19|fr\"").unwrap();
        assert_eq!(
            outcome,
            Outcome {
                code: Code::Ok,
                state: Active::Reader,
                words: 1234,
                link_percent: 7,
                prefs: Prefs { theme: Theme::Sepia, size: 19 },
                lang: crate::translate::Lang::new("fr").unwrap(),
            }
        );
        assert_eq!(parse_result("  \"r2|none|off|12|0|auto|14|\"  ").unwrap().code, Code::None);
        assert!(parse_result("\"r2|none|off|12|0|auto|14|\"").unwrap().lang.is_none(), "a page with no language");
        assert_eq!(parse_result("\"r2|err|off|0|0|dark|30|de\"").unwrap().code, Code::Failed);
        assert_eq!(parse_result("\"r2|ok|calm|0|100|light|22|fil\"").unwrap().state, Active::Calm);
        assert_eq!(parse_result("\"r2|ok|off|1|1|auto|19|zh\"").unwrap().lang.as_str(), "zh");
    }

    #[test]
    fn result_parser_refuses_everything_else() {
        for bad in [
            "",
            "null",
            "undefined",
            "r2|ok|off|1|1|auto|19|fr",
            "'r2|ok|off|1|1|auto|19|fr'",
            "\"r2|ok|off|1|1|auto|19|fr",
            // The previous version (no language) is not accepted: the script and the parser ship together.
            "\"r1|ok|off|1|1|auto|19\"",
            "\"r3|ok|off|1|1|auto|19|fr\"",
            "\"r2|ok|off|1|1|auto|19\"",
            "\"r2|ok|off|1|1|auto|19|fr|x\"",
            "\"r2|maybe|off|1|1|auto|19|fr\"",
            "\"r2|ok|paused|1|1|auto|19|fr\"",
            "\"r2|ok|off|1|1|neon|19|fr\"",
            "\"r2|ok|off|1|101|auto|19|fr\"",
            "\"r2|ok|off|-1|1|auto|19|fr\"",
            "\"r2|ok|off|1.5|1|auto|19|fr\"",
            "\"r2|ok|off|12345678|1|auto|19|fr\"",
            "\"r2|ok|off||1|auto|19|fr\"",
            "\"r2|ok|off|1|1|auto|13|fr\"",
            "\"r2|ok|off|1|1|auto|31|fr\"",
            "\"r2|ok|off|1|1|auto|190|fr\"",
            "\"r2|ok|off|1|1|auto|19|fr\\n\"",
            "\"r2|ok|off|1|1|auto|19|fr \"",
            "\"r2|ok|off|1|1|auto|1\u{0661}|fr\"",
            // The language field takes a code or nothing: not a tag, not upper case, not markup.
            "\"r2|ok|off|1|1|auto|19|FR\"",
            "\"r2|ok|off|1|1|auto|19|f\"",
            "\"r2|ok|off|1|1|auto|19|frxx\"",
            "\"r2|ok|off|1|1|auto|19|fr-FR\"",
            "\"r2|ok|off|1|1|auto|19|f1\"",
            "\"r2|ok|off|1|1|auto|19|<s\"",
            "{\"ok\":true}",
            "\"r2|ok|off|1|1|auto|19|https://evil.example/?q=page-text\"",
        ] {
            assert_eq!(parse_result(bad), None, "{bad:?}");
        }
        // Page text smuggled into an otherwise valid field never gets through.
        let long = format!("\"r2|ok|off|1|1|auto|{}|fr\"", "9".repeat(60));
        assert_eq!(parse_result(&long), None);
        let long_lang = format!("\"r2|ok|off|1|1|auto|19|{}\"", "a".repeat(60));
        assert_eq!(parse_result(&long_lang), None);
    }

    #[test]
    fn invocation_is_a_call_of_the_script_with_validated_colours() {
        let call = invocation(Action::Toggle(Mode::Reader), Prefs::default(), true, &colours());
        assert!(call.starts_with("((function (o) {"), "{}", &call[..40]);
        assert!(call.contains("\"a\":\"toggle\",\"m\":\"reader\",\"t\":\"auto\",\"s\":19,\"d\":true"));
        assert!(call.contains("\"bg\":\"#101010\""));
        assert!(call.contains("\"ac\":\"rgba(138, 180, 248, 1)\""));
        assert!(call.contains("\"k\":\"") && call.contains("\"b\":\""));
        assert!(!call.contains("</script"), "never closes a script element");
        // A colour that is not a plain CSS colour never reaches the page.
        let mut hostile = colours();
        hostile.bg = "red; background: url(https://evil.example)".into();
        hostile.fg = "expression(alert(1))".into();
        let call = invocation(Action::Toggle(Mode::Calm), Prefs::default(), false, &hostile);
        assert!(!call.contains("evil.example") && !call.contains("expression"));
        assert!(call.contains("\"m\":\"calm\"") && call.contains("\"d\":false"));
        assert!(call.contains("\"ac\":"), "valid colours stay");
    }

    #[test]
    fn probe_and_exit_carry_no_styles() {
        for action in [Action::Probe, Action::Exit] {
            let call = invocation(action, Prefs::default(), false, &colours());
            assert!(!call.contains("\"k\":") && !call.contains("\"c\":"), "{action:?}");
            assert!(call.contains(&format!("\"a\":\"{}\"", action.key())));
        }
    }

    #[test]
    fn invocation_clamps_the_size() {
        let call = invocation(
            Action::Probe,
            Prefs { theme: Theme::Dark, size: 250 },
            false,
            &colours(),
        );
        assert!(call.contains("\"t\":\"dark\",\"s\":30"), "{call}");
        let call = invocation(Action::Probe, Prefs { theme: Theme::Light, size: 1 }, false, &colours());
        assert!(call.contains("\"s\":14"));
    }

    #[test]
    fn the_embedded_files_are_what_the_contract_says() {
        assert!(SCRIPT.trim_start().starts_with("(function (o) {"));
        assert!(SCRIPT.trim_end().ends_with("})"));
        assert!(CSS.contains("/*R*/"), "reader styles are split by the marker");
        assert!(!CSS.contains("url(") && !CSS.contains("@import"), "styles load nothing");
        assert!(!BAR_CSS.contains("url(") && !BAR_CSS.contains("@import"));
        // The page-world script must not reach for dynamic code, the network, or storage.
        for banned in [
            "eval(", "new Function", "innerHTML", "outerHTML", "insertAdjacentHTML", "document.write",
            "fetch(", "XMLHttpRequest", "sendBeacon", "WebSocket", "localStorage", "sessionStorage",
            "document.cookie", "postMessage", "import(", "chrome.webview",
        ] {
            assert!(!SCRIPT.contains(banned), "reader.js must not contain {banned}");
        }
    }

    #[test]
    fn tab_state_shows_the_button_only_for_articles_and_while_reading() {
        let mut state = TabState::default();
        assert_eq!(state.pill(), Pill::Hidden);
        let article = Outcome { code: Code::Ok, state: Active::Off, words: 900, link_percent: 8, prefs: Prefs::default(), lang: crate::translate::Lang::NONE };
        state.apply_probe(&article);
        assert!(state.probed);
        assert_eq!(state.pill(), Pill::Available);
        let front_page = Outcome { words: 2000, link_percent: 80, ..article };
        state.apply_probe(&front_page);
        assert_eq!(state.pill(), Pill::Hidden);
        let on = Outcome { state: Active::Reader, ..article };
        state.apply_toggle(&on);
        assert_eq!(state.pill(), Pill::On);
        state.apply_toggle(&Outcome { state: Active::Off, ..article });
        assert_eq!(state.pill(), Pill::Available, "leaving keeps the article hint");
        // Distraction hiding does not press the reader button.
        state.apply_toggle(&Outcome { state: Active::Calm, ..article });
        assert_eq!(state.pill(), Pill::Available);
        state.apply_toggle(&Outcome { code: Code::None, state: Active::Off, ..article });
        assert_eq!(state.pill(), Pill::Hidden, "nothing to read hides the button");
        state.apply_probe(&article);
        state.reset();
        assert_eq!(
            state,
            TabState { epoch: 1, ..TabState::default() },
            "everything is forgotten but the generation moves on"
        );
    }

    #[test]
    fn reader_settings_are_session_only_and_never_touch_disk() {
        // Reader view is allowed in private tabs, so nothing it remembers may reach a file: the
        // production half of this module (before the tests) has no file API and no store.
        let source = include_str!("mod.rs");
        let production = source.split("#[cfg(test)]").next().unwrap();
        for banned in ["std::fs", "fs::", "File::", "OpenOptions", "site_prefs", "std::io::Write", "write_all"] {
            assert!(!production.contains(banned), "reader/mod.rs must not use {banned}");
        }
        // Defaults are what a fresh session starts from; nothing loads them from elsewhere.
        assert_eq!(Prefs::default(), Prefs { theme: Theme::Auto, size: DEFAULT_SIZE });
    }

    #[test]
    fn a_failed_call_never_leaves_the_button_pressed() {
        let mut state = TabState { article: true, probed: true, active: Active::Reader, busy: false, epoch: 0 };
        state.apply_toggle(&Outcome { code: Code::Failed, state: Active::Off, words: 0, link_percent: 0, prefs: Prefs::default(), lang: crate::translate::Lang::NONE });
        assert_eq!(state.active, Active::Off);
        assert_eq!(state.pill(), Pill::Available);
    }
}
