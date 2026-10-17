//! HTML chrome host: the WebView2 toolbar bridge, its wire protocol and embedded assets.
//! See docs/html-chrome-protocol.md.

pub mod address_page; // the address bar, search and bookmark options (src/address_settings.rs)
pub mod assets;
#[cfg(test)]
mod fingerprint;
#[cfg(test)]
mod guard;
pub mod host;
pub mod json;
pub mod protocol;
pub mod settings;
pub mod history_page; // bowser://history (docs/design/history-page.md)
pub mod downloads_page; // bowser://downloads (docs/design/downloads.md)
pub mod commands_page; // the settings page's Keyboard and Quick Commands sections (docs/commands.md)
pub mod start_page; // the new-tab start page's wire (docs/design/start-page.md)
pub mod extensions_page; // the settings page's Extensions section and the toolbar button (docs/extensions.md)
// --- split view ---
pub mod split;
// --- end split view ---
pub mod tab_drag; // the native ghost of a held tab (docs/design/tab-drag.md)
// --- panel host (lane P) ---
pub mod panels;
// --- end panel host ---

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use windows::Win32::Foundation::RECT;

use crate::chrome::layout::Layout;
use crate::chrome::rail::{RailEntry, RailIcon, RailKey, WorkspaceGlyph};
use crate::chrome::tab_card::Marker;
use crate::site_security::SiteSecurity;
use crate::tab_groups::{GroupPreferences, TabGroupKey};
use crate::tabs::TabId;
use crate::theme::{Palette, Rgba};
use protocol::{RailItem, RailKeyWire, Security, TabItem};

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

/// A cache may be advanced only after the matching WebView2 post succeeds.
pub fn state_differs(last: &Option<String>, json: &str) -> bool {
    last.as_deref() != Some(json)
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

/// A pending native edit must cross to the HTML omnibox as the toolbar takes over.
pub fn should_handoff_edit(native_focused: bool, native_text: &str, committed_text: &str) -> bool {
    native_focused || native_text != committed_text
}

/// The text the HTML omnibox starts with when it takes over from the native edit. An empty field
/// that nobody has typed in (`native_modified` is false) is just the blank start state of a window
/// whose page is still loading, so the tab's address (`committed_text`, which may be the requested
/// one) is shown instead of a placeholder; anything the user typed or cleared is kept.
pub fn handoff_text<'a>(native_text: &'a str, native_modified: bool, committed_text: &'a str) -> &'a str {
    if native_text.is_empty() && !native_modified { committed_text } else { native_text }
}

/// Whether the HTML chrome is enabled. Read once from `BOWSER_UI`: `html` enables it and `native`
/// (or `gdi`) disables it. When the variable is unset or has another value, builds made with the
/// `html-chrome-default` feature (the installer's release build) use the HTML chrome and plain
/// builds keep the GDI chrome. The HTML chrome falls back to GDI at runtime if its views fail.
pub fn html_ui_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        ui_mode(
            std::env::var("BOWSER_UI").ok().as_deref(),
            cfg!(feature = "html-chrome-default"),
        )
    })
}

/// Resolves the chrome mode: an explicit `html` or `native`/`gdi` (any letter case) wins, anything
/// else uses `default_html`.
pub fn ui_mode(value: Option<&str>, default_html: bool) -> bool {
    match value {
        Some(v) if v.eq_ignore_ascii_case("html") => true,
        Some(v) if v.eq_ignore_ascii_case("native") || v.eq_ignore_ascii_case("gdi") => false,
        _ => default_html,
    }
}

/// Only the value `html`, in any letter case, selects the HTML chrome (no compile-time default).
pub fn parse_ui_mode(value: Option<&str>) -> bool {
    ui_mode(value, false)
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

/// Rounds physical toolbar coordinates to CSS pixels with the same half-up convention as
/// `Metrics::px`, expressed in the inverse direction.
pub fn css_px_from_physical(physical: i32, dpi: u32) -> i32 {
    let dpi = crate::chrome::layout::Metrics::new(dpi).dpi() as i64;
    ((i64::from(physical.max(0)) * 96 + dpi / 2) / dpi) as i32
}

/// CSS px the toolbar reserves for the download button while it shows (docs/design/downloads.md): the
/// 28 px button and a 4 px gap. The pill gives up exactly this much width, so the page's actions group
/// moves left by the same amount and the menu button never moves. The toolbar page agrees (`.has-dl`).
pub const DL_BUTTON_SLOT: i32 = 32;

/// CSS px the toolbar reserves for the extensions button (docs/extensions.md), left of the download
/// button's slot: the same 28 px button and 4 px gap. The toolbar page agrees (`.has-ext`).
pub const EXT_BUTTON_SLOT: i32 = 32;

/// Pill x, pill width and menu x in CSS px relative to the HTML toolbar host's left edge.
pub fn toolbar_css_placement(layout: &Layout, dpi: u32) -> Option<(i32, i32, i32)> {
    toolbar_css_placement_with_slot(layout, dpi, 0)
}

/// [`toolbar_css_placement`] with `slot` CSS px taken from the pill's right end for the download
/// button (`0` or [`DL_BUTTON_SLOT`]). The pill keeps at least one pixel.
pub fn toolbar_css_placement_with_slot(layout: &Layout, dpi: u32, slot: i32) -> Option<(i32, i32, i32)> {
    let bounds = toolbar_bounds(layout)?;
    Some((
        css_px_from_physical(layout.omnibox.left - bounds.left, dpi),
        (css_px_from_physical(layout.omnibox.width(), dpi) - slot.max(0)).max(1),
        css_px_from_physical(layout.main_menu_button.left - bounds.left, dpi),
    ))
}

/// Client-relative physical pixels of the omnibox pill as the HTML toolbar paints it: the CSS
/// placement above, on the pill's CSS row (centred in the toolbar row), with each edge scaled and
/// rounded to a device pixel the way Blink snaps box edges. The suggestion dropdown attaches to this
/// rectangle rather than to `layout.omnibox`, which can sit a pixel away from the painted pill once
/// the placement has been rounded to CSS pixels (125% and 175%).
pub fn toolbar_pill_rect(layout: &Layout, dpi: u32) -> Option<RECT> {
    toolbar_pill_rect_with_slot(layout, dpi, 0)
}

/// [`toolbar_pill_rect`] for a toolbar whose pill gave `slot` CSS px to the download button.
pub fn toolbar_pill_rect_with_slot(layout: &Layout, dpi: u32, slot: i32) -> Option<RECT> {
    use crate::chrome::layout::Metrics;
    let bounds = toolbar_bounds(layout)?;
    let (x, w, _) = toolbar_css_placement_with_slot(layout, dpi, slot)?;
    let scale = f64::from(Metrics::new(dpi).dpi()) / 96.0;
    let snap = |css: i32| (f64::from(css) * scale).round() as i32;
    let top = (Metrics::STRIP_HEIGHT - Metrics::OMNIBOX_HEIGHT) / 2;
    Some(RECT {
        left: bounds.left + snap(x),
        top: bounds.top + snap(top),
        right: bounds.left + snap(x + w),
        bottom: bounds.top + snap(top + Metrics::OMNIBOX_HEIGHT),
    })
}

/// Client-relative physical pixels of the HTML tab strip: the tab row, including its rail-shifted
/// left edge. The strip stops one island gap before the native overflow chevron while it is showing
/// (so the last tab is not painted under the button), and before "+" when that sits in the row.
/// `None` when the row is empty (vertical sidebar mode has no tab row) or the chevron leaves no
/// room.
pub fn tab_row_bounds(layout: &Layout) -> Option<RECT> {
    use crate::chrome::layout::Metrics;
    let row = layout.tab_row;
    if row.is_empty() {
        return None;
    }
    let gap = layout.metrics.px(Metrics::ISLAND_GAP);
    let mut right = row.right;
    // `ToolbarTop` keeps "+" inside the full-width row, at its right end. The other layouts end the
    // row before it, so there it is not inside the row and changes nothing.
    let plus = layout.new_tab_button;
    if !plus.is_empty() && row.contains_rect(plus) {
        right = (plus.left - gap).max(row.left);
    }
    let button = layout.tab_menu_button;
    if !button.is_empty() {
        right = right.min((button.left - gap).max(row.left));
    }
    if right <= row.left {
        return None;
    }
    Some(RECT {
        left: row.left,
        top: row.top,
        right,
        bottom: row.bottom,
    })
}

// --- vertical sidebar (lane S) ---
/// Client-relative HTML sidebar bounds, stopping at the native resize handle's left edge.
pub fn sidebar_bounds(layout: &crate::vertical_tabs::SidebarLayout) -> Option<RECT> {
    let bounds = layout.sidebar;
    // --- layout (thread T6) --- the handle is on the inner edge: right of a left sidebar, left of a right one.
    let (left, right) = (layout.content_left(), layout.content_right());
    if right <= left || bounds.bottom <= bounds.top { return None; }
    Some(RECT { left, top: bounds.top, right, bottom: bounds.bottom })
}
// --- end vertical sidebar ---

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
    /// A private tab. Private tabs never join a group, so a private row's group is ignored.
    pub private: bool,
    pub marker: Option<Marker>,
    /// Pinned tabs never join a group, so a pinned row's group is ignored.
    pub group: Option<RowGroup>,
    pub has_favicon: bool,
    // --- tab options ---
    pub marks: protocol::TabMarks,
    // --- end tab options ---
}

/// Wire items for the strip, in the order given. The first unpinned member of each group is its
/// leader and carries the group fields; `group_size` counts the unpinned members. A row whose id has
/// no wire form is left out.
pub fn tab_items(rows: &[TabRow]) -> Vec<TabItem> {
    fn grouped(row: &TabRow) -> Option<&RowGroup> {
        row.group.as_ref().filter(|_| !row.pinned && !row.private)
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
                private: row.private,
                marker: row.marker.map_or(-1, |marker| i32::from(marker.id())),
                group: wire_group,
                leader,
                group_name: name,
                group_colour: colour,
                collapsed,
                group_size: size,
                has_favicon: row.has_favicon,
                marks: row.marks.clone(), // --- tab options ---
            })
        })
        .collect()
}

/// Client-relative physical pixels of the HTML left rail: the full-height pill (`layout.rail`).
/// `None` when the layout has no rail.
pub fn rail_bounds(layout: &Layout) -> Option<RECT> {
    let rail = layout.rail;
    if rail.is_empty() {
        return None;
    }
    Some(RECT {
        left: rail.left,
        top: rail.top,
        right: rail.right,
        bottom: rail.bottom,
    })
}

/// Wire form of a rail key. Total: every rail key has one spelling.
pub fn rail_key_to_wire(key: RailKey) -> RailKeyWire {
    match key {
        RailKey::Home => RailKeyWire::Home,
        RailKey::Customize => RailKeyWire::Customize,
        RailKey::Workspace(id) => RailKeyWire::Workspace(id),
        RailKey::AddWorkspace => RailKeyWire::AddWorkspace,
        RailKey::App(id) => RailKeyWire::App(id),
        RailKey::AddApp => RailKeyWire::AddApp,
        RailKey::Bookmarks => RailKeyWire::Bookmarks,
        RailKey::History => RailKeyWire::History,
        RailKey::Menu => RailKeyWire::Menu,
    }
}

/// The rail key a wire key names, but only when the current rail model has it. `None` for a key
/// the model does not hold, so an unknown or stale key is ignored, never guessed.
pub fn rail_key_from_wire(wire: RailKeyWire, known: &[RailKey]) -> Option<RailKey> {
    known
        .iter()
        .copied()
        .find(|key| rail_key_to_wire(*key) == wire)
}

/// The wire icon name for a rail icon. Workspace glyphs are `g:` followed by the glyph name.
pub fn rail_icon_wire(icon: RailIcon) -> String {
    let name = match icon {
        RailIcon::Home => "home",
        RailIcon::Customize => "customize",
        RailIcon::Plus => "plus",
        RailIcon::Heart => "heart",
        RailIcon::Clock => "clock",
        RailIcon::Dots => "dots",
        RailIcon::AppTile => "apptile",
        RailIcon::Letter => "letter",
        RailIcon::Glyph(glyph) => {
            let glyph = match glyph {
                WorkspaceGlyph::Briefcase => "briefcase",
                WorkspaceGlyph::Home => "home",
                WorkspaceGlyph::Code => "code",
                WorkspaceGlyph::Book => "book",
                WorkspaceGlyph::Cart => "cart",
                WorkspaceGlyph::Game => "game",
                WorkspaceGlyph::Music => "music",
                WorkspaceGlyph::Heart => "heart",
                WorkspaceGlyph::Star => "star",
                WorkspaceGlyph::Bolt => "bolt",
                WorkspaceGlyph::Globe => "globe",
                WorkspaceGlyph::Leaf => "leaf",
            };
            return format!("g:{glyph}");
        }
    };
    name.to_owned()
}

/// The rail as the HTML view shows it, built from the same entries the native paint draws, in their
/// order. `open` is the button whose panel is open; it shows as active like the current workspace.
/// `png_of` gives an entry's favicon PNG, when it has one. An app whose favicon may not be sent
/// falls back to the letter tile, since the page has nothing to draw for it.
pub fn rail_items<'a>(
    entries: &[RailEntry<'_>],
    open: Option<RailKey>,
    png_of: impl Fn(RailKey) -> Option<&'a [u8]>,
) -> Vec<RailItem> {
    entries
        .iter()
        .map(|entry| {
            let has_favicon = png_of(entry.key).is_some_and(favicon_sendable);
            let icon = if entry.icon == RailIcon::AppTile && !has_favicon {
                RailIcon::Letter
            } else {
                entry.icon
            };
            RailItem {
                key: rail_key_to_wire(entry.key).to_wire(),
                icon: rail_icon_wire(icon),
                label: entry.label.clone(),
                active: entry.active || open == Some(entry.key),
                dot: entry.dot,
                accent: entry.accent.map(css_colour).unwrap_or_default(),
                has_favicon,
                count: entry.count, // --- tab stacks and workspaces (T10) ---
                emoji: entry.emoji.clone().unwrap_or_default(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_edit_handoff_requires_focus_or_uncommitted_text() {
        assert!(!should_handoff_edit(false, "https://a.test", "https://a.test"));
        assert!(should_handoff_edit(true, "https://a.test", "https://a.test"));
        assert!(should_handoff_edit(false, "https://typed.test", "https://a.test"));
    }

    #[test]
    fn handoff_shows_the_loading_address_unless_the_user_edited_the_field() {
        assert_eq!(handoff_text("", false, "example.com/"), "example.com/", "pristine blank field: show the target");
        assert_eq!(handoff_text("", true, "example.com/"), "", "the user cleared it: keep it cleared");
        assert_eq!(handoff_text("typed", false, "example.com/"), "typed");
        assert_eq!(handoff_text("typed", true, "example.com/"), "typed");
        assert_eq!(handoff_text("", false, ""), "", "a new tab stays blank");
    }
    use crate::chrome::layout::{Metrics, Rect, compute_in_frame};

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
    fn explicit_value_beats_the_build_default_and_unknown_values_use_it() {
        // A build that defaults to the HTML chrome (the installer's release build).
        assert!(ui_mode(None, true));
        assert!(ui_mode(Some(""), true));
        assert!(ui_mode(Some("html "), true));
        assert!(ui_mode(Some("1"), true));
        assert!(ui_mode(Some("HTML"), true));
        assert!(!ui_mode(Some("native"), true));
        assert!(!ui_mode(Some("NATIVE"), true));
        assert!(!ui_mode(Some("gdi"), true));
        // A plain build keeps the GDI chrome unless asked.
        assert!(!ui_mode(None, false));
        assert!(!ui_mode(Some("native"), false));
        assert!(ui_mode(Some("html"), false));
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
    fn toolbar_placement_uses_window_geometry_in_css_pixels() {
        for (dpi, physical, expected) in [
            (120, 13, 10), // 125%
            (144, 16, 11), // 150%
            (192, 19, 10), // 200%, half rounds up
        ] {
            assert_eq!(css_px_from_physical(physical, dpi), expected, "dpi={dpi}");
        }
        for dpi in [96, 120, 144, 192] {
            let layout = compute_in_frame(1600, 900, dpi, 3, 0, false);
            let (pill_x, pill_w, menu_x) = toolbar_css_placement(&layout, dpi).expect("toolbar");
            let bounds = toolbar_bounds(&layout).expect("toolbar bounds");
            assert_eq!(pill_x, css_px_from_physical(layout.omnibox.left - bounds.left, dpi));
            assert_eq!(pill_w, css_px_from_physical(layout.omnibox.width(), dpi));
            assert_eq!(menu_x, css_px_from_physical(layout.main_menu_button.left - bounds.left, dpi));
        }
    }

    #[test]
    fn the_download_button_slot_comes_out_of_the_pill_and_nothing_else() {
        assert_eq!(DL_BUTTON_SLOT, 32);
        for dpi in [96, 120, 144, 192] {
            for width in [760, 1100, 1600] {
                let layout = compute_in_frame(width, 800, dpi, 3, 0, false);
                let (x, w, menu) = toolbar_css_placement(&layout, dpi).expect("placement");
                assert_eq!(toolbar_css_placement_with_slot(&layout, dpi, 0), Some((x, w, menu)));
                let (sx, sw, smenu) = toolbar_css_placement_with_slot(&layout, dpi, DL_BUTTON_SLOT).expect("slotted");
                assert_eq!((sx, smenu), (x, menu), "only the pill's width changes dpi={dpi} width={width}");
                assert_eq!(sw, (w - DL_BUTTON_SLOT).max(1), "dpi={dpi} width={width}");
                // The dropdown under the omnibox follows the narrower pill, left edge unchanged.
                let pill = toolbar_pill_rect(&layout, dpi).expect("pill");
                let narrow = toolbar_pill_rect_with_slot(&layout, dpi, DL_BUTTON_SLOT).expect("narrow pill");
                assert_eq!((narrow.left, narrow.top, narrow.bottom), (pill.left, pill.top, pill.bottom));
                assert!(narrow.right < pill.right, "dpi={dpi} width={width}");
            }
        }
        // A toolbar with no room never reaches a zero or negative pill.
        let layout = compute_in_frame(760, 800, 96, 3, 0, false);
        assert_eq!(toolbar_css_placement_with_slot(&layout, 96, 100_000).map(|(_, w, _)| w), Some(1));
    }

    #[test]
    fn painted_pill_rect_snaps_each_edge_and_stays_within_a_pixel_of_the_layout() {
        for dpi in [96, 120, 144, 168, 192] {
            for width in [760, 1100, 1600] {
                let layout = compute_in_frame(width, 800, dpi, 3, 0, false);
                let pill = toolbar_pill_rect(&layout, dpi).expect("pill");
                let bounds = toolbar_bounds(&layout).expect("toolbar");
                let (x, w, _) = toolbar_css_placement(&layout, dpi).expect("placement");
                let snap = |css: i32| (f64::from(css) * f64::from(dpi) / 96.0).round() as i32;
                assert_eq!(pill.left, bounds.left + snap(x), "dpi={dpi} width={width}");
                assert_eq!(pill.right, bounds.left + snap(x + w), "dpi={dpi} width={width}");
                assert_eq!(
                    pill.bottom - bounds.top,
                    snap((Metrics::STRIP_HEIGHT - Metrics::OMNIBOX_HEIGHT) / 2 + Metrics::OMNIBOX_HEIGHT),
                    "the 26px pill is centred in the toolbar row"
                );
                for (painted, laid_out) in [
                    (pill.left, layout.omnibox.left),
                    (pill.right, layout.omnibox.right),
                    (pill.top, layout.omnibox.top),
                    (pill.bottom, layout.omnibox.bottom),
                ] {
                    assert!((painted - laid_out).abs() <= 1, "dpi={dpi} width={width}: {painted} vs {laid_out}");
                }
                if dpi == 96 {
                    assert_eq!((pill.left, pill.top, pill.right, pill.bottom), (layout.omnibox.left, layout.omnibox.top, layout.omnibox.right, layout.omnibox.bottom));
                }
            }
        }
    }

    #[test]
    fn html_bounds_and_toolbar_placement_follow_resize_and_dpi_relayout() {
        for dpi in [96, 120, 144, 192] {
            let narrow = compute_in_frame(1100, 700, dpi, 3, 0, false);
            let wide = compute_in_frame(1600, 900, dpi, 3, 0, true);
            let narrow_toolbar = toolbar_bounds(&narrow).expect("narrow toolbar");
            let wide_toolbar = toolbar_bounds(&wide).expect("wide toolbar");
            assert_eq!(wide_toolbar.top, wide.strip.top, "dpi={dpi}");
            assert_eq!(wide_toolbar.bottom, wide.strip.bottom, "dpi={dpi}");
            assert_ne!(narrow_toolbar.right, wide_toolbar.right, "resize updates toolbar bounds dpi={dpi}");
            assert_ne!(toolbar_css_placement(&narrow, dpi), toolbar_css_placement(&wide, dpi), "resize updates pill state dpi={dpi}");
            assert_eq!(tab_row_bounds(&wide).unwrap().top, wide.tab_row.top);
            assert_eq!(rail_bounds(&wide).unwrap().top, wide.rail.top);
            let sidebar = crate::vertical_tabs::SidebarLayout::compute_in_frame(1600, 900, dpi, 260, true);
            assert_eq!(sidebar_bounds(&sidebar).unwrap().top, Metrics::new(dpi).px(Metrics::TOP_MARGIN));
        }
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
    fn state_diff_does_not_advance_the_cache() {
        let mut last = None;
        assert!(state_differs(&last, "{\"a\":1}"));
        assert_eq!(last, None);
        last = Some("{\"a\":1}".to_owned());
        assert!(!state_differs(&last, "{\"a\":1}"));
        assert!(state_differs(&last, "{\"a\":2}"));
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
    fn tab_row_bounds_is_the_tab_row_up_to_the_new_tab_button() {
        let mut layout = compute_in_frame(1200, 700, 96, 3, 0, false);
        let bounds = tab_row_bounds(&layout).expect("horizontal layout has a tab row");
        assert_eq!(bounds.left, layout.tab_row.left);
        assert_eq!(bounds.top, layout.tab_row.top);
        assert_eq!(bounds.bottom, layout.tab_row.bottom);
        // "+" sits at the right end of the full-width row, so the strip stops one gap before it.
        let gap = layout.metrics.px(crate::chrome::layout::Metrics::ISLAND_GAP);
        assert!(layout.tab_row.contains_rect(layout.new_tab_button));
        assert_eq!(bounds.right, layout.new_tab_button.left - gap);
        assert_eq!(bounds.right, layout.tab_zone.right);
        // Without a "+" in the row (it dropped out) the strip runs to the end of the row.
        layout.new_tab_button = Rect::default();
        assert_eq!(tab_row_bounds(&layout).map(|bounds| bounds.right), Some(layout.tab_row.right));
        layout = compute_in_frame(1200, 700, 96, 3, 0, false);
        // Three tabs do not overflow, so no chevron takes a slot.
        assert!(layout.tab_menu_button.is_empty());
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
    fn tab_row_bounds_stops_one_island_gap_before_the_overflow_chevron() {
        use crate::chrome::layout::Metrics;
        for dpi in [96, 120, 144, 192] {
            let mut layout = compute_in_frame(1200, 700, dpi, 40, 0, false);
            let button = layout.tab_menu_button;
            assert!(!button.is_empty(), "forty tabs overflow, dpi={dpi}");
            let gap = layout.metrics.px(Metrics::ISLAND_GAP);
            let bounds = tab_row_bounds(&layout).expect("overflowing row keeps a strip");
            assert_eq!(bounds.left, layout.tab_row.left, "dpi={dpi}");
            assert_eq!(bounds.top, layout.tab_row.top, "dpi={dpi}");
            assert_eq!(bounds.bottom, layout.tab_row.bottom, "dpi={dpi}");
            assert_eq!(bounds.right, button.left - gap, "dpi={dpi}");
            assert!(bounds.right < button.left, "dpi={dpi}");

            // A chevron that leaves no room for the strip leaves no strip.
            layout.tab_menu_button = Rect::new(layout.tab_row.left, 41, layout.tab_row.left + 26, 67);
            assert!(tab_row_bounds(&layout).is_none(), "dpi={dpi}");
        }
    }

    // --- vertical sidebar (lane S) ---
    #[test]
    fn html_sidebar_bounds_end_before_the_native_resize_handle() {
        for dpi in [96, 120, 144, 192] {
            let layout = crate::vertical_tabs::SidebarLayout::compute_in_frame(1200, 700, dpi, 260, false);
            let bounds = sidebar_bounds(&layout).expect("usable sidebar area");
            assert_eq!(bounds.left, layout.sidebar.left, "dpi={dpi}");
            assert_eq!(bounds.top, layout.sidebar.top, "dpi={dpi}");
            assert_eq!(bounds.right, layout.handle.left, "dpi={dpi}");
            assert_eq!(bounds.bottom, layout.sidebar.bottom, "dpi={dpi}");
        }
        let mut narrow = crate::vertical_tabs::SidebarLayout::compute(20, 300, 96, 260, false);
        narrow.handle.left = narrow.sidebar.left;
        assert!(sidebar_bounds(&narrow).is_none());
        narrow.sidebar.bottom = narrow.sidebar.top;
        assert!(sidebar_bounds(&narrow).is_none());
    }

    #[test]
    fn vertical_html_regions_match_native_geometry_without_overlaps_at_supported_dpis() {
        for dpi in [96, 120, 144, 192] {
            let width = 1600;
            let height = 1000;
            let (layout, native_sidebar) = crate::chrome::layout::compute_vertical_in_frame(width, height, dpi, 12, 0, false, 260);
            let sidebar = crate::chrome::layout::Rect::from(sidebar_bounds(&native_sidebar).expect("sidebar"));
            let toolbar = toolbar_bounds(&layout).expect("toolbar");
            let toolbar = crate::chrome::layout::Rect::from(toolbar);
            let rail = crate::chrome::layout::Rect::from(rail_bounds(&layout).expect("rail"));
            let caption = layout.caption_area;
            let new_tab = layout.new_tab_button;
            assert!(tab_row_bounds(&layout).is_none(), "dpi={dpi}: vertical mode must not expose a horizontal row");
            let disjoint = |a: crate::chrome::layout::Rect, b: crate::chrome::layout::Rect| {
                a.right <= b.left || b.right <= a.left || a.bottom <= b.top || b.bottom <= a.top
            };
            for (name_a, a, name_b, b) in [
                ("toolbar", toolbar, "sidebar", sidebar),
                ("toolbar", toolbar, "rail", rail),
                ("toolbar", toolbar, "caption", caption),
                ("toolbar", toolbar, "new-tab", new_tab),
                ("sidebar", sidebar, "rail", rail),
                ("sidebar", sidebar, "caption", caption),
                ("sidebar", sidebar, "new-tab", new_tab),
                ("rail", rail, "caption", caption),
                ("rail", rail, "new-tab", new_tab),
                ("caption", caption, "new-tab", new_tab),
            ] {
                assert!(disjoint(a, b), "dpi={dpi}: {name_a} overlaps {name_b}: {a:?} vs {b:?}");
            }
            assert_eq!(sidebar.top, crate::chrome::layout::Metrics::new(dpi).px(crate::chrome::layout::Metrics::TOP_MARGIN));
            assert!(sidebar.left >= rail.right, "dpi={dpi}");
        }
    }
    // --- end vertical sidebar ---

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
            private: false,
            marker: None,
            group,
            has_favicon: id == 1,
            marks: Default::default(),
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

    fn entry(key: RailKey, icon: RailIcon, label: &str) -> RailEntry<'static> {
        RailEntry {
            key,
            icon,
            label: label.to_owned(),
            active: false,
            dot: false,
            accent: None,
            favicon: None,
            count: 0,
            emoji: None,
        }
    }

    #[test]
    fn rail_bounds_is_the_full_rail_pill() {
        let layout = compute_in_frame(1200, 700, 96, 3, 0, false);
        let bounds = rail_bounds(&layout).expect("wide window has a rail");
        assert_eq!(bounds.left, layout.rail.left);
        assert_eq!(bounds.top, layout.rail.top);
        assert_eq!(bounds.right, layout.rail.right);
        assert_eq!(bounds.bottom, layout.rail.bottom);
        assert!(bounds.bottom > bounds.top && bounds.right > bounds.left);
        let mut empty = layout.clone();
        empty.rail = Rect::default();
        assert!(rail_bounds(&empty).is_none());
    }

    #[test]
    fn rail_keys_convert_both_ways_and_are_checked_against_the_model() {
        let model = [
            RailKey::Home,
            RailKey::Workspace(7),
            RailKey::App(3),
            RailKey::Menu,
        ];
        for key in model {
            let wire = rail_key_to_wire(key);
            assert_eq!(rail_key_from_wire(wire, &model), Some(key), "{key:?}");
            assert_eq!(rail_key_from_wire(wire, &model[..0]), None, "{key:?} needs the model");
        }
        assert_eq!(rail_key_to_wire(RailKey::Workspace(7)).to_wire(), "ws:7");
        assert_eq!(rail_key_to_wire(RailKey::AddApp).to_wire(), "addapp");
        // Keys the model does not hold are rejected, including near misses.
        for raw in ["ws:8", "app:2", "ws:70", "bookmarks"] {
            let wire = RailKeyWire::parse(raw).expect("grammar");
            assert_eq!(rail_key_from_wire(wire, &model), None, "{raw}");
        }
        // Every key kind has a distinct spelling.
        let every = [
            RailKey::Home,
            RailKey::Customize,
            RailKey::Workspace(1),
            RailKey::AddWorkspace,
            RailKey::App(1),
            RailKey::AddApp,
            RailKey::Bookmarks,
            RailKey::History,
            RailKey::Menu,
        ];
        let spellings: HashSet<String> = every.iter().map(|k| rail_key_to_wire(*k).to_wire()).collect();
        assert_eq!(spellings.len(), every.len());
    }

    #[test]
    fn rail_icons_map_to_the_wire_names() {
        let cases = [
            (RailIcon::Home, "home"),
            (RailIcon::Customize, "customize"),
            (RailIcon::Plus, "plus"),
            (RailIcon::Heart, "heart"),
            (RailIcon::Clock, "clock"),
            (RailIcon::Dots, "dots"),
            (RailIcon::AppTile, "apptile"),
            (RailIcon::Letter, "letter"),
            (RailIcon::Glyph(WorkspaceGlyph::Briefcase), "g:briefcase"),
            (RailIcon::Glyph(WorkspaceGlyph::Home), "g:home"),
            (RailIcon::Glyph(WorkspaceGlyph::Code), "g:code"),
            (RailIcon::Glyph(WorkspaceGlyph::Book), "g:book"),
            (RailIcon::Glyph(WorkspaceGlyph::Cart), "g:cart"),
            (RailIcon::Glyph(WorkspaceGlyph::Game), "g:game"),
            (RailIcon::Glyph(WorkspaceGlyph::Music), "g:music"),
            (RailIcon::Glyph(WorkspaceGlyph::Heart), "g:heart"),
            (RailIcon::Glyph(WorkspaceGlyph::Star), "g:star"),
            (RailIcon::Glyph(WorkspaceGlyph::Bolt), "g:bolt"),
            (RailIcon::Glyph(WorkspaceGlyph::Globe), "g:globe"),
            (RailIcon::Glyph(WorkspaceGlyph::Leaf), "g:leaf"),
        ];
        for (icon, wire) in cases {
            assert_eq!(rail_icon_wire(icon), wire, "{icon:?}");
        }
        // Every glyph the rail can hold has a name in the contract's list.
        for glyph in WorkspaceGlyph::ALL {
            assert!(rail_icon_wire(RailIcon::Glyph(glyph)).starts_with("g:"));
        }
    }

    #[test]
    fn rail_items_follow_the_native_entries_in_order() {
        let mut workspace = entry(RailKey::Workspace(7), RailIcon::Glyph(WorkspaceGlyph::Code), "Work");
        workspace.active = true;
        workspace.accent = Some(Rgba::hex(0x4385D1, 0xFF));
        let mut dotted = entry(RailKey::History, RailIcon::Clock, "History");
        dotted.dot = true;
        let entries = vec![
            entry(RailKey::Home, RailIcon::Home, "Home"),
            workspace,
            entry(RailKey::App(3), RailIcon::Letter, "Mail"),
            dotted,
        ];
        let items = rail_items(&entries, None, |_| None);
        let keys: Vec<&str> = items.iter().map(|item| item.key.as_str()).collect();
        assert_eq!(keys, ["home", "ws:7", "app:3", "history"]);
        assert_eq!(items[1].icon, "g:code");
        assert_eq!(items[1].label, "Work");
        assert!(items[1].active && !items[0].active);
        assert_eq!(items[1].accent, "#4385d1");
        assert_eq!(items[0].accent, "", "no accent means the theme accent");
        assert!(items[3].dot && !items[1].dot);
        assert!(items.iter().all(|item| !item.has_favicon));
    }

    #[test]
    fn an_open_panel_shows_its_button_as_active() {
        let entries = vec![
            entry(RailKey::Bookmarks, RailIcon::Heart, "Bookmarks"),
            entry(RailKey::Workspace(1), RailIcon::Glyph(WorkspaceGlyph::Star), "A"),
        ];
        let items = rail_items(&entries, Some(RailKey::Bookmarks), |_| None);
        assert!(items[0].active, "open panel");
        assert!(!items[1].active);
        let none = rail_items(&entries, None, |_| None);
        assert!(!none[0].active && !none[1].active);
    }

    #[test]
    fn an_app_without_a_sendable_favicon_falls_back_to_the_letter_tile() {
        let app = entry(RailKey::App(4), RailIcon::AppTile, "Mail");
        let png = {
            let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
            bytes.extend_from_slice(&[0, 0, 0, 13]);
            bytes.extend_from_slice(b"IHDR");
            bytes.extend_from_slice(&[0, 0, 0, 16, 0, 0, 0, 16, 8, 6, 0, 0, 0]);
            bytes
        };
        let entries = vec![app.clone(), entry(RailKey::Home, RailIcon::Home, "Home")];
        // Sendable PNG: the app keeps its tile and reports a favicon.
        let with = rail_items(&entries, None, |key| (key == RailKey::App(4)).then_some(png.as_slice()));
        assert_eq!(with[0].icon, "apptile");
        assert!(with[0].has_favicon);
        // Bytes that are not a PNG: no favicon, so the letter tile.
        let gif: &[u8] = b"GIF89a";
        let without = rail_items(&entries, None, |key| (key == RailKey::App(4)).then_some(gif));
        assert_eq!(without[0].icon, "letter");
        assert!(!without[0].has_favicon);
        // A letter tile stays a letter tile, and an entry with no bytes has no favicon.
        assert_eq!(without[1].icon, "home");
        assert!(!without[1].has_favicon);
    }
}
