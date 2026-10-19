//! Remove only redundant existing WebView2 registration globs, never synthesize new ones.
//!
//! The registration URI is a whole-string glob (`*`, `?`, and escaped `*`/`?`), per
//! https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2.addwebresourcerequestedfilter
//! This is a conservative sufficient cover proof, not a general glob-language solver.

const MAX_TOKENS: usize = 512;
const MAX_PROOF_WORK: usize = 8_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Token {
    Star,
    One,
    Literal(u8),
}

fn tokens(pattern: &str) -> Option<Vec<Token>> {
    if !pattern.is_ascii() || pattern.bytes().any(|b| b.is_ascii_control()) || pattern.len() > MAX_TOKENS { return None; }
    let mut result = Vec::new();
    let mut bytes = pattern.bytes();
    while let Some(byte) = bytes.next() {
        let token = match byte {
            b'*' => Token::Star,
            b'?' => Token::One,
            // Only the two documented wildcard escapes are proved. Unknown syntax stays.
            b'\\' => match bytes.next()? {
                byte @ (b'*' | b'?') => Token::Literal(byte),
                _ => return None,
            },
            byte => Token::Literal(byte),
        };
        if token != Token::Star || result.last() != Some(&Token::Star) { result.push(token); }
    }
    Some(result)
}

/// Every expansion of a narrow token is covered by its aligned broad token. A broad star
/// can cover any whole narrow-token sequence, including unbounded narrow stars. A broad
/// question can cover one narrow literal/question, NEVER a narrow star. This fixed token
/// alignment proves inclusion for every URI, not just the sampled URL corpus.
fn covers(broad: &[Token], narrow: &[Token], budget: &mut usize) -> Option<bool> {
    let work = broad.len().checked_add(1)?.checked_mul(narrow.len().checked_add(1)?)?;
    if work > *budget { return None; }
    *budget -= work;
    let mut previous = vec![false; narrow.len() + 1];
    previous[0] = true;
    for &token in broad {
        let mut next = vec![false; narrow.len() + 1];
        if token == Token::Star {
            next[0] = previous[0];
            for index in 1..next.len() { next[index] = previous[index] || next[index - 1]; }
        } else {
            for index in 1..next.len() {
                let aligned = match (token, narrow[index - 1]) {
                    (Token::One, Token::One | Token::Literal(_)) => true,
                    (Token::Literal(a), Token::Literal(b)) => a == b,
                    _ => false,
                };
                next[index] = previous[index - 1] && aligned;
            }
        }
        previous = next;
    }
    previous.last().copied()
}

pub(super) fn minimize(patterns: Vec<String>) -> Vec<String> {
    minimize_with_budget(patterns, MAX_PROOF_WORK)
}

fn minimize_with_budget(mut original: Vec<String>, mut budget: usize) -> Vec<String> {
    original.sort();
    original.dedup();
    let parsed: Vec<_> = original.iter().map(|pattern| tokens(pattern)).collect();
    let mut order: Vec<_> = (0..original.len()).collect();
    // Broad short patterns normally come first. Ordering is an efficiency choice only;
    // every deletion still needs a proof from a permanently retained existing witness.
    order.sort_by_key(|&index| {
        let rank = parsed[index].as_ref().map(|tokens| {
            (tokens.iter().filter(|&&t| t != Token::Star).count(),
             usize::MAX - tokens.iter().filter(|&&t| t == Token::Star).count(), tokens.len())
        }).unwrap_or((usize::MAX, usize::MAX, usize::MAX));
        (rank, index)
    });
    let mut retained: Vec<usize> = Vec::new();
    for index in order {
        let mut redundant = false;
        if let Some(narrow) = parsed[index].as_ref() {
            for &witness in &retained {
                let Some(broad) = parsed[witness].as_ref() else { continue; };
                match covers(broad, narrow, &mut budget) {
                    Some(true) => { redundant = true; break; }
                    Some(false) => {}
                    // No partial result is applied when the bounded proof budget runs out.
                    None => return original,
                }
            }
        }
        if !redundant { retained.push(index); }
    }
    // Retained witnesses are never themselves deleted: equivalent cycles cannot remove both.
    retained.sort_unstable();
    retained.into_iter().map(|index| original[index].clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(patterns: &[&str]) -> Vec<String> { patterns.iter().map(|s| (*s).to_owned()).collect() }
    fn proof(broad: &str, narrow: &str) -> bool {
        let mut budget = MAX_PROOF_WORK;
        covers(&tokens(broad).unwrap(), &tokens(narrow).unwrap(), &mut budget).unwrap()
    }
    fn matches(pattern: &[u8], text: &[u8]) -> bool {
        match pattern.split_first() {
            None => text.is_empty(),
            Some((b'*', rest)) => (0..=text.len()).any(|n| matches(rest, &text[n..])),
            Some((b'?', rest)) => !text.is_empty() && matches(rest, &text[1..]),
            Some((b'\\', rest)) if !rest.is_empty() => text.first() == rest.first() && matches(&rest[1..], &text[1..]),
            Some((byte, rest)) => text.first() == Some(byte) && matches(rest, &text[1..]),
        }
    }
    fn words(alphabet: &[&str], max: usize) -> Vec<String> {
        let mut all = vec![String::new()];
        let mut level = vec![String::new()];
        for _ in 0..max {
            level = level.iter().flat_map(|prefix| alphabet.iter().map(move |suffix| format!("{prefix}{suffix}"))).collect();
            all.extend(level.iter().cloned());
        }
        all
    }

    #[test]
    fn conservative_cover_handles_stars_questions_and_escapes() {
        for (broad, narrow) in [("", ""), ("*", ""), ("*", "a*b"), ("a?*", "ab*"), ("*a*", "*ab*"), ("*?*", "*a*"), (r"a\?*", r"a\?b*"), (r"*\**", r"a\*b")] {
            assert!(proof(broad, narrow), "{broad:?} should cover {narrow:?}");
        }
        for (broad, narrow) in [("", "*"), ("?", "*"), ("a?", "a*"), (r"a\?", "a?"), ("ab", "a?")] {
            assert!(!proof(broad, narrow), "{broad:?} must not prove {narrow:?}");
        }
    }

    #[test]
    fn equivalent_cycles_and_transitive_candidates_keep_permanent_witnesses() {
        let original = list(&["a*b", "a**b", "ab", "axxb", "a?b"]);
        let retained = minimize(original.clone());
        assert_eq!(retained.len(), 1);
        for pattern in &original { assert!(retained.iter().any(|w| proof(w, pattern))); }
        assert!(retained.iter().all(|p| original.contains(p)));
        let mut reversed = original; reversed.reverse();
        assert_eq!(minimize(reversed), retained);
    }

    #[test]
    fn unknown_unicode_and_budget_exhaustion_keep_original_filters() {
        let original = list(&["a*", r"a\z", "aé", "a\\", "a?", "a\0b"]);
        let retained = minimize(original.clone());
        for unknown in [r"a\z", "aé", "a\\", "a\0b"] { assert!(retained.contains(&unknown.to_owned())); }
        let mut sorted = original.clone(); sorted.sort(); sorted.dedup();
        assert_eq!(minimize_with_budget(original, 0), sorted);
        let long = "a".repeat(MAX_TOKENS + 1);
        assert_eq!(minimize(vec!["*".to_owned(), long.clone()]), vec!["*".to_owned(), long]);
    }

    #[test]
    fn builtin_reduction_keeps_an_existing_covering_witness_for_every_original() {
        let blocker = crate::blocker::Blocker::from_lists(None);
        let original: Vec<_> = blocker.request_filter_candidates().into_iter().collect();
        let retained = blocker.request_filters();
        println!("builtin registration globs: {} -> {}", original.len(), retained.len());
        assert!(retained.len() < original.len(), "built-in repeated registrations should be reduced");
        assert!(retained.iter().all(|pattern| original.contains(pattern)));
        for pattern in original {
            assert!(retained.iter().any(|witness| proof(witness, &pattern)), "no retained proof for {pattern}");
        }
    }

    #[test]
    fn every_small_proved_relation_preserves_every_enumerated_string() {
        let patterns = words(&["a", "b", "*", "?", r"\?", r"\*"], 3);
        let texts = words(&["a", "b", "?", "*"], 4);
        for broad in &patterns {
            for narrow in &patterns {
                if proof(broad, narrow) {
                    for text in &texts {
                        assert!(!matches(narrow.as_bytes(), text.as_bytes()) || matches(broad.as_bytes(), text.as_bytes()), "unsound {broad:?} covers {narrow:?}: {text:?}");
                    }
                }
            }
        }
    }
}
