//! Quick Commands search, in Rust: filter prefixes, the calculator row, result types and their order,
//! the visible-result limit. The HTML palette does the same in `ui/src/palette-logic.ts` (the page
//! filters as you type); this is the native fallback's search and the reference both are tested
//! against (`ui/test/fixtures/palette-cases.json`).

use super::{
    calc,
    store::{Options, ResultType},
};

/// One searchable row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub kind: ResultType,
    pub label: String,
    pub detail: String,
    /// What running the row does: `tab:<id>`, `url:<address>`, a command id, ...
    pub action: String,
}

/// Rows shown for an empty search, per type.
pub fn empty_limit(kind: ResultType) -> usize {
    match kind {
        ResultType::Tab => 5,
        ResultType::Closed => 3,
        ResultType::Workspace => 3,
        ResultType::Command => 8,
        _ => 0,
    }
}

/// `bookmark:` and `bkm:`, `note:`, `tab:` and `tb:` (open and closed tabs), `history:`, `command:`
/// and `cmd:`, `action:`, `workspace:`. Returns the types the prefix names and the text after it.
/// Text with no known prefix is returned whole.
pub fn parse_prefix(query: &str) -> (Option<Vec<ResultType>>, &str) {
    let trimmed = query.trim_start();
    if let Some((word, rest)) = trimmed.split_once(':') {
        let kinds: Option<Vec<ResultType>> = match word.to_ascii_lowercase().as_str() {
            "bookmark" | "bkm" => Some(vec![ResultType::Bookmark]),
            "note" => Some(vec![ResultType::Note]),
            "tab" | "tb" => Some(vec![ResultType::Tab, ResultType::Closed]),
            "history" => Some(vec![ResultType::History]),
            "command" | "cmd" => Some(vec![ResultType::Command]),
            "action" => Some(vec![ResultType::Action]),
            "workspace" => Some(vec![ResultType::Workspace]),
            _ => None,
        };
        if kinds.is_some() {
            return (kinds, rest.trim_start());
        }
    }
    (None, query)
}

fn subsequence_gap(needle: &str, haystack: &str) -> Option<usize> {
    let mut chars = haystack.char_indices();
    let mut first = None;
    let mut last = 0usize;
    for wanted in needle.chars() {
        let (at, _) = chars.find(|(_, found)| *found == wanted)?;
        first.get_or_insert(at);
        last = at;
    }
    Some(last.saturating_sub(first?))
}

/// Score of an item for a lower-cased, trimmed needle: exact label, label prefix, word prefix,
/// detail prefix, then ordered fuzzy matches in the label and the detail. `None` for no match.
pub fn score(label: &str, detail: &str, needle: &str) -> Option<i32> {
    let label = label.to_lowercase();
    let detail = detail.to_lowercase();
    if label == needle {
        return Some(3000);
    }
    if label.starts_with(needle) {
        return Some(2000 - label.chars().count().min(500) as i32);
    }
    if label.split_whitespace().any(|word| word.starts_with(needle)) {
        return Some(1500 - label.chars().count().min(500) as i32);
    }
    if detail.starts_with(needle) {
        return Some(1000 - detail.chars().count().min(500) as i32);
    }
    if let Some(gap) = subsequence_gap(needle, &label) {
        return Some(500 - gap.min(400) as i32);
    }
    subsequence_gap(needle, &detail).map(|gap| 250 - gap.min(200) as i32)
}

/// The rows for `query`, in display order. Empty text lists the first rows of each enabled type;
/// typed text lists the best `palette_max_results` matches, grouped by type (where the best match
/// of each group ranks while the order is the default, else in the user's order), with a
/// calculator row first when the text is a sum.
pub fn search(items: &[Item], query: &str, options: &Options) -> Vec<Item> {
    let (filter, text) = parse_prefix(query);
    let needle = text.trim().to_lowercase();
    let allowed = |kind: ResultType| options.palette_types.contains(&kind) && filter.as_ref().is_none_or(|f| f.contains(&kind));
    let limit = usize::from(options.palette_max_results);
    if needle.is_empty() {
        let mut out = Vec::new();
        for kind in &options.palette_types {
            if !allowed(*kind) {
                continue;
            }
            let cap = if filter.is_some() { limit } else { empty_limit(*kind) };
            out.extend(items.iter().filter(|item| item.kind == *kind).take(cap).cloned());
        }
        return out;
    }
    let mut ranked: Vec<(usize, i32, &Item)> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| allowed(item.kind))
        .filter_map(|(index, item)| score(&item.label, &item.detail, &needle).map(|s| (index, s, item)))
        .collect();
    ranked.sort_by(|(ia, sa, a), (ib, sb, b)| {
        sb.cmp(sa)
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
            .then_with(|| ia.cmp(ib))
    });
    ranked.truncate(limit);
    let mut order: Vec<ResultType> = Vec::new();
    if options.palette_ranked() {
        for (_, _, item) in &ranked {
            if !order.contains(&item.kind) {
                order.push(item.kind);
            }
        }
    } else {
        order = options.palette_types.iter().copied().filter(|kind| ranked.iter().any(|(_, _, item)| item.kind == *kind)).collect();
    }
    let mut out = Vec::new();
    if filter.is_none() && options.palette_types.contains(&ResultType::Calc) && let Some(result) = calc::result_text(text) {
        out.push(Item { kind: ResultType::Calc, label: result.clone(), detail: format!("{} = {result}. Press Enter to copy.", text.trim()), action: format!("copy:{result}") });
    }
    for kind in order {
        out.extend(ranked.iter().filter(|(_, _, item)| item.kind == kind).map(|(_, _, item)| (*item).clone()));
    }
    out
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    fn item(kind: ResultType, label: &str, detail: &str) -> Item {
        Item { kind, label: label.into(), detail: detail.into(), action: format!("{}:{label}", kind.key()) }
    }

    fn sample() -> Vec<Item> {
        vec![
            item(ResultType::Tab, "Settings guide", "https://docs.example/"),
            item(ResultType::Closed, "Closed news", "https://news.example/"),
            item(ResultType::Workspace, "Work", "3 tabs"),
            item(ResultType::Command, "Settings", "Open browser settings"),
            item(ResultType::Action, "Reload", "Reload the current page"),
            item(ResultType::Bookmark, "Sweet tea", "https://tea.example/"),
            item(ResultType::Note, "Shopping", "milk and tea"),
            item(ResultType::History, "News", "https://set.example/"),
        ]
    }

    fn labels(items: &[Item]) -> Vec<String> {
        items.iter().map(|i| i.label.clone()).collect()
    }

    /// The cases the HTML palette is also tested against (`ui/test/palette.mjs`), so both searches agree.
    #[test]
    fn the_shared_cases_give_the_same_rows_as_the_page() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/test/fixtures/palette-cases.txt");
        let text = std::fs::read_to_string(path).expect("palette cases");
        let mut items = Vec::new();
        let mut checked = 0;
        for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            let parts: Vec<&str> = line.split('\t').collect();
            match parts[0] {
                "item" => items.push(Item {
                    kind: ResultType::parse(parts[1]).expect("kind"),
                    label: parts[2].into(),
                    detail: parts[3].into(),
                    action: parts[4].into(),
                }),
                "case" => {
                    let mut options = Options::default();
                    if parts[2] != "*" {
                        options.palette_types = crate::commands::store::parse_types(parts[2]);
                    }
                    options.palette_max_results = parts[3].parse().expect("limit");
                    let expected: Vec<&str> = if parts.get(4).is_none_or(|e| e.is_empty()) { Vec::new() } else { parts[4].split('|').collect() };
                    let found: Vec<String> = search(&items, parts[1], &options).into_iter().map(|i| i.action).collect();
                    assert_eq!(found, expected, "query {:?} types {} limit {}", parts[1], parts[2], parts[3]);
                    checked += 1;
                }
                other => panic!("unknown record {other}"),
            }
        }
        assert!(checked >= 30, "{checked}");
    }

    #[test]
    fn prefixes_and_their_short_forms_pick_the_types() {
        let cases = [
            ("bookmark: tea", vec![ResultType::Bookmark], "tea"),
            ("bkm:tea", vec![ResultType::Bookmark], "tea"),
            ("BKM: tea", vec![ResultType::Bookmark], "tea"),
            ("note: milk", vec![ResultType::Note], "milk"),
            ("tab: set", vec![ResultType::Tab, ResultType::Closed], "set"),
            ("tb: set", vec![ResultType::Tab, ResultType::Closed], "set"),
            ("history:news", vec![ResultType::History], "news"),
            ("command: set", vec![ResultType::Command], "set"),
            ("cmd: set", vec![ResultType::Command], "set"),
            ("action: rel", vec![ResultType::Action], "rel"),
            ("workspace: wo", vec![ResultType::Workspace], "wo"),
            ("  tb:", vec![ResultType::Tab, ResultType::Closed], ""),
        ];
        for (query, kinds, rest) in cases {
            assert_eq!(parse_prefix(query), (Some(kinds), rest), "{query}");
        }
        for plain in ["settings", "foo: bar", "https://example.com", "12:30", "calc: 1+1", ":tab"] {
            assert_eq!(parse_prefix(plain), (None, plain), "{plain}");
        }
    }

    #[test]
    fn a_prefix_limits_results_to_its_types() {
        let options = Options::default();
        let found = search(&sample(), "tb: s", &options);
        assert_eq!(labels(&found), ["Settings guide", "Closed news"], "tb: lists open and closed tabs");
        assert_eq!(labels(&search(&sample(), "tab: news", &options)), ["Closed news"], "and closed tabs");
        assert_eq!(labels(&search(&sample(), "bkm: tea", &options)), ["Sweet tea"]);
        assert_eq!(labels(&search(&sample(), "note: tea", &options)), ["Shopping"], "notes match their text");
        assert_eq!(labels(&search(&sample(), "cmd: set", &options)), ["Settings"]);
        assert_eq!(labels(&search(&sample(), "action: rel", &options)), ["Reload"]);
        assert_eq!(labels(&search(&sample(), "workspace: wor", &options)), ["Work"]);
        assert_eq!(labels(&search(&sample(), "history: news", &options)), ["News"]);
        // A prefix alone lists that type.
        assert_eq!(labels(&search(&sample(), "bookmark:", &options)), ["Sweet tea"]);
    }

    #[test]
    fn typed_results_rank_by_match_quality_while_the_order_is_the_default() {
        let options = Options::default();
        let found = search(&sample(), "settings", &options);
        // The exact command match (3000) leads; the tab (a word match) follows.
        assert_eq!(labels(&found), ["Settings", "Settings guide"]);
        let found = search(&sample(), "set", &options);
        assert_eq!(found[0].kind, ResultType::Command, "the best match leads");
    }

    #[test]
    fn a_custom_order_groups_results_by_it() {
        let mut options = Options::default();
        options.palette_types = vec![ResultType::History, ResultType::Command, ResultType::Tab];
        let found = search(&sample(), "set", &options);
        let kinds: Vec<_> = found.iter().map(|i| i.kind).collect();
        assert_eq!(kinds, [ResultType::History, ResultType::Command, ResultType::Tab], "{found:?}");
    }

    #[test]
    fn hidden_types_never_appear_even_through_their_prefix() {
        let mut options = Options::default();
        options.palette_types.retain(|kind| *kind != ResultType::Bookmark && *kind != ResultType::Note);
        assert!(search(&sample(), "tea", &options).iter().all(|i| !matches!(i.kind, ResultType::Bookmark | ResultType::Note)));
        assert!(search(&sample(), "bkm: tea", &options).is_empty());
        assert!(search(&sample(), "", &options).iter().all(|i| i.kind != ResultType::Bookmark));
    }

    #[test]
    fn the_limit_caps_typed_results_and_prefixed_lists() {
        let many: Vec<Item> = (0..40).map(|n| item(ResultType::Bookmark, &format!("Page {n}"), "https://example.com/")).collect();
        let mut options = Options::default();
        assert_eq!(search(&many, "page", &options).len(), 12);
        options.palette_max_results = 5;
        assert_eq!(search(&many, "page", &options).len(), 5);
        assert_eq!(search(&many, "bkm:", &options).len(), 5, "a bare prefix lists up to the limit");
        options.palette_max_results = 30;
        assert_eq!(search(&many, "page", &options).len(), 30);
    }

    #[test]
    fn an_empty_search_lists_the_first_tabs_and_commands_only() {
        let options = Options::default();
        let found = search(&sample(), "", &options);
        assert_eq!(labels(&found), ["Settings guide", "Closed news", "Work", "Settings"]);
        let many: Vec<Item> = (0..20).map(|n| item(ResultType::Tab, &format!("T{n}"), "")).collect();
        assert_eq!(search(&many, "", &options).len(), 5);
    }

    #[test]
    fn a_sum_adds_a_calculator_row_first_unless_prefixed_or_hidden() {
        let options = Options::default();
        let found = search(&sample(), "2+2*3", &options);
        assert_eq!(found[0].kind, ResultType::Calc);
        assert_eq!((found[0].label.as_str(), found[0].action.as_str()), ("8", "copy:8"));
        assert!(found[0].detail.contains("2+2*3 = 8"), "{}", found[0].detail);
        assert!(search(&sample(), "tab: 2+2", &options).iter().all(|i| i.kind != ResultType::Calc));
        let mut hidden = Options::default();
        hidden.palette_types.retain(|kind| *kind != ResultType::Calc);
        assert!(search(&sample(), "2+2", &hidden).is_empty());
        assert!(search(&sample(), "42", &options).iter().all(|i| i.kind != ResultType::Calc), "a bare number is not a sum");
    }

    #[test]
    fn matching_is_case_insensitive_and_fuzzy_inside_a_label() {
        let options = Options::default();
        assert_eq!(labels(&search(&sample(), "RELOAD", &options)), ["Reload"]);
        assert_eq!(labels(&search(&sample(), "swt", &options)), ["Sweet tea"], "ordered letters match");
        assert!(search(&sample(), "zzz", &options).is_empty());
    }
}
