//! Strong password generation with per-class guarantees, and a local strength estimate.
//!
//! The generator draws from the Windows system CSPRNG (`generate_password`), so every character is
//! uniform over the chosen set; a result that misses a requested class is drawn again rather than
//! patched, which keeps the output uniform over all passwords that contain every class. The
//! strength estimate never leaves the process and never stores what it scored.

use super::{PasswordError, SecretString, generate_password};

const LOWER: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
const UPPER: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &[u8] = b"0123456789";
/// Symbols most sign-up forms accept. Quotes, backslash and spaces are left out on purpose.
const SYMBOLS: &[u8] = b"!#$%&()*+,-./:;<=>?@[]^_{|}~";
/// Characters that are easy to misread when a password is typed by hand.
const AMBIGUOUS: &[u8] = b"Il1O0o|";

pub const MIN_LENGTH: usize = 8;
pub const MAX_LENGTH: usize = 128;
pub const DEFAULT_LENGTH: usize = 20;

/// What the generator may use. At least one class must be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeneratorOptions {
    pub length: usize,
    pub lower: bool,
    pub upper: bool,
    pub digits: bool,
    pub symbols: bool,
    pub avoid_ambiguous: bool,
}

impl Default for GeneratorOptions {
    fn default() -> Self {
        Self { length: DEFAULT_LENGTH, lower: true, upper: true, digits: true, symbols: true, avoid_ambiguous: true }
    }
}

impl GeneratorOptions {
    /// Parses the compact wire form `"<length>:<classes>[:a]"`, where classes is any of `l`, `u`,
    /// `d`, `s` and a trailing `:a` avoids look-alike characters, e.g. `"20:luds:a"`.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split(':');
        let length = parts.next()?.parse::<usize>().ok()?;
        let classes = parts.next()?;
        let avoid_ambiguous = match parts.next() {
            None => false,
            Some("a") => true,
            Some(_) => return None,
        };
        if parts.next().is_some() || !(MIN_LENGTH..=MAX_LENGTH).contains(&length) {
            return None;
        }
        let unique = classes.bytes().enumerate().all(|(i, b)| !classes.as_bytes()[..i].contains(&b));
        if classes.is_empty() || classes.len() > 4 || !unique || !classes.bytes().all(|b| b"luds".contains(&b)) {
            return None;
        }
        let has = |c: u8| classes.bytes().any(|b| b == c);
        Some(Self { length, lower: has(b'l'), upper: has(b'u'), digits: has(b'd'), symbols: has(b's'), avoid_ambiguous })
    }

    fn classes(&self) -> Vec<Vec<u8>> {
        let keep = |set: &[u8]| -> Vec<u8> {
            set.iter().copied().filter(|c| !(self.avoid_ambiguous && AMBIGUOUS.contains(c))).collect()
        };
        let mut classes = Vec::new();
        for (on, set) in [(self.lower, LOWER), (self.upper, UPPER), (self.digits, DIGITS), (self.symbols, SYMBOLS)] {
            if on {
                classes.push(keep(set));
            }
        }
        classes
    }

    /// The character set the options draw from, unique and printable.
    pub fn charset(&self) -> Vec<u8> {
        self.classes().concat()
    }
}

/// Generates a password from `options` that contains at least one character of every chosen class.
pub fn generate_strong(options: GeneratorOptions) -> Result<SecretString, PasswordError> {
    let classes = options.classes();
    if classes.is_empty() || options.length < classes.len() || !(MIN_LENGTH..=MAX_LENGTH).contains(&options.length) {
        return Err(PasswordError::InvalidGenerator);
    }
    let charset = options.charset();
    // With 8+ characters and at most four classes the chance of a miss is small; 64 draws make a
    // failure astronomically unlikely, and it is reported rather than weakened.
    for _ in 0..64 {
        let candidate = generate_password(options.length, &charset)?;
        if classes.iter().all(|class| candidate.bytes().any(|b| class.contains(&b))) {
            return Ok(candidate);
        }
    }
    Err(PasswordError::InvalidGenerator)
}

/// A coarse 0 (very weak) to 4 (very strong) score, the same scale the settings page draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Strength(pub u8);

impl Strength {
    pub fn is_weak(self) -> bool {
        self.0 < 2
    }
    pub fn label(self) -> &'static str {
        match self.0 {
            0 => "Very weak",
            1 => "Weak",
            2 => "Fair",
            3 => "Strong",
            _ => "Very strong",
        }
    }
}

/// Very common passwords and their stems. Any password that is one of these, or one of these
/// followed by digits or symbols, scores 0.
const COMMON: &[&str] = &[
    "password", "passw0rd", "123456", "12345678", "qwerty", "abc123", "letmein", "welcome", "monkey",
    "dragon", "football", "baseball", "iloveyou", "admin", "login", "master", "sunshine", "princess",
    "shadow", "trustno1", "superman", "michael", "starwars", "whatever", "freedom", "hello", "secret",
    "charlie", "computer", "qwertyuiop", "asdfgh", "zxcvbn", "changeme", "default", "summer", "winter",
    "spring", "autumn", "pokemon", "batman", "liverpool", "chelsea", "arsenal",
];

/// Estimates strength from the character pool and length, less what predictable structure gives
/// away: runs, sequences, keyboard rows and common words.
pub fn strength(password: &str) -> Strength {
    let chars: Vec<char> = password.chars().collect();
    if chars.is_empty() {
        return Strength(0);
    }
    let lower = password.to_lowercase();
    let stem = lower.trim_end_matches(|c: char| c.is_ascii_digit() || c.is_ascii_punctuation());
    let leet: String = stem
        .chars()
        .map(|c| match c {
            '0' => 'o',
            '1' | '!' => 'i',
            '3' => 'e',
            '4' | '@' => 'a',
            '5' | '$' => 's',
            '7' => 't',
            c => c,
        })
        .collect();
    if COMMON.iter().any(|word| lower == *word || stem == *word || leet == *word) || chars.len() < 6 {
        return Strength(0);
    }
    let mut pool = 0u32;
    if chars.iter().any(char::is_ascii_lowercase) {
        pool += 26;
    }
    if chars.iter().any(char::is_ascii_uppercase) {
        pool += 26;
    }
    if chars.iter().any(char::is_ascii_digit) {
        pool += 10;
    }
    if chars.iter().any(|c| c.is_ascii_punctuation() || *c == ' ') {
        pool += 33;
    }
    if chars.iter().any(|c| !c.is_ascii()) {
        pool += 64;
    }
    // Characters that repeat the previous one, or continue an ascending/descending run or a
    // keyboard row, add almost nothing.
    const ROWS: [&str; 4] = ["qwertyuiop", "asdfghjkl", "zxcvbnm", "1234567890"];
    let mut effective = 1.0f64;
    for pair in chars.windows(2) {
        let (a, b) = (pair[0].to_ascii_lowercase(), pair[1].to_ascii_lowercase());
        let step = (b as i64) - (a as i64);
        let keyboard = ROWS.iter().any(|row| {
            row.find(a).zip(row.find(b)).is_some_and(|(x, y)| x.abs_diff(y) == 1)
        });
        effective += if a == b {
            0.1
        } else if step.abs() == 1 || keyboard {
            0.25
        } else {
            1.0
        };
    }
    // A password that contains a common word is mostly that word.
    for word in COMMON.iter().filter(|word| word.len() >= 4) {
        if leet.contains(word) || lower.contains(word) {
            effective -= (word.len() as f64) * 0.75;
        }
    }
    let unique = {
        let mut seen = chars.clone();
        seen.sort_unstable();
        seen.dedup();
        seen.len() as f64
    };
    // Few distinct characters cap the estimate however long the password is.
    let effective = effective.max(1.0).min(unique * 2.5);
    let bits = effective * f64::from(pool.max(1)).log2();
    Strength(match bits {
        b if b < 28.0 => 0,
        b if b < 40.0 => 1,
        b if b < 60.0 => 2,
        b if b < 80.0 => 3,
        _ => 4,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_parse_the_compact_wire_form_and_refuse_anything_else() {
        assert_eq!(GeneratorOptions::parse("20:luds:a"), Some(GeneratorOptions::default()));
        let digits = GeneratorOptions::parse("12:d").unwrap();
        assert!(digits.digits && !digits.lower && !digits.upper && !digits.symbols && !digits.avoid_ambiguous);
        for bad in ["", "20", "7:luds", "129:l", "20:", "20:x", "20:lll", "20:luds:b", "20:luds:a:x", "-1:l", "20:ludsl"] {
            assert_eq!(GeneratorOptions::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_charset_is_unique_printable_and_drops_look_alikes_on_request() {
        let options = GeneratorOptions::default();
        let set = options.charset();
        let mut sorted = set.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), set.len());
        assert!(set.iter().all(|b| (b'!'..=b'~').contains(b)));
        assert!(!set.iter().any(|b| AMBIGUOUS.contains(b)));
        let all = GeneratorOptions { avoid_ambiguous: false, ..options }.charset();
        assert!(all.contains(&b'0') && all.contains(&b'l') && all.len() == 26 + 26 + 10 + SYMBOLS.len());
    }

    #[test]
    fn strength_scores_common_short_and_patterned_passwords_low() {
        for weak in ["password", "Password1!", "P@ssw0rd", "123456789", "qwertyuiop", "aaaaaaaaaaaa", "abc", "letmein2024"] {
            assert!(strength(weak).is_weak(), "{weak} scored {:?}", strength(weak));
        }
        for strong in ["vT7#qL9!mZ2@xR4$", "correct-horse-battery-staple-91", "Xk3$9pQz!vL2mN8&wR5t"] {
            assert!(strength(strong).0 >= 3, "{strong} scored {:?}", strength(strong));
        }
        assert_eq!(strength(""), Strength(0));
        assert_eq!(Strength(4).label(), "Very strong");
    }

    #[cfg(windows)]
    #[test]
    fn generated_passwords_hold_every_requested_class() {
        for _ in 0..50 {
            let password = generate_strong(GeneratorOptions::default()).unwrap();
            assert_eq!(password.len(), DEFAULT_LENGTH);
            assert!(password.bytes().any(|b| b.is_ascii_lowercase()));
            assert!(password.bytes().any(|b| b.is_ascii_uppercase()));
            assert!(password.bytes().any(|b| b.is_ascii_digit()));
            assert!(password.bytes().any(|b| SYMBOLS.contains(&b)));
            assert!(strength(&password).0 >= 3);
        }
        assert!(generate_strong(GeneratorOptions { lower: false, upper: false, digits: false, symbols: false, ..Default::default() }).is_err());
    }
}
