//! On-device page translation: the pure decisions around an on-request page script.
//!
//! Page text never leaves the machine. Nothing happens until the user asks (the toolbar button, the
//! main menu or the command palette): Bowser then downloads the language files for that pair once
//! (`store`), starts a hidden worker page that runs Mozilla's Bergamot translator (`worker.js`) on
//! those files, and moves the page's text through it in batches: one `ExecuteScript` call collects
//! a batch from the page (`translate.js`), the worker translates it, and the next call writes it
//! back and collects the following batch. No script is injected at document creation, no page
//! observer is installed, and a page that is not being translated costs nothing but the language
//! tag the reader probe already reports (`crate::reader`).
//!
//! Everything that comes back from a page or the worker is validated here ([`parse_reply`],
//! [`parse_worker_message`]) before it is used; see `docs/translate.md`.

pub mod json;
pub mod manifest;
pub mod store;

use url::Url;

use crate::chrome_host::protocol::json_string;
use crate::site_prefs::SiteTranslate;

pub use manifest::{File, MANIFEST, Manifest, Pair};

/// The page script: a function expression taking one options object.
pub const SCRIPT: &str = include_str!("translate.js");
/// The worker page and its script, written next to the language files when a worker is first needed.
pub const WORKER_HTML: &str = include_str!("worker.html");
pub const WORKER_JS: &str = include_str!("worker.js");
/// The virtual host the worker page is served from: a folder mapping, never the network.
pub const WORKER_HOST: &str = "bowser-translate.invalid";
/// How long the worker page may sit unused before it is closed.
pub const WORKER_IDLE_SECS: u64 = 120;

/// Most strings in one batch, either direction.
pub const MAX_BATCH_ITEMS: usize = 400;
/// Longest single string accepted (UTF-8 bytes).
pub const MAX_ITEM_BYTES: usize = 8 * 1024;
/// Most text in one batch (UTF-8 bytes).
pub const MAX_BATCH_BYTES: usize = 96 * 1024;
/// Longest raw answer read from a page or the worker, in bytes.
pub const MAX_ANSWER_BYTES: usize = 384 * 1024;
/// Most batches one translation will run, so a page that keeps producing text ends.
pub const MAX_BATCHES: usize = 80;

/// Only plain web pages are ever translated. It is the same rule as reader view, and the worker's
/// own virtual host is never a page.
pub fn url_allowed(url: &Url) -> bool {
    crate::reader::url_allowed(url) && url.host_str() != Some(WORKER_HOST)
}

/// A language reduced to its primary subtag, as lower-case ASCII letters (`en-GB` is `en`). `None`
/// for anything that is not a plain language tag: empty, `und`, `x-...`, a subtag with a character
/// that is not a letter or digit, or a primary subtag that is not 2 or 3 letters.
pub fn normalize_lang(tag: &str) -> Option<String> {
    let tag = tag.trim();
    if tag.is_empty() || tag.len() > 35 {
        return None;
    }
    let mut subtags = tag.split(['-', '_']);
    let primary = subtags.next()?;
    if !(2..=3).contains(&primary.len()) || !primary.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    // The rest must at least look like language subtags, so markup or text smuggled into a `lang`
    // attribute is refused instead of being cut at the first dash.
    if subtags.any(|subtag| subtag.is_empty() || subtag.len() > 8 || !subtag.bytes().all(|byte| byte.is_ascii_alphanumeric())) {
        return None;
    }
    let primary = primary.to_ascii_lowercase();
    // "und" (undetermined), "mul" (multiple) and "zxx" (no linguistic content) say nothing.
    (!matches!(primary.as_str(), "und" | "mul" | "zxx" | "mis")).then_some(primary)
}

/// A page's language as the probe reports it: nothing, or 2 to 3 lower-case letters. It is `Copy`,
/// so the per-tab state stays plain data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lang {
    bytes: [u8; 3],
    len: u8,
}

impl Lang {
    /// No language known.
    pub const NONE: Self = Self { bytes: [0; 3], len: 0 };

    /// From an already normalised code (see [`normalize_lang`]).
    pub fn new(code: &str) -> Option<Self> {
        let code = code.as_bytes();
        if !(2..=3).contains(&code.len()) || !code.iter().all(u8::is_ascii_lowercase) {
            return None;
        }
        let mut bytes = [0; 3];
        bytes[..code.len()].copy_from_slice(code);
        Some(Self { bytes, len: code.len() as u8 })
    }

    pub fn is_none(self) -> bool {
        self.len == 0
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }
}

/// The language field of the reader probe's answer: empty (the page did not say) or 2 to 3
/// lower-case letters. Anything else is refused, so nothing but a language code gets through.
pub fn parse_lang(field: &str) -> Option<Lang> {
    if field.is_empty() {
        return Some(Lang::NONE);
    }
    Lang::new(field)
}

/// The Windows display language, reduced to its primary subtag. Asked once, the first time it is
/// needed; never at startup.
pub fn ui_language() -> &'static str {
    use std::sync::OnceLock;
    static LANGUAGE: OnceLock<String> = OnceLock::new();
    LANGUAGE.get_or_init(|| windows_ui_language().and_then(|tag| normalize_lang(&tag)).unwrap_or_else(|| "en".to_owned()))
}

fn windows_ui_language() -> Option<String> {
    use windows::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
    use windows::core::PWSTR;
    let mut count = 0u32;
    let mut length = 0u32;
    unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut length) }.ok()?;
    if length == 0 || length > 4096 {
        return None;
    }
    let mut buffer = vec![0u16; length as usize];
    unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, Some(PWSTR(buffer.as_mut_ptr())), &mut length) }.ok()?;
    // A double-NUL-terminated list; the first entry is the preferred language.
    let first = buffer.split(|unit| *unit == 0).next()?;
    (!first.is_empty()).then(|| String::from_utf16_lossy(first))
}

/// The language pages are translated into: the setting when it is a language, else the Windows
/// display language.
pub fn target_language(setting: &str) -> String {
    match normalize_lang(setting) {
        Some(target) => target,
        None => ui_language().to_owned(),
    }
}

/// What the toolbar button shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pill {
    /// Not shown.
    Hidden,
    /// The page is in another language that can be translated into the target.
    Available,
    /// The page is translated; the button shows the original.
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

/// Whether to offer translation for a page, and whether to start it without being asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offer {
    No,
    /// Show the button.
    Offer,
    /// Show the button and translate now (the site is set to "always translate", the automatic
    /// setting is on, and the language files are already on disk).
    Auto,
}

/// Everything [`should_offer`] decides from.
#[derive(Clone, Copy, Debug)]
pub struct OfferInput<'a> {
    /// The page's language from the probe; [`Lang::NONE`] when the page did not say.
    pub page_lang: Lang,
    /// The language pages are translated into.
    pub target: &'a str,
    /// The `translate_offer` setting.
    pub offer_on: bool,
    /// The `translate_auto` setting.
    pub auto_on: bool,
    /// The stored per-site choice, if any.
    pub site: Option<SiteTranslate>,
    pub private: bool,
    /// The language files for this page's route are already on disk.
    pub pack_ready: bool,
}

/// The offer for a page. Pure: no I/O, so it is safe to run for every toolbar refresh.
///
/// A page with no language is never offered (the menu command still works). A page already in the
/// target language, or in one no route reaches the target from, is not offered. "Never" for a site
/// hides the offer. A private tab ignores stored per-site choices entirely, so nothing recorded
/// outside it changes what it does; it still offers, and translating there is always the user's own
/// click.
pub fn should_offer(input: &OfferInput<'_>) -> Offer {
    if !input.offer_on || input.page_lang.is_none() || input.page_lang.as_str() == input.target {
        return Offer::No;
    }
    if pair_route(input.page_lang.as_str(), input.target).is_none() {
        return Offer::No;
    }
    let site = if input.private { None } else { input.site };
    match site {
        Some(SiteTranslate::Never) => Offer::No,
        Some(SiteTranslate::Always) if input.auto_on && input.pack_ready => Offer::Auto,
        _ => Offer::Offer,
    }
}

/// How a pair of languages is translated: one model, or two through English.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    pub first: &'static Pair,
    pub second: Option<&'static Pair>,
}

impl Route {
    /// The pair ids the worker loads and runs, in order (`fr-en`, then `en-es`).
    pub fn ids(&self) -> Vec<&'static str> {
        std::iter::once(self.first.id).chain(self.second.map(|pair| pair.id)).collect()
    }

    /// Every file the route needs on disk: the engine, then each pair's files.
    pub fn files(&self) -> Vec<&'static File> {
        let mut files: Vec<&'static File> = MANIFEST.engine.iter().collect();
        for pair in std::iter::once(self.first).chain(self.second) {
            files.extend(pair.files.iter());
        }
        files
    }

    pub fn src(&self) -> &'static str {
        self.first.src
    }

    pub fn dst(&self) -> &'static str {
        self.second.map_or(self.first.dst, |pair| pair.dst)
    }
}

/// The route from `src` to `dst`: a direct pair when there is one, else through English (so `fr` to
/// `es` runs `fr-en` then `en-es`). `None` for the same language, an unknown language, or a
/// language that has no route.
pub fn pair_route(src: &str, dst: &str) -> Option<Route> {
    if src == dst {
        return None;
    }
    if let Some(first) = MANIFEST.pair(src, dst) {
        return Some(Route { first, second: None });
    }
    if src != "en" && dst != "en" {
        let first = MANIFEST.pair(src, "en")?;
        let second = MANIFEST.pair("en", dst)?;
        return Some(Route { first, second: Some(second) });
    }
    None
}

/// The languages a page can be translated into: English, and every language that has an
/// English-to-language model (anything else reaches English first, then one of these).
pub fn target_languages() -> Vec<&'static str> {
    let mut targets = vec!["en"];
    for pair in MANIFEST.pairs {
        if pair.src == "en" && !targets.contains(&pair.dst) {
            targets.push(pair.dst);
        }
    }
    targets
}

/// The English name of a language the manifest covers, for messages.
pub fn language_name(code: &str) -> &str {
    match code {
        "en" => "English",
        "de" => "German",
        "es" => "Spanish",
        "fr" => "French",
        "it" => "Italian",
        "nl" => "Dutch",
        "pl" => "Polish",
        "pt" => "Portuguese",
        "ru" => "Russian",
        "tr" => "Turkish",
        _ => code,
    }
}

/// Per-tab translation state, owned by the app. The page is the authority (it keeps the originals)
/// and every command re-reads it; this is a hint for the toolbar and a guard against overlap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabState {
    /// The page's language from the reader probe.
    pub page_lang: Lang,
    /// The page shows a translation.
    pub active: bool,
    /// A translation or restore is running for this tab.
    pub busy: bool,
    /// Bumped whenever the document changes, so work started for an earlier document is recognised
    /// and dropped.
    pub epoch: u32,
}

impl TabState {
    /// A new document (or a same-document address change) invalidates what was known.
    pub fn reset(&mut self) {
        *self = Self { epoch: self.epoch.wrapping_add(1), ..Self::default() };
    }

    pub fn pill(&self, offered: bool) -> Pill {
        if self.active {
            Pill::On
        } else if offered {
            Pill::Available
        } else {
            Pill::Hidden
        }
    }

    /// Whether work started at `epoch` still belongs to this document.
    pub fn is_current(&self, epoch: u32) -> bool {
        self.epoch == epoch
    }
}

// --- page script calls ---

/// One call of the page script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action<'a> {
    /// Gather the page's text and return the first batch.
    Collect,
    /// Write `strings` back from item `index` of the page named by `token`, and return the next batch.
    Apply { token: &'a str, index: usize, strings: &'a [String] },
    /// Put every original back.
    Restore,
}

/// The script call for `action`, ready for `ExecuteScript`: `((function (o) {...}))({...})`. Strings
/// are escaped with [`json_string`], which also writes `<`, U+2028 and U+2029 as escapes, so no text
/// can close a script element or end a line.
pub fn invocation(action: &Action<'_>) -> String {
    let options = match action {
        Action::Collect => "{\"a\":\"collect\"}".to_owned(),
        Action::Restore => "{\"a\":\"restore\"}".to_owned(),
        Action::Apply { token, index, strings } => {
            let mut options = format!("{{\"a\":\"apply\",\"k\":{},\"i\":{index},\"s\":[", json_string(token));
            for (position, text) in strings.iter().enumerate() {
                if position > 0 {
                    options.push(',');
                }
                options.push_str(&json_string(text));
            }
            options.push_str("]}");
            options
        }
    };
    format!("({})({options})", SCRIPT.trim())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyCode {
    /// A batch (possibly the last, empty one).
    Ok,
    /// The page has no text worth translating.
    None,
    /// The page was already translated.
    On,
    /// The token is not this page's (it changed under the call).
    Stale,
    /// The script failed; the page was returned to how it was where possible.
    Failed,
}

/// A validated answer of the page script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub code: ReplyCode,
    /// The page's token: 1 to 16 characters of `a-z0-9`, or empty.
    pub token: String,
    /// Index of the first string in `strings`.
    pub next: usize,
    /// How many strings the page holds in all.
    pub total: usize,
    pub strings: Vec<String>,
}

/// A JSON array of strings within the batch limits, or `None`.
fn strings_of(value: &json::Value) -> Option<Vec<String>> {
    let items = value.as_array()?;
    if items.len() > MAX_BATCH_ITEMS {
        return None;
    }
    let mut total = 0usize;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let text = item.as_str()?;
        total += text.len();
        if text.len() > MAX_ITEM_BYTES || total > MAX_BATCH_BYTES {
            return None;
        }
        out.push(text.to_owned());
    }
    Some(out)
}

/// A batch of strings: exactly a JSON array of strings, within [`MAX_BATCH_ITEMS`], [`MAX_ITEM_BYTES`]
/// and [`MAX_BATCH_BYTES`]. Non-arrays, nested values, other types and anything oversized are refused.
pub fn parse_batch(raw: &str) -> Option<Vec<String>> {
    if raw.len() > MAX_ANSWER_BYTES {
        return None;
    }
    strings_of(&json::parse(raw)?)
}

fn valid_token(token: &str) -> bool {
    token.len() <= 16 && token.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

/// Parses what `ExecuteScript` returned for a page script call: one JSON object
/// `{"v":1,"c":code,"k":token,"i":next,"n":total,"s":[strings]}` and nothing else. The answer came
/// from a page's world, so any other shape, member, length or character is refused.
pub fn parse_reply(raw: &str) -> Option<Reply> {
    if raw.len() > MAX_ANSWER_BYTES {
        return None;
    }
    let value = json::parse(raw)?;
    let mut keys = value.keys();
    keys.sort_unstable();
    if keys != ["c", "i", "k", "n", "s", "v"] || value.get("v")?.as_int()? != 1 {
        return None;
    }
    let code = match value.get("c")?.as_str()? {
        "ok" => ReplyCode::Ok,
        "none" => ReplyCode::None,
        "on" => ReplyCode::On,
        "stale" => ReplyCode::Stale,
        "err" => ReplyCode::Failed,
        _ => return None,
    };
    let token = value.get("k")?.as_str()?;
    if !valid_token(token) {
        return None;
    }
    let count = |key: &str| usize::try_from(value.get(key)?.as_int()?).ok().filter(|count| *count <= 1_000_000);
    let (next, total) = (count("i")?, count("n")?);
    let strings = strings_of(value.get("s")?)?;
    if next > total || next + strings.len() > total {
        return None;
    }
    Some(Reply { code, token: token.to_owned(), next, total, strings })
}

// --- worker messages ---

/// Why the worker could not do what was asked. Fixed words, never page text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerError {
    /// The engine files could not be loaded.
    Engine,
    /// A language model could not be loaded or is not loaded.
    Model,
    /// The engine failed on a batch.
    Translate,
    /// The worker did not understand the request.
    Bad,
}

/// A validated message from the worker page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkerMessage {
    Ready,
    Loaded { id: u32 },
    Out { id: u32, strings: Vec<String> },
    Error { id: u32, error: WorkerError },
}

/// Parses a worker message: a JSON object whose members are exactly those of one of the four
/// shapes. Anything else is refused.
pub fn parse_worker_message(raw: &str) -> Option<WorkerMessage> {
    if raw.len() > MAX_ANSWER_BYTES {
        return None;
    }
    let value = json::parse(raw)?;
    let mut keys = value.keys();
    keys.sort_unstable();
    let id = || u32::try_from(value.get("id")?.as_int()?).ok();
    match value.get("t")?.as_str()? {
        "ready" if keys == ["t"] => Some(WorkerMessage::Ready),
        "loaded" if keys == ["id", "t"] => Some(WorkerMessage::Loaded { id: id()? }),
        "out" if keys == ["id", "s", "t"] => Some(WorkerMessage::Out { id: id()?, strings: strings_of(value.get("s")?)? }),
        "error" if keys == ["id", "m", "t"] => {
            let error = match value.get("m")?.as_str()? {
                "engine" => WorkerError::Engine,
                "model" => WorkerError::Model,
                "translate" => WorkerError::Translate,
                "bad" => WorkerError::Bad,
                _ => return None,
            };
            Some(WorkerMessage::Error { id: id()?, error })
        }
        _ => None,
    }
}

/// The message that makes the worker load a route's models.
pub fn load_message(id: u32, route: &Route) -> String {
    let pairs: Vec<String> = route.ids().into_iter().map(json_string).collect();
    format!("{{\"t\":\"load\",\"id\":{id},\"pairs\":[{}]}}", pairs.join(","))
}

/// The message that makes the worker translate `strings` along a route.
pub fn translate_message(id: u32, route: &Route, strings: &[String]) -> String {
    let ids: Vec<String> = route.ids().into_iter().map(json_string).collect();
    let texts: Vec<String> = strings.iter().map(|text| json_string(text)).collect();
    format!("{{\"t\":\"tr\",\"id\":{id},\"route\":[{}],\"s\":[{}]}}", ids.join(","), texts.join(","))
}

/// Whether a worker address belongs to the worker's own virtual host: the only origin the worker
/// page may load from, navigate to, or send messages from.
pub fn is_worker_url(url: &Url) -> bool {
    url.scheme() == "https" && url.host_str() == Some(WORKER_HOST) && url.port().is_none()
}

/// Checks a model's translations against the batch they answer: the same number of strings, none
/// over the limits. Returns them, or `None` when the worker's answer cannot be trusted.
pub fn check_answer(sent: usize, strings: Vec<String>) -> Option<Vec<String>> {
    (strings.len() == sent).then_some(strings)
}

/// The text of the toolbar and menu command for a page in `target`.
pub fn command_label(target: &str, active: bool) -> String {
    if active {
        "Show original".to_owned()
    } else {
        format!("Translate page to {}", language_name(target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang(code: &str) -> Lang {
        Lang::new(code).unwrap()
    }

    fn offer(page: &str, target: &str) -> OfferInput<'static> {
        let target: &'static str = Box::leak(target.to_owned().into_boxed_str());
        OfferInput {
            page_lang: if page.is_empty() { Lang::NONE } else { lang(page) },
            target,
            offer_on: true,
            auto_on: false,
            site: None,
            private: false,
            pack_ready: false,
        }
    }

    #[test]
    fn normalize_lang_reduces_to_the_primary_subtag() {
        for (tag, expected) in [
            ("en", "en"),
            ("en-GB", "en"),
            ("en_US", "en"),
            ("FR", "fr"),
            ("zh-Hant-TW", "zh"),
            (" de-DE ", "de"),
            ("fil", "fil"),
            ("pt-BR", "pt"),
            ("sr-Latn-RS", "sr"),
            ("es-419", "es"),
        ] {
            assert_eq!(normalize_lang(tag).as_deref(), Some(expected), "{tag:?}");
        }
        for junk in [
            "",
            " ",
            "e",
            "english",
            "klingon-ish!",
            "1234",
            "x-private",
            "und",
            "mul",
            "zxx",
            "en-",
            "-en",
            "en--GB",
            "en-<script>",
            "<script>",
            "en-GB-verylongsubtag",
            "fr; alert(1)",
            "éé",
            &"a".repeat(40),
        ] {
            assert_eq!(normalize_lang(junk), None, "{junk:?}");
        }
    }

    #[test]
    fn parse_lang_takes_only_a_code_or_nothing() {
        assert_eq!(parse_lang(""), Some(Lang::NONE));
        assert_eq!(parse_lang("fr").unwrap().as_str(), "fr");
        assert_eq!(parse_lang("fil").unwrap().as_str(), "fil");
        for bad in ["f", "frxx", "FR", "f1", "fr-FR", "fr ", "<s", "fé", "1fr"] {
            assert_eq!(parse_lang(bad), None, "{bad:?}");
        }
        assert!(Lang::NONE.is_none() && !lang("fr").is_none());
    }

    #[test]
    fn should_offer_truth_table() {
        // A French page offered in English: the base case.
        assert_eq!(should_offer(&offer("fr", "en")), Offer::Offer);
        // The page's own language, no language, no route, or the whole thing off.
        assert_eq!(should_offer(&offer("en", "en")), Offer::No, "same language");
        assert_eq!(should_offer(&offer("", "en")), Offer::No, "a page with no language is never offered");
        assert_eq!(should_offer(&offer("ja", "en")), Offer::No, "no pinned route from ja");
        assert_eq!(should_offer(&offer("fr", "ja")), Offer::No, "no pinned route to ja");
        let mut input = offer("fr", "en");
        input.offer_on = false;
        assert_eq!(should_offer(&input), Offer::No, "global off");
        // A pivot route counts.
        assert_eq!(should_offer(&offer("fr", "es")), Offer::Offer);
        // A site set to never hides it; always alone only offers.
        let mut input = offer("fr", "en");
        input.site = Some(SiteTranslate::Never);
        assert_eq!(should_offer(&input), Offer::No, "site never");
        input.site = Some(SiteTranslate::Always);
        assert_eq!(should_offer(&input), Offer::Offer, "always does nothing by itself");
        // Always plus the automatic setting translates, but only with the files already on disk.
        input.auto_on = true;
        assert_eq!(should_offer(&input), Offer::Offer, "never starts a download unasked");
        input.pack_ready = true;
        assert_eq!(should_offer(&input), Offer::Auto, "site always plus translate_auto");
        // The automatic setting without a site choice does nothing either.
        input.site = None;
        assert_eq!(should_offer(&input), Offer::Offer);
        // A private tab ignores stored per-site choices altogether.
        let mut private = offer("fr", "en");
        private.private = true;
        private.auto_on = true;
        private.pack_ready = true;
        private.site = Some(SiteTranslate::Always);
        assert_eq!(should_offer(&private), Offer::Offer, "private: always is ignored");
        private.site = Some(SiteTranslate::Never);
        assert_eq!(should_offer(&private), Offer::Offer, "private: never is ignored");
    }

    #[test]
    fn pair_route_is_direct_then_through_english_then_nothing() {
        let direct = pair_route("fr", "en").unwrap();
        assert_eq!(direct.ids(), ["fr-en"]);
        assert_eq!((direct.src(), direct.dst()), ("fr", "en"));
        let into = pair_route("en", "de").unwrap();
        assert_eq!(into.ids(), ["en-de"]);
        let pivot = pair_route("fr", "es").unwrap();
        assert_eq!(pivot.ids(), ["fr-en", "en-es"]);
        assert_eq!((pivot.src(), pivot.dst()), ("fr", "es"));
        assert_eq!(pair_route("en", "en"), None);
        assert_eq!(pair_route("fr", "fr"), None);
        assert_eq!(pair_route("ja", "en"), None, "unsupported source");
        assert_eq!(pair_route("en", "ja"), None, "unsupported target");
        assert_eq!(pair_route("fr", "ja"), None, "unsupported pivot target");
        // Russian is pinned into English only: there is no English to Russian model.
        assert!(pair_route("ru", "en").is_some());
        assert_eq!(pair_route("en", "ru"), None);
        assert!(pair_route("ru", "fr").is_some(), "Russian reaches French through English");
        assert_eq!(pair_route("fr", "ru"), None, "the second hop (en-ru) does not exist");
    }

    #[test]
    fn a_route_lists_the_engine_and_each_pairs_files() {
        let direct = pair_route("fr", "en").unwrap().files();
        assert_eq!(direct.len(), MANIFEST.engine.len() + 3);
        let pivot = pair_route("fr", "es").unwrap().files();
        assert_eq!(pivot.len(), MANIFEST.engine.len() + 6);
        assert_eq!(direct[0].path, MANIFEST.engine[0].path, "the engine comes first");
    }

    #[test]
    fn target_language_prefers_the_setting_and_falls_back_to_the_display_language() {
        assert_eq!(target_language("de"), "de");
        assert_eq!(target_language("en-GB"), "en");
        // An empty or unreadable setting resolves to the display language, which is always a code.
        let fallback = target_language("");
        assert!(normalize_lang(&fallback).is_some(), "{fallback}");
        assert_eq!(target_language("<junk>"), fallback);
    }

    #[test]
    fn tab_state_reset_bumps_the_epoch_and_forgets_everything_else() {
        let mut state = TabState { page_lang: lang("fr"), active: true, busy: true, epoch: 4 };
        assert_eq!(state.pill(true), Pill::On, "on wins");
        state.reset();
        assert_eq!(state, TabState { epoch: 5, ..TabState::default() });
        assert_eq!(state.pill(false), Pill::Hidden);
        assert_eq!(state.pill(true), Pill::Available);
        let mut wrap = TabState { epoch: u32::MAX, ..TabState::default() };
        wrap.reset();
        assert_eq!(wrap.epoch, 0, "the generation wraps instead of overflowing");
    }

    #[test]
    fn work_for_an_earlier_document_is_stale() {
        let mut state = TabState::default();
        let started = state.epoch;
        assert!(state.is_current(started));
        state.reset();
        assert!(!state.is_current(started), "a navigation makes the old epoch stale");
        assert!(state.is_current(state.epoch));
    }

    #[test]
    fn pill_wire_values() {
        assert_eq!(Pill::Hidden.as_str(), "none");
        assert_eq!(Pill::Available.as_str(), "available");
        assert_eq!(Pill::On.as_str(), "on");
    }

    #[test]
    fn parse_batch_accepts_only_a_bounded_array_of_strings() {
        assert_eq!(parse_batch(r#"["a","b\n","é😀"]"#), Some(vec!["a".into(), "b\n".into(), "é😀".into()]));
        assert_eq!(parse_batch("[]"), Some(Vec::new()));
        for bad in [
            "",
            "null",
            "\"a\"",
            "{}",
            "{\"s\":[\"a\"]}",
            "[1]",
            "[\"a\",2]",
            "[null]",
            "[[\"a\"]]",
            "[{\"a\":\"b\"}]",
            "[\"a\",]",
            "[\"a\"] x",
            "[\"a\"",
        ] {
            assert_eq!(parse_batch(bad), None, "{bad:?}");
        }
        let too_many = format!("[{}]", vec!["\"a\""; MAX_BATCH_ITEMS + 1].join(","));
        assert_eq!(parse_batch(&too_many), None, "too many items");
        let at_limit = format!("[{}]", vec!["\"a\""; MAX_BATCH_ITEMS].join(","));
        assert_eq!(parse_batch(&at_limit).map(|batch| batch.len()), Some(MAX_BATCH_ITEMS));
        let long = format!("[\"{}\"]", "x".repeat(MAX_ITEM_BYTES + 1));
        assert_eq!(parse_batch(&long), None, "one string over the limit");
        // Many strings that are each fine but together too big.
        let chunk = "y".repeat(MAX_ITEM_BYTES);
        let items = MAX_BATCH_BYTES / MAX_ITEM_BYTES + 2;
        let heavy = format!("[{}]", vec![format!("\"{chunk}\""); items].join(","));
        assert_eq!(parse_batch(&heavy), None, "oversized total");
        // Over the raw answer limit before it is even parsed.
        assert_eq!(parse_batch(&format!("[\"{}\"]", "z".repeat(MAX_ANSWER_BYTES))), None);
    }

    fn reply(code: &str, token: &str, next: i64, total: i64, strings: &str) -> String {
        format!("{{\"v\":1,\"c\":\"{code}\",\"k\":\"{token}\",\"i\":{next},\"n\":{total},\"s\":{strings}}}")
    }

    #[test]
    fn parse_reply_accepts_exactly_the_documented_shape() {
        let got = parse_reply(&reply("ok", "ab12cd", 0, 5, r#"["Bonjour","le monde"]"#)).unwrap();
        assert_eq!(
            got,
            Reply { code: ReplyCode::Ok, token: "ab12cd".into(), next: 0, total: 5, strings: vec!["Bonjour".into(), "le monde".into()] }
        );
        assert_eq!(parse_reply(&reply("ok", "ab12cd", 5, 5, "[]")).unwrap().strings.len(), 0, "the last, empty batch");
        assert_eq!(parse_reply(&reply("none", "", 0, 0, "[]")).unwrap().code, ReplyCode::None);
        assert_eq!(parse_reply(&reply("on", "tok", 3, 3, "[]")).unwrap().code, ReplyCode::On);
        assert_eq!(parse_reply(&reply("stale", "", 0, 0, "[]")).unwrap().code, ReplyCode::Stale);
        assert_eq!(parse_reply(&reply("err", "", 0, 0, "[]")).unwrap().code, ReplyCode::Failed);
    }

    #[test]
    fn parse_reply_refuses_everything_else() {
        let good = reply("ok", "ab12", 0, 2, r#"["a","b"]"#);
        assert!(parse_reply(&good).is_some());
        for bad in [
            String::new(),
            "null".into(),
            "[]".into(),
            "\"ok\"".into(),
            good.replace("\"v\":1", "\"v\":2"),
            good.replace("\"c\":\"ok\"", "\"c\":\"maybe\""),
            good.replace("\"k\":\"ab12\"", "\"k\":\"AB12\""),
            good.replace("\"k\":\"ab12\"", "\"k\":\"abcdefghijklmnopq\""),
            good.replace("\"k\":\"ab12\"", "\"k\":\"a b\""),
            good.replace("\"k\":\"ab12\"", "\"k\":\"<s>\""),
            good.replace("\"i\":0", "\"i\":-1"),
            good.replace("\"i\":0", "\"i\":1.5"),
            good.replace("\"i\":0", "\"i\":\"0\""),
            good.replace("\"n\":2", "\"n\":1"),
            good.replace("\"n\":2", "\"n\":1000001"),
            good.replace("\"s\":[\"a\",\"b\"]", "\"s\":[\"a\",1]"),
            good.replace("\"s\":[\"a\",\"b\"]", "\"s\":\"ab\""),
            good.replace("}", ",\"x\":1}"),
            good.replace("\"v\":1,", ""),
            reply("ok", "ab12", 1, 2, r#"["a","b"]"#),
            format!("{good}x"),
            format!("\"{}\"", good.replace('"', "\\\"")),
        ] {
            assert_eq!(parse_reply(&bad), None, "{bad}");
        }
        assert!(parse_reply(&format!("{good} ")).is_some(), "trailing whitespace is harmless");
        // A duplicate member is not an object this script writes.
        assert_eq!(parse_reply(&good.replace("\"c\":\"ok\"", "\"c\":\"ok\",\"c\":\"err\"")), None);
        // Oversized before it is parsed.
        assert_eq!(parse_reply(&reply("ok", "a", 0, 1, &format!("[\"{}\"]", "q".repeat(MAX_ANSWER_BYTES)))), None);
    }

    #[test]
    fn parse_worker_message_knows_four_shapes() {
        assert_eq!(parse_worker_message(r#"{"t":"ready"}"#), Some(WorkerMessage::Ready));
        assert_eq!(parse_worker_message(r#"{"t":"loaded","id":3}"#), Some(WorkerMessage::Loaded { id: 3 }));
        assert_eq!(
            parse_worker_message(r#"{"t":"out","id":4,"s":["a","b"]}"#),
            Some(WorkerMessage::Out { id: 4, strings: vec!["a".into(), "b".into()] })
        );
        assert_eq!(
            parse_worker_message(r#"{"t":"error","id":5,"m":"model"}"#),
            Some(WorkerMessage::Error { id: 5, error: WorkerError::Model })
        );
        for bad in [
            "",
            "ready",
            r#"{"t":"ready","x":1}"#,
            r#"{"t":"loaded"}"#,
            r#"{"t":"loaded","id":-1}"#,
            r#"{"t":"loaded","id":4294967296}"#,
            r#"{"t":"loaded","id":"3"}"#,
            r#"{"t":"out","id":4}"#,
            r#"{"t":"out","id":4,"s":"a"}"#,
            r#"{"t":"out","id":4,"s":[1]}"#,
            r#"{"t":"error","id":5,"m":"anything the page said"}"#,
            r#"{"t":"error","id":5}"#,
            r#"{"t":"nope","id":5}"#,
            r#"{"id":5}"#,
            r#"[{"t":"ready"}]"#,
        ] {
            assert_eq!(parse_worker_message(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn answers_must_match_the_batch_that_was_sent() {
        assert_eq!(check_answer(2, vec!["a".into(), "b".into()]).map(|a| a.len()), Some(2));
        assert_eq!(check_answer(3, vec!["a".into(), "b".into()]), None);
        assert_eq!(check_answer(1, vec!["a".into(), "b".into()]), None);
    }

    #[test]
    fn invocation_is_a_call_of_the_script_with_escaped_strings() {
        let call = invocation(&Action::Collect);
        assert!(call.starts_with("((function (o) {"), "{}", &call[..40]);
        assert!(call.ends_with("}))({\"a\":\"collect\"})"), "{}", &call[call.len() - 40..]);
        assert!(invocation(&Action::Restore).ends_with("({\"a\":\"restore\"})"));
        let strings = vec![
            "plain".to_owned(),
            "</script><script>alert(1)</script>".to_owned(),
            "line\u{2028}sep\u{2029}end".to_owned(),
            "quote \" backslash \\ newline \n".to_owned(),
        ];
        let call = invocation(&Action::Apply { token: "ab12", index: 7, strings: &strings });
        let options = call.rsplit_once("))(").unwrap().1;
        assert!(options.starts_with("{\"a\":\"apply\",\"k\":\"ab12\",\"i\":7,\"s\":[\"plain\""), "{options}");
        assert!(!options.contains("</script") && !options.contains('<'), "never closes a script element");
        assert!(!options.contains('\u{2028}') && !options.contains('\u{2029}') && !options.contains('\n'));
        assert!(options.contains("\\u003c/script>"), "{options}");
        // What goes in comes back out unchanged through the strict reader.
        let literal = options.strip_prefix("{\"a\":\"apply\",\"k\":\"ab12\",\"i\":7,\"s\":").unwrap().strip_suffix("})").unwrap();
        assert_eq!(parse_batch(literal), Some(strings));
    }

    #[test]
    fn worker_messages_are_built_with_escaped_strings_and_parse_back() {
        let route = pair_route("fr", "es").unwrap();
        assert_eq!(load_message(7, &route), r#"{"t":"load","id":7,"pairs":["fr-en","en-es"]}"#);
        let sent = vec!["a \"b\" <i>".to_owned(), "\u{2028}".to_owned()];
        let message = translate_message(9, &route, &sent);
        assert!(message.starts_with(r#"{"t":"tr","id":9,"route":["fr-en","en-es"],"s":["#), "{message}");
        assert!(!message.contains('<') && !message.contains('\u{2028}'));
        let value = json::parse(&message).unwrap();
        assert_eq!(strings_of(value.get("s").unwrap()), Some(sent));
    }

    #[test]
    fn only_the_workers_own_origin_counts_as_the_worker() {
        for good in ["https://bowser-translate.invalid/worker.html", "https://bowser-translate.invalid/engine/x.js"] {
            assert!(is_worker_url(&Url::parse(good).unwrap()), "{good}");
        }
        for bad in [
            "http://bowser-translate.invalid/worker.html",
            "https://bowser-translate.invalid:8443/worker.html",
            "https://bowser-translate.invalid.evil.example/",
            "https://evil.example/bowser-translate.invalid",
            "https://example.com/",
            "bowser-ui://app/index.html",
            "about:blank",
        ] {
            assert!(!is_worker_url(&Url::parse(bad).unwrap()), "{bad}");
        }
        // A page may never be one of the worker's addresses, so it is never scripted either.
        assert!(url_allowed(&Url::parse("https://example.com/").unwrap()));
        assert!(!url_allowed(&Url::parse("bowser://settings").unwrap()));
        assert!(!url_allowed(&Url::parse("file:///C:/a.html").unwrap()));
        assert!(!url_allowed(&Url::parse("https://bowser-translate.invalid/worker.html").unwrap()), "the worker host is never translated");
    }

    #[test]
    fn command_labels_name_the_target() {
        assert_eq!(command_label("en", false), "Translate page to English");
        assert_eq!(command_label("fr", false), "Translate page to French");
        assert_eq!(command_label("xx", false), "Translate page to xx");
        assert_eq!(command_label("en", true), "Show original");
    }

    #[test]
    fn the_embedded_files_are_what_the_contract_says() {
        assert!(SCRIPT.trim_start().starts_with("(function (o) {"));
        assert!(SCRIPT.trim_end().ends_with("})"));
        // The page-world script must not reach for dynamic code, the network, storage or Bowser.
        for banned in [
            "eval(", "new Function", "innerHTML", "outerHTML", "insertAdjacentHTML", "document.write",
            "fetch(", "XMLHttpRequest", "sendBeacon", "WebSocket", "localStorage", "sessionStorage",
            "document.cookie", "postMessage", "import(", "chrome.webview", "addEventListener", "MutationObserver",
            "setTimeout", "setInterval",
        ] {
            assert!(!SCRIPT.contains(banned), "translate.js must not contain {banned}");
        }
        // The worker page can reach nothing but its own origin.
        assert!(WORKER_HTML.contains("default-src 'none'"));
        assert!(WORKER_HTML.contains("script-src 'self' 'wasm-unsafe-eval'"));
        assert!(WORKER_HTML.contains("connect-src 'self'"));
        assert!(!WORKER_HTML.contains("unsafe-inline") && !WORKER_HTML.contains("'unsafe-eval'"));
        assert!(!WORKER_HTML.contains("http:") && !WORKER_HTML.contains("https:"));
        for banned in ["eval(", "new Function", "innerHTML", "XMLHttpRequest", "sendBeacon", "WebSocket", "http://", "https://", "importScripts", "new Worker"] {
            assert!(!WORKER_JS.contains(banned), "worker.js must not contain {banned}");
        }
    }

    #[test]
    fn translation_state_is_session_only_and_the_logic_touches_no_files() {
        // Translating works in a private tab, so nothing this module decides is written anywhere:
        // the production half of this file has no file API and no store of choices.
        let source = include_str!("mod.rs");
        let production = source.split("#[cfg(test)]").next().unwrap();
        for banned in ["std::fs", "fs::read", "fs::write", "File::", "OpenOptions", "std::io::Write", "write_all"] {
            assert!(!production.contains(banned), "translate/mod.rs must not use {banned}");
        }
    }
}
