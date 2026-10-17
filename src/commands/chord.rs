//! A keyboard chord as text ("Ctrl+Shift+K") and as a virtual-key code with modifier state.
//!
//! Pure. The text form is what the keyboard editor shows, `keybindings.txt` stores and the palette
//! prints; [`Chord::parse`] accepts exactly what [`Chord::format`] writes (modifier order Ctrl, Alt,
//! Shift, then the key), plus any spelling of the same modifiers.

/// One chord: a virtual-key code and exactly these modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    pub key: u32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Why a chord cannot be assigned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The editing, clipboard and window chords the page or Windows keeps.
    Reserved,
    /// Ctrl+Alt with a typing key is AltGr on many layouts; it would swallow typing.
    AltGr,
    /// A letter, digit or punctuation key on its own: only while single-key shortcuts are on.
    SingleKey,
}

impl Refusal {
    pub fn message(self) -> &'static str {
        match self {
            Self::Reserved => "That shortcut is kept for editing and Windows.",
            Self::AltGr => "Ctrl+Alt with a typing key is AltGr on many keyboards, so it cannot be a shortcut.",
            Self::SingleKey => "Turn on single-key shortcuts to use a key on its own.",
        }
    }
}

const NAMED: [(&str, u32); 32] = [
    ("Backspace", 0x08),
    ("Tab", 0x09),
    ("Enter", 0x0D),
    ("Esc", 0x1B),
    ("Space", 0x20),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
    ("End", 0x23),
    ("Home", 0x24),
    ("Left", 0x25),
    ("Up", 0x26),
    ("Right", 0x27),
    ("Down", 0x28),
    ("Insert", 0x2D),
    ("Delete", 0x2E),
    ("Numpad0", 0x60),
    ("Numpad1", 0x61),
    ("Numpad2", 0x62),
    ("Numpad3", 0x63),
    ("Numpad4", 0x64),
    ("Numpad5", 0x65),
    ("Numpad6", 0x66),
    ("Numpad7", 0x67),
    ("Numpad8", 0x68),
    ("Numpad9", 0x69),
    ("Numpad*", 0x6A),
    ("Numpad+", 0x6B),
    ("Numpad-", 0x6D),
    ("Numpad.", 0x6E),
    ("Numpad/", 0x6F),
    ("BrowserBack", 0xA6),
    ("BrowserForward", 0xA7),
];
const NAMED_MORE: [(&str, u32); 6] = [
    ("BrowserRefresh", 0xA8),
    ("BrowserStop", 0xA9),
    ("BrowserSearch", 0xAA),
    ("BrowserHome", 0xAC),
    ("Menu", 0x5D),
    ("PrintScreen", 0x2C),
];
/// OEM punctuation keys on a US layout (what the existing table already uses: `,` and `\`).
const PUNCT: [(&str, u32); 11] = [
    (";", 0xBA),
    ("=", 0xBB),
    (",", 0xBC),
    ("-", 0xBD),
    (".", 0xBE),
    ("/", 0xBF),
    ("`", 0xC0),
    ("[", 0xDB),
    ("\\", 0xDC),
    ("]", 0xDD),
    ("'", 0xDE),
];

impl Chord {
    pub const fn new(key: u32, ctrl: bool, shift: bool, alt: bool) -> Self {
        Self { key, ctrl, shift, alt }
    }

    /// The text for the key alone ("K", "F5", "Left", ","), `None` for a code Bowser has no name for.
    pub fn key_name(key: u32) -> Option<String> {
        match key {
            0x30..=0x39 | 0x41..=0x5A => Some(char::from(key as u8).to_string()),
            0x70..=0x87 => Some(format!("F{}", key - 0x6F)),
            _ => NAMED
                .iter()
                .chain(NAMED_MORE.iter())
                .chain(PUNCT.iter())
                .find(|(_, code)| *code == key)
                .map(|(name, _)| (*name).to_owned()),
        }
    }

    fn key_code(name: &str) -> Option<u32> {
        let mut chars = name.chars();
        if let (Some(first), None) = (chars.next(), chars.clone().next()) {
            let upper = first.to_ascii_uppercase();
            if upper.is_ascii_uppercase() || upper.is_ascii_digit() {
                return Some(upper as u32);
            }
        }
        if let Some(number) = name.strip_prefix(['F', 'f']).and_then(|n| n.parse::<u32>().ok())
            && (1..=24).contains(&number)
        {
            return Some(0x6F + number);
        }
        let lower = name.to_ascii_lowercase();
        let found = NAMED
            .iter()
            .chain(NAMED_MORE.iter())
            .chain(PUNCT.iter())
            .find(|(candidate, _)| candidate.to_ascii_lowercase() == lower)
            .map(|(_, code)| *code);
        found.or(match lower.as_str() {
            "escape" => Some(0x1B),
            "del" => Some(0x2E),
            "pgup" => Some(0x21),
            "pgdn" => Some(0x22),
            "return" => Some(0x0D),
            "plus" | "+" => Some(0xBB),
            "minus" => Some(0xBD),
            "comma" => Some(0xBC),
            _ => None,
        })
    }

    /// "Ctrl+Alt+Shift+K". `None` when the key has no name.
    pub fn format(&self) -> Option<String> {
        let key = Self::key_name(self.key)?;
        let mut out = String::new();
        for (on, name) in [(self.ctrl, "Ctrl"), (self.alt, "Alt"), (self.shift, "Shift")] {
            if on {
                out.push_str(name);
                out.push('+');
            }
        }
        out.push_str(&key);
        Some(out)
    }

    /// Parses "Ctrl+Shift+K", "alt+left", "F5", "Ctrl+,", "Ctrl++" (Ctrl and the plus key).
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() || text.len() > 40 {
            return None;
        }
        // The key is the first tail after a `+` (or the whole text) that names a key, so keys whose own
        // name holds a plus ("Numpad+", "+") still parse.
        let splits = std::iter::once(0).chain(text.match_indices('+').map(|(at, _)| at + 1));
        for start in splits {
            let (head, key_text) = if start == 0 { ("", text) } else { (&text[..start - 1], &text[start..]) };
            let Some(key) = Self::key_code(key_text) else { continue };
            let mut chord = Self::new(key, false, false, false);
            let mut valid = true;
            if start > 0 {
                for part in head.split('+') {
                    match part.trim().to_ascii_lowercase().as_str() {
                        "ctrl" | "control" if !chord.ctrl => chord.ctrl = true,
                        "shift" if !chord.shift => chord.shift = true,
                        "alt" if !chord.alt => chord.alt = true,
                        _ => valid = false,
                    }
                }
            }
            if valid {
                return Some(chord);
            }
        }
        None
    }

    /// A typing key (letter, digit, punctuation, space, Enter, Tab) without Ctrl or Alt: using it as
    /// a shortcut competes with typing. Shift alone does not make a typing key safe.
    pub fn is_single_key(&self) -> bool {
        !self.ctrl && !self.alt && self.is_typing_key()
    }

    fn is_typing_key(&self) -> bool {
        matches!(self.key, 0x30..=0x39 | 0x41..=0x5A | 0x20 | 0x0D | 0x09 | 0x08 | 0xBA..=0xC0 | 0xDB..=0xDE)
    }

    /// Whether `self` may be assigned. `single_keys` is the "single-key shortcuts" setting.
    pub fn check_assignable(&self, single_keys: bool) -> Result<(), Refusal> {
        const KEYS: [u32; 6] = [0x41, 0x43, 0x56, 0x58, 0x59, 0x5A]; // A C V X Y Z
        if self.ctrl && !self.alt && !self.shift && KEYS.contains(&self.key) {
            return Err(Refusal::Reserved);
        }
        if !self.ctrl && !self.shift && !self.alt && self.key == 0x1B {
            return Err(Refusal::Reserved); // Esc cancels things in the page and the address bar
        }
        if self.alt && !self.ctrl && !self.shift && self.key == 0x73 {
            return Err(Refusal::Reserved); // Alt+F4
        }
        if self.shift && !self.ctrl && !self.alt && self.key == 0x79 {
            return Err(Refusal::Reserved); // Shift+F10, the page's context menu
        }
        if self.ctrl && self.alt && self.is_typing_key() {
            return Err(Refusal::AltGr);
        }
        if self.is_single_key() && !single_keys {
            return Err(Refusal::SingleKey);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_and_codes_round_trip_for_every_nameable_key_and_modifier_set() {
        for key in 0..=0xFF_u32 {
            let Some(name) = Chord::key_name(key) else { continue };
            for modifiers in 0..8_u8 {
                let chord = Chord::new(key, modifiers & 1 != 0, modifiers & 2 != 0, modifiers & 4 != 0);
                let text = chord.format().expect("a named key formats");
                assert_eq!(Chord::parse(&text), Some(chord), "{text} ({name})");
            }
        }
    }

    /// Every name the keyboard editor can capture (`ui/test/fixtures/chord-names.txt`, also checked by
    /// `ui/test/commands.mjs`) is a key Rust reads and writes back unchanged, alone and with modifiers.
    #[test]
    fn every_key_name_the_editor_captures_parses_and_round_trips() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/test/fixtures/chord-names.txt");
        let text = std::fs::read_to_string(path).expect("chord names");
        let mut count = 0;
        for line in text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let name = line.split('\t').next().unwrap();
            let chord = Chord::parse(name).unwrap_or_else(|| panic!("{name} parses"));
            assert_eq!(chord.format().as_deref(), Some(name), "{name} writes back as itself");
            let with = format!("Ctrl+Alt+Shift+{name}");
            assert_eq!(Chord::parse(&with).and_then(|c| c.format()).as_deref(), Some(with.as_str()), "{with}");
            count += 1;
        }
        assert!(count >= 100, "{count}");
    }

    #[test]
    fn existing_accelerator_strings_parse_to_their_bindings() {
        assert_eq!(Chord::parse("Ctrl+T"), Some(Chord::new(0x54, true, false, false)));
        assert_eq!(Chord::parse("Ctrl+Shift+P"), Some(Chord::new(0x50, true, true, false)));
        assert_eq!(Chord::parse("Alt+Left"), Some(Chord::new(0x25, false, false, true)));
        assert_eq!(Chord::parse("Ctrl+Alt+R"), Some(Chord::new(0x52, true, false, true)));
        assert_eq!(Chord::parse("Ctrl+,"), Some(Chord::new(0xBC, true, false, false)));
        assert_eq!(Chord::parse("Ctrl+Shift+\\"), Some(Chord::new(0xDC, true, true, false)));
        assert_eq!(Chord::parse("F12"), Some(Chord::new(0x7B, false, false, false)));
        assert_eq!(Chord::parse("ctrl+shift+delete"), Some(Chord::new(0x2E, true, true, false)));
        assert_eq!(Chord::parse("Ctrl++"), Some(Chord::new(0xBB, true, false, false)));
        assert_eq!(Chord::parse("Ctrl+Numpad+"), Some(Chord::new(0x6B, true, false, false)));
        assert_eq!(Chord::parse("+"), Some(Chord::new(0xBB, false, false, false)));
        assert_eq!(Chord::parse("Ctrl+Alt+Shift+K").and_then(|c| c.format()), Some("Ctrl+Alt+Shift+K".to_owned()));
    }

    #[test]
    fn malformed_text_is_refused() {
        for bad in ["", "Ctrl+", "Ctrl+Ctrl+T", "Foo+T", "Ctrl+Nope", "Ctrl+Shift", "T+Ctrl", "Ctrl+F25", "Ctrl+F0", &"x".repeat(60)] {
            assert_eq!(Chord::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn editing_window_altgr_and_single_key_rules() {
        let ok = |text: &str, single: bool| Chord::parse(text).unwrap().check_assignable(single);
        assert_eq!(ok("Ctrl+C", false), Err(Refusal::Reserved));
        assert_eq!(ok("Ctrl+Z", true), Err(Refusal::Reserved));
        assert_eq!(ok("Alt+F4", true), Err(Refusal::Reserved));
        assert_eq!(ok("Shift+F10", true), Err(Refusal::Reserved));
        assert_eq!(ok("Ctrl+Alt+K", true), Err(Refusal::AltGr));
        assert_eq!(ok("K", false), Err(Refusal::SingleKey));
        assert_eq!(ok("Shift+K", false), Err(Refusal::SingleKey));
        assert_eq!(ok("K", true), Ok(()));
        assert_eq!(ok("F8", false), Ok(()), "function keys are not typing keys");
        assert_eq!(ok("Esc", false), Err(Refusal::Reserved));
        assert_eq!(ok("Shift+Esc", false), Ok(()));
        assert_eq!(ok("Ctrl+Shift+C", false), Ok(()));
        assert_eq!(ok("Ctrl+Alt+Left", false), Ok(()), "arrows are not typing keys");
        assert_eq!(ok("Ctrl+Alt+R", false), Err(Refusal::AltGr), "the default is allowed by being a default, not by the check");
    }
}
