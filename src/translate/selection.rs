//! Translation of the text a person selects and right-clicks.
//!
//! When the page's context menu opens on a selection that looks like it is in another language,
//! the menu gets one more row at the top. The row says "Translating…" while one request goes to
//! Google Translate on a background thread, then shows the translation. Choosing the row copies the
//! translation. **The selected text is sent to Google**, so this only happens for a right-click on a
//! selection, never in a private tab, and only while the `translate_offer` setting is on.
//!
//! [`looks_foreign`] decides without asking anyone, from the letters' writing system and the page's
//! declared language. There is one lookup at a time; a new one replaces the last.

use std::sync::{
    Mutex, MutexGuard,
    atomic::{AtomicU64, Ordering},
};

use super::{Lang, google::{self, GoogleError}, language_name};

/// The longest selection that gets a translation row, in characters.
pub const MAX_CHARS: usize = 2000;

/// A writing system, as far as telling languages apart needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Script {
    Latin,
    Greek,
    Cyrillic,
    Armenian,
    Hebrew,
    Arabic,
    /// One of the Brahmic blocks from U+0900 (Devanagari) to U+0DFF (Sinhala), by block number.
    Indic(u8),
    Thai,
    Georgian,
    Hangul,
    Kana,
    Han,
    Other,
}

fn script(letter: char) -> Script {
    match letter as u32 {
        0x41..=0x5A | 0x61..=0x7A | 0xC0..=0x24F | 0x1E00..=0x1EFF => Script::Latin,
        0x370..=0x3FF | 0x1F00..=0x1FFF => Script::Greek,
        0x400..=0x52F => Script::Cyrillic,
        0x530..=0x58F => Script::Armenian,
        0x590..=0x5FF => Script::Hebrew,
        0x600..=0x6FF | 0x750..=0x77F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => Script::Arabic,
        code @ 0x900..=0xDFF => Script::Indic(((code - 0x900) / 0x80) as u8),
        0xE00..=0xE7F => Script::Thai,
        0x10A0..=0x10FF => Script::Georgian,
        0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF => Script::Hangul,
        0x3040..=0x30FF => Script::Kana,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF => Script::Han,
        _ => Script::Other,
    }
}

/// Whether `language` (a primary subtag) is normally written in `script`. A language not listed is
/// taken to be written in Latin letters.
fn writes(language: &str, script: Script) -> bool {
    match language {
        "ru" | "uk" | "bg" | "mk" | "be" | "kk" => script == Script::Cyrillic,
        "sr" => matches!(script, Script::Cyrillic | Script::Latin),
        "el" => script == Script::Greek,
        "hy" => script == Script::Armenian,
        "he" | "yi" => script == Script::Hebrew,
        "ar" | "fa" | "ur" | "ps" => script == Script::Arabic,
        "hi" | "mr" | "ne" => script == Script::Indic(0),
        "bn" => script == Script::Indic(1),
        "pa" => script == Script::Indic(2),
        "gu" => script == Script::Indic(3),
        "ta" => script == Script::Indic(5),
        "te" => script == Script::Indic(6),
        "kn" => script == Script::Indic(7),
        "ml" => script == Script::Indic(8),
        "th" => script == Script::Thai,
        "ka" => script == Script::Georgian,
        "ko" => script == Script::Hangul,
        "ja" => matches!(script, Script::Kana | Script::Han),
        "zh" => script == Script::Han,
        _ => script == Script::Latin,
    }
}

/// Whether `text` looks like it is not in `target`. Most of its letters must be in a writing system
/// `target` does not use (Russian or Japanese text for an English reader), or the page must declare a
/// language other than `target` that is written in those letters (French text on a French page).
/// Text with no letters (numbers, symbols) never does. A passage in another language that uses the
/// same letters on a page in the target language is not caught.
pub fn looks_foreign(text: &str, page_lang: Lang, target: &str) -> bool {
    let page = (!page_lang.is_none() && page_lang.as_str() != target).then(|| page_lang.as_str());
    let (mut letters, mut other_script, mut page_script) = (0usize, 0usize, 0usize);
    for letter in text.chars().filter(|c| c.is_alphabetic()) {
        let script = script(letter);
        letters += 1;
        other_script += usize::from(!writes(target, script));
        page_script += usize::from(page.is_some_and(|page| writes(page, script)));
    }
    letters > 0 && (other_script * 2 > letters || page_script * 2 > letters)
}

struct Lookup {
    id: u64,
    text: String,
    target: String,
    answer: Option<Result<String, GoogleError>>,
}

static CURRENT: Mutex<Option<Lookup>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(1);

fn current() -> MutexGuard<'static, Option<Lookup>> {
    CURRENT.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Starts translating `text` into `target` on a background thread and returns the lookup's id. The
/// same text and target as a lookup that worked reuses its answer instead of asking again.
pub fn start(text: String, target: String) -> u64 {
    let mut slot = current();
    if let Some(lookup) = slot.as_ref().filter(|lookup| lookup.text == text && lookup.target == target && !matches!(lookup.answer, Some(Err(_)))) {
        return lookup.id;
    }
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    *slot = Some(Lookup { id, text: text.clone(), target: target.clone(), answer: None });
    drop(slot);
    let spawned = std::thread::Builder::new().name("bowser-translate-selection".into()).spawn(move || {
        let answer = google::translate("auto", &target, &[text]).and_then(|mut out| out.pop().ok_or(GoogleError::Bad));
        if let Some(lookup) = current().as_mut().filter(|lookup| lookup.id == id) {
            lookup.answer = Some(answer);
        }
    });
    if spawned.is_err()
        && let Some(lookup) = current().as_mut().filter(|lookup| lookup.id == id)
    {
        lookup.answer = Some(Err(GoogleError::Network));
    }
    id
}

/// Whether lookup `id` is still waiting for Google.
pub fn loading(id: u64) -> bool {
    current().as_ref().is_some_and(|lookup| lookup.id == id && lookup.answer.is_none())
}

/// The translation of lookup `id`, once it has arrived.
pub fn translation(id: u64) -> Option<String> {
    match current().as_ref().filter(|lookup| lookup.id == id)?.answer.as_ref()? {
        Ok(text) => Some(text.clone()),
        Err(_) => None,
    }
}

/// The menu row's text for lookup `id`: "Translating…", "English: <translation>" on one line, or why
/// there is no translation.
pub fn label(id: u64) -> String {
    let slot = current();
    let Some(lookup) = slot.as_ref().filter(|lookup| lookup.id == id) else {
        return "Translation unavailable".to_owned();
    };
    match &lookup.answer {
        None => "Translating…".to_owned(),
        Some(Ok(text)) => format!("{}: {}", language_name(&lookup.target), text.split_whitespace().collect::<Vec<_>>().join(" ")),
        Some(Err(GoogleError::Limited)) => "Google Translate is limiting requests".to_owned(),
        Some(Err(GoogleError::Network)) => "Google Translate could not be reached".to_owned(),
        Some(Err(GoogleError::Bad)) => "Translation unavailable".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang(code: &str) -> Lang {
        Lang::new(code).unwrap()
    }

    #[test]
    fn another_writing_system_is_foreign() {
        assert!(looks_foreign("Привет, мир", Lang::NONE, "en"));
        assert!(looks_foreign("こんにちは世界", lang("en"), "en"));
        assert!(looks_foreign("안녕하세요", Lang::NONE, "en"));
        assert!(looks_foreign("مرحبا بالعالم", Lang::NONE, "fr"));
        assert!(looks_foreign("Hello world", Lang::NONE, "ru"));
        assert!(looks_foreign("नमस्ते दुनिया", Lang::NONE, "bn"));
        assert!(looks_foreign("ひらがな", Lang::NONE, "zh"));
    }

    #[test]
    fn the_target_writing_system_is_not_foreign_on_its_own() {
        assert!(!looks_foreign("Hello world", Lang::NONE, "en"));
        assert!(!looks_foreign("Bonjour le monde", lang("en"), "en"));
        assert!(!looks_foreign("Привет", Lang::NONE, "uk"));
        assert!(!looks_foreign("漢字とかな", Lang::NONE, "ja"));
        assert!(!looks_foreign("Zdravo svete", Lang::NONE, "sr"));
        assert!(!looks_foreign("नमस्ते", Lang::NONE, "hi"));
    }

    #[test]
    fn a_page_in_another_language_makes_its_letters_foreign() {
        assert!(looks_foreign("Bonjour le monde", lang("fr"), "en"));
        assert!(looks_foreign("漢字", lang("ja"), "zh"));
        // The page's language does not count when it is the target.
        assert!(!looks_foreign("Bonjour", lang("en"), "en"));
        // Nor does it make letters it does not use foreign.
        assert!(!looks_foreign("Hello", lang("ru"), "en"));
    }

    #[test]
    fn text_without_letters_is_never_foreign() {
        assert!(!looks_foreign("", lang("fr"), "en"));
        assert!(!looks_foreign("12 345,67 — %", lang("fr"), "en"));
    }

    #[test]
    fn mostly_target_letters_with_a_foreign_word_is_not_foreign() {
        assert!(!looks_foreign("The word for cat is кот", Lang::NONE, "en"));
        assert!(looks_foreign("Кот это cat", Lang::NONE, "en"));
    }

    #[test]
    fn indic_blocks_are_numbered_from_devanagari() {
        assert_eq!(script('क'), Script::Indic(0));
        assert_eq!(script('ক'), Script::Indic(1));
        assert_eq!(script('த'), Script::Indic(5));
        assert_eq!(script('ക'), Script::Indic(8));
    }
}
