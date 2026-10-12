//! HTML chrome host: the WebView2 toolbar bridge, its wire protocol and embedded assets.
//! See docs/html-chrome-protocol.md.

pub mod assets;
pub mod host;
pub mod protocol;

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use windows::Win32::Foundation::RECT;

use crate::chrome::layout::Layout;
use crate::chrome::tab_card::Marker;
use crate::site_security::SiteSecurity;
use crate::tab_groups::{GroupPreferences, TabGroupKey};
use crate::tabs::TabId;
use crate::theme::{Palette, Rgba};
use protocol::{Security, TabItem};

/// Wire value for a site classification. The mapping is part of the bridge contract.
pub fn security_of(site: SiteSecurity) -> Security {
    match site {
        SiteSecurity::Secure => Security::Secure,
        SiteSecurity::NotSecure => Security::NotSecure,
        SiteSecurity::CertificateProblem => Security::CertProblem,
        SiteSecurity::Internal => Security::Internal,
        SiteSecurity::Unknown => Security::Unknown,
        SiteSecurity::NoSite => Security::None,
    }
}

/// CSS colour for a palette entry: `#rrggbb` when opaque, otherwise `rgba(r,g,b,a)` with alpha as
/// a fraction. Every output passes `protocol::is_safe_css_colour`.
pub fn css_colour(colour: Rgba) -> String {
    if colour.a == 0xFF {
        format!("#{:02x}{:02x}{:02x}", colour.r, colour.g, colour.b)
    } else {
        format!(
            "rgba({},{},{},{:.3})",
            colour.r,
            colour.g,
            colour.b,
            f32::from(colour.a) / 255.0
        )
    }
}

/// The toolbar's theme tokens, mapped from the native palette (see docs/html-chrome-protocol.md).
pub fn theme_tokens(palette: &Palette) -> Vec<(&'static str, String)> {
    vec![
        ("bg", css_colour(palette.window)),
        ("fill", css_colour(palette.omnibox_fill)),
        ("border", css_colour(palette.omnibox_border)),
        ("focusBorder", css_colour(palette.omnibox_focus_border)),
        ("text", css_colour(palette.omnibox_text)),
        ("placeholder", css_colour(palette.omnibox_placeholder)),
        ("completion", css_colour(palette.omnibox_completion)),
        ("selection", css_colour(palette.omnibox_selection)),
        ("hover", css_colour(palette.tab_hover)),
        ("progress", css_colour(palette.progress_fill)),
    ]
}

/// Records `json` as the last state sent. Returns true only when it differs from the last one, so
/// an unchanged state is never posted again.
pub fn state_changed(last: &mut Option<String>, json: &str) -> bool {
    if last.as_deref() == Some(json) {
        return false;
    }
    *last = Some(json.to_string());
    true
}

/// Whether the omnibox text should be sent with `setText` on this sync. `prev_tab` is the tab the
/// previous sync saw (`None` before the first sync). A change of active tab always sends, even while
/// the omnibox is focused, so the new tab's address replaces the old one. Within the same tab a
/// focused omnibox keeps the user's typing; otherwise the text is sent when `text_changed`.
pub fn should_set_text(prev_tab: Option<TabId>, tab: TabId, focused: bool, text_changed: bool) -> bool {
    if prev_tab != Some(tab) {
        return true;
    }
    !focused && text_changed
}

/// Whether the HTML toolbar is enabled. Read once from `BOWSER_UI`; unset means the GDI chrome.
pub fn html_ui_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| parse_ui_mode(std::env::var("BOWSER_UI").ok().as_deref()))
}

/// Only the value `html`, in any letter case, selects the HTML chrome.
pub fn parse_ui_mode(value: Option<&str>) -> bool {
    value.is_some_and(|value| value.eq_ignore_ascii_case("html"))
}

/// Client-relative physical pixels of the HTML toolbar: from the back button's left edge to the
/// main menu button's right edge, spanning the strip's height. `None` when the main menu button is
/// empty (the toolbar is too narrow for it) or the span has no width.
pub fn toolbar_bounds(layout: &Layout) -> Option<RECT> {
    if layout.main_menu_button.is_empty() {
        return None;
    }
    let left = layout.back_button.left;
    let right = layout.main_menu_button.right;
    if right - left <= 0 {
        return None;
    }
    Some(RECT {
        left,
        top: layout.strip.top,
        right,
        bottom: layout.strip.bottom,
    })
}

/// Client-relative physical pixels of the HTML tab strip: the whole tab row, including its
/// rail-shifted left edge. `None` when the row is empty (vertical sidebar mode has no tab row).
pub fn tab_row_bounds(layout: &Layout) -> Option<RECT> {
    let row = layout.tab_row;
    if row.is_empty() {
        return None;
    }
    Some(RECT {
        left: row.left,
        top: row.top,
        right: row.right,
        bottom: row.bottom,
    })
}

/// Wire id of a tab: `TabId + 1`, so `0` stays free for "no tab" (`before: 0` is the end of the
/// strip). `None` only when the id has no wire form (`u32::MAX`).
pub fn tab_to_wire(id: TabId) -> Option<u32> {
    id.0.checked_add(1)
}

/// The tab named by a wire id, or `None` for `0`, an id with no wire form, or a tab that is not in
/// `known`. Unknown ids are rejected, never guessed.
pub fn tab_from_wire(raw: u32, known: &[TabId]) -> Option<TabId> {
    let id = TabId(raw.checked_sub(1)?);
    known.contains(&id).then_some(id)
}

/// Post-removal index for dropping `moved` just before the tab named by `before` (`0` = the end of
/// the strip). This is the index `App::drop_tab` takes, the same convention `reorder_target` uses.
/// `None` for an unknown tab or a drop that leaves the tab where it is.
pub fn drop_index(ids: &[TabId], moved: TabId, before: u32) -> Option<usize> {
    let from = ids.iter().position(|id| *id == moved)?;
    if before == 0 {
        return Some(ids.len() - 1).filter(|last| *last != from);
    }
    let target = ids.iter().position(|id| Some(*id) == tab_from_wire(before, ids))?;
    if target == from {
        return None;
    }
    Some(if from < target { target - 1 } else { target })
}

/// Stable, non-zero wire key for a group: FNV-1a of a canonical name for the group. The same group
/// always gets the same key. A hash that lands on `0` is sent as `1`.
pub fn group_wire_key(key: &TabGroupKey) -> u32 {
    let name = match key {
        TabGroupKey::Domain(domain) => format!("d:{}", domain.as_str()),
        TabGroupKey::Opener(id) => format!("o:{}", id.0),
    };
    let mut hash: u32 = 0x811c_9dc5;
    for byte in name.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash.max(1)
}

/// Whether a favicon may be sent to the tab strip: the protocol's gate (a PNG of at most
/// `MAX_FAVICON_BYTES`). The bytes are kept only when this holds.
pub fn favicon_sendable(png: &[u8]) -> bool {
    protocol::fav_json(0, png).is_some()
}

/// Group identity and display state of one tab, from the projection and group preferences.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowGroup {
    pub key: TabGroupKey,
    pub name: String,
    /// Colour id; `None` for the theme accent.
    pub colour: Option<u8>,
    pub collapsed: bool,
    /// Only domain groups collapse and get the island menu; opener groups behave like plain tabs.
    pub collapsible: bool,
}

/// Group state for a key: domain groups carry their name, colour and collapse state.
pub fn row_group(key: &TabGroupKey, preferences: &GroupPreferences) -> RowGroup {
    match key {
        TabGroupKey::Domain(domain) => RowGroup {
            key: key.clone(),
            name: preferences
                .name(domain)
                .map_or_else(|| domain.as_str().to_owned(), str::to_owned),
            colour: preferences.colour(domain),
            collapsed: preferences.is_collapsed(domain),
            collapsible: true,
        },
        TabGroupKey::Opener(_) => RowGroup {
            key: key.clone(),
            name: String::new(),
            colour: None,
            collapsed: false,
            collapsible: false,
        },
    }
}

/// One tab as the strip needs it, gathered from the same data the native paint uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabRow {
    pub id: TabId,
    pub title: String,
    pub loading: bool,
    pub pinned: bool,
    pub muted: bool,
    pub marker: Option<Marker>,
    /// Pinned tabs never join a group, so a pinned row's group is ignored.
    pub group: Option<RowGroup>,
    pub has_favicon: bool,
}

/// Wire items for the strip, in the order given. The first unpinned member of each group is its
/// leader and carries the group fields; `group_size` counts the unpinned members. A row whose id has
/// no wire form is left out.
pub fn tab_items(rows: &[TabRow]) -> Vec<TabItem> {
    fn grouped(row: &TabRow) -> Option<&RowGroup> {
        row.group.as_ref().filter(|_| !row.pinned)
    }
    let mut sizes: HashMap<&TabGroupKey, u32> = HashMap::new();
    for group in rows.iter().filter_map(grouped) {
        *sizes.entry(&group.key).or_default() += 1;
    }
    let mut seen: HashSet<&TabGroupKey> = HashSet::new();
    rows.iter()
        .filter_map(|row| {
            let id = tab_to_wire(row.id)?;
            let group = grouped(row);
            let leader = group.is_some_and(|group| seen.insert(&group.key));
            let (wire_group, name, colour, collapsed, size) = match group {
                Some(group) => (
                    group_wire_key(&group.key),
                    group.name.clone(),
                    group.colour.map_or(-1, i32::from),
                    group.collapsed,
                    sizes.get(&group.key).copied().unwrap_or(0),
                ),
                None => (0, String::new(), -1, false, 0),
            };
            Some(TabItem {
                id,
                title: row.title.clone(),
                loading: row.loading,
                pinned: row.pinned,
                muted: row.muted,
                marker: row.marker.map_or(-1, |marker| i32::from(marker.id())),
                group: wire_group,
                leader,
                group_name: name,
                group_colour: colour,
                collapsed,
                group_size: size,
                has_favicon: row.has_favicon,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::layout::{Rect, compute_in_frame};

    #[test]
    fn only_html_enables_the_html_chrome() {
        assert!(parse_ui_mode(Some("html")));
        assert!(parse_ui_mode(Some("HTML")));
        assert!(parse_ui_mode(Some("Html")));
        for value in [None, Some(""), Some("gdi"), Some("html "), Some(" html"), Some("htm"), Some("1")] {
            assert!(!parse_ui_mode(value), "{value:?}");
        }
    }

    #[test]
    fn toolbar_bounds_span_back_button_to_main_menu() {
        let layout = compute_in_frame(1200, 700, 96, 3, 0, false);
        let bounds = toolbar_bounds(&layout).expect("wide window has a toolbar");
        assert_eq!(bounds.left, layout.back_button.left);
        assert_eq!(bounds.right, layout.main_menu_button.right);
        assert_eq!(bounds.top, layout.strip.top);
        assert_eq!(bounds.bottom, layout.strip.bottom);
        assert!(bounds.right > bounds.left);
    }

    #[test]
    fn toolbar_bounds_is_none_without_a_main_menu_button() {
        let mut layout = compute_in_frame(1200, 700, 96, 3, 0, false);
        layout.main_menu_button = Rect::new(0, 0, 0, 0);
        assert!(toolbar_bounds(&layout).is_none());
    }

    #[test]
    fn css_colour_renders_opaque_and_translucent_forms() {
        assert_eq!(css_colour(Rgba::hex(0x1A2B3C, 0xFF)), "#1a2b3c");
        assert_eq!(css_colour(Rgba { r: 0, g: 0, b: 0, a: 0 }), "rgba(0,0,0,0.000)");
        assert_eq!(css_colour(Rgba { r: 255, g: 255, b: 255, a: 128 }), "rgba(255,255,255,0.502)");
        for colour in [
            Rgba::hex(0x000000, 0xFF),
            Rgba { r: 9, g: 8, b: 7, a: 1 },
            Rgba { r: 255, g: 0, b: 128, a: 254 },
        ] {
            assert!(protocol::is_safe_css_colour(&css_colour(colour)), "{colour:?}");
        }
    }

    #[test]
    fn theme_tokens_cover_every_token_with_safe_colours() {
        for (appearance, solid) in [
            (crate::theme::Appearance::Light, false),
            (crate::theme::Appearance::Dark, false),
            (crate::theme::Appearance::Light, true),
            (crate::theme::Appearance::Dark, true),
        ] {
            let tokens = theme_tokens(crate::theme::palette(appearance, solid));
            let keys: Vec<&str> = tokens.iter().map(|(key, _)| *key).collect();
            assert_eq!(keys, protocol::THEME_TOKENS, "{appearance} solid={solid}");
            for (key, value) in &tokens {
                assert!(protocol::is_safe_css_colour(value), "{key}={value}");
            }
        }
    }

    #[test]
    fn site_security_maps_to_the_wire_values() {
        let cases = [
            (SiteSecurity::Secure, "secure"),
            (SiteSecurity::NotSecure, "notsecure"),
            (SiteSecurity::CertificateProblem, "certproblem"),
            (SiteSecurity::Internal, "internal"),
            (SiteSecurity::Unknown, "unknown"),
            (SiteSecurity::NoSite, "none"),
        ];
        for (site, wire) in cases {
            assert_eq!(security_of(site).as_str(), wire, "{site:?}");
        }
    }

    #[test]
    fn state_diff_skips_unchanged_json() {
        let mut last = None;
        assert!(state_changed(&mut last, "{\"a\":1}"));
        assert!(!state_changed(&mut last, "{\"a\":1}"));
        assert!(state_changed(&mut last, "{\"a\":2}"));
        assert!(state_changed(&mut last, "{\"a\":1}"));
        assert_eq!(last.as_deref(), Some("{\"a\":1}"));
    }

    #[test]
    fn set_text_follows_active_tab_changes_even_when_focused() {
        let (a, b) = (TabId(1), TabId(2));
        // First sync and any tab change send the text, focused or not.
        assert!(should_set_text(None, a, false, true));
        assert!(should_set_text(None, a, true, true));
        assert!(should_set_text(Some(a), b, true, false), "focused new tab");
        assert!(should_set_text(Some(a), b, false, false), "switch while unfocused");
        // Same tab, focused: never rewritten, so typing survives.
        assert!(!should_set_text(Some(a), a, true, true));
        assert!(!should_set_text(Some(a), a, true, false));
        // Same tab, unfocused: sent only when the text changed.
        assert!(should_set_text(Some(a), a, false, true));
        assert!(!should_set_text(Some(a), a, false, false));
    }

    #[test]
    fn toolbar_bounds_is_none_when_the_span_has_no_width() {
        let mut layout = compute_in_frame(1200, 700, 96, 3, 0, false);
        // Menu button ends left of the back button: the span is negative.
        layout.main_menu_button = Rect::new(0, 0, layout.back_button.left - 10, 10);
        assert!(toolbar_bounds(&layout).is_none());
    }

    #[test]
    fn tab_row_bounds_is_the_full_tab_row() {
        let mut layout = compute_in_frame(1200, 700, 96, 3, 0, false);
        let bounds = tab_row_bounds(&layout).expect("horizontal layout has a tab row");
        assert_eq!(bounds.left, layout.tab_row.left);
        assert_eq!(bounds.top, layout.tab_row.top);
        assert_eq!(bounds.right, layout.tab_row.right);
        assert_eq!(bounds.bottom, layout.tab_row.bottom);
        // The rail-shifted left edge is kept as laid out.
        layout.tab_row = Rect::new(60, 34, 1200, 62);
        assert_eq!(tab_row_bounds(&layout).map(|bounds| bounds.left), Some(60));
        // Vertical sidebar mode leaves the row empty, and so does a zero-width row.
        layout.tab_row = Rect::default();
        assert!(tab_row_bounds(&layout).is_none());
        layout.tab_row = Rect::new(0, 34, 0, 62);
        assert!(tab_row_bounds(&layout).is_none());
    }

    #[test]
    fn tab_wire_ids_are_offset_and_checked() {
        assert_eq!(tab_to_wire(TabId(0)), Some(1));
        assert_eq!(tab_to_wire(TabId(u32::MAX)), None);
        let known = [TabId(0), TabId(4)];
        assert_eq!(tab_from_wire(1, &known), Some(TabId(0)));
        assert_eq!(tab_from_wire(5, &known), Some(TabId(4)));
        assert_eq!(tab_from_wire(0, &known), None, "0 is the end marker, never a tab");
        assert_eq!(tab_from_wire(3, &known), None, "unknown tab");
        assert_eq!(tab_from_wire(u32::MAX, &known), None);
        for id in known {
            assert_eq!(tab_from_wire(tab_to_wire(id).expect("wire id"), &known), Some(id));
        }
    }

    #[test]
    fn drop_index_matches_the_native_post_removal_index() {
        let ids = [TabId(0), TabId(1), TabId(2), TabId(3)];
        let wire = |id: u32| tab_to_wire(TabId(id)).expect("wire id");
        // Tab 0 dropped before tab 2 is reinserted at index 1 once it is removed.
        assert_eq!(drop_index(&ids, TabId(0), wire(2)), Some(1));
        // Tab 3 dropped before tab 1 lands at index 1.
        assert_eq!(drop_index(&ids, TabId(3), wire(1)), Some(1));
        // Before 0 means the end of the strip.
        assert_eq!(drop_index(&ids, TabId(0), 0), Some(3));
        // No-ops and unknown ids.
        assert_eq!(drop_index(&ids, TabId(1), wire(1)), None, "before itself");
        assert_eq!(drop_index(&ids, TabId(3), 0), None, "already last");
        assert_eq!(drop_index(&ids, TabId(9), wire(1)), None, "unknown moved tab");
        assert_eq!(drop_index(&ids, TabId(0), wire(9)), None, "unknown target");
    }

    #[test]
    fn group_wire_keys_are_stable_nonzero_and_distinct() {
        let domain = |text: &str| {
            let url = url::Url::parse(text).expect("url");
            TabGroupKey::Domain(crate::tab_groups::DomainKey::from_url(&url).expect("domain"))
        };
        let keys = [
            domain("https://example.com/"),
            domain("https://example.org/"),
            TabGroupKey::Opener(TabId(7)),
            TabGroupKey::Opener(TabId(8)),
        ];
        for key in &keys {
            let wire = group_wire_key(key);
            assert_ne!(wire, 0, "{key:?}");
            assert_eq!(group_wire_key(key), wire, "{key:?} is stable");
        }
        // A freshly built key for the same domain has the same wire key.
        assert_eq!(group_wire_key(&keys[0]), group_wire_key(&domain("https://example.com/other")));
        let wires: HashSet<u32> = keys.iter().map(group_wire_key).collect();
        assert_eq!(wires.len(), keys.len());
    }

    #[test]
    fn favicon_gate_is_png_and_at_most_the_protocol_cap() {
        // Signature, then an IHDR chunk of 16x16, padded to `len` bytes.
        let png = |len: usize| {
            let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
            bytes.extend_from_slice(&[0, 0, 0, 13]);
            bytes.extend_from_slice(b"IHDR");
            bytes.extend_from_slice(&[0, 0, 0, 16, 0, 0, 0, 16, 8, 6, 0, 0, 0]);
            bytes.resize(len, 0);
            bytes
        };
        assert!(favicon_sendable(&png(protocol::MAX_FAVICON_BYTES)));
        assert!(!favicon_sendable(&png(protocol::MAX_FAVICON_BYTES + 1)));
        assert!(!favicon_sendable(b"GIF89a"));
        assert!(!favicon_sendable(&[]));
        // A PNG whose IHDR is 300 pixels wide is not sent.
        let mut wide = png(64);
        wide[16..20].copy_from_slice(&300u32.to_be_bytes());
        assert!(!favicon_sendable(&wide));
    }

    #[test]
    fn tab_items_mark_one_leader_and_count_unpinned_members() {
        let group = RowGroup {
            key: TabGroupKey::Domain(
                crate::tab_groups::DomainKey::from_url(
                    &url::Url::parse("https://example.com/").expect("url"),
                )
                .expect("domain"),
            ),
            name: "Example".to_owned(),
            colour: Some(3),
            collapsed: true,
            collapsible: true,
        };
        let row = |id: u32, group: Option<RowGroup>, pinned: bool| TabRow {
            id: TabId(id),
            title: format!("tab {id}"),
            loading: false,
            pinned,
            muted: false,
            marker: None,
            group,
            has_favicon: id == 1,
        };
        let rows = vec![
            row(0, Some(group.clone()), false),
            row(1, Some(group.clone()), false),
            TabRow {
                marker: Some(Marker::Flame),
                ..row(2, None, false)
            },
            row(3, Some(group.clone()), true),
        ];
        let items = tab_items(&rows);
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].id, 1, "wire id is TabId + 1");
        assert!(items[0].leader && !items[1].leader);
        assert_eq!(items[0].group, items[1].group);
        assert_ne!(items[0].group, 0);
        assert_eq!(items[0].group_size, 2);
        assert_eq!(items[0].group_name, "Example");
        assert_eq!(items[0].group_colour, 3);
        assert!(items[0].collapsed);
        assert!(!items[0].has_favicon && items[1].has_favicon);
        assert_eq!(items[2].group, 0);
        assert_eq!(items[2].marker, 1);
        assert_eq!(items[2].group_colour, -1);
        assert_eq!(items[3].group, 0, "pinned tabs never join a group");
        assert!(!items[3].leader);
        let json = protocol::tabs_json(1, &items);
        assert!(json.contains("\"gl\":true") && json.contains("\"gsize\":2"), "{json}");
    }
}
