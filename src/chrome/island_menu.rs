//! Pure model and geometry for the tab island menu.
//!
//! Hovering an island's bar opens a card below it with a name field (with a clear button), a row of
//! colour swatches, the island's tabs, and a full-width "Close Tab Island" button. Nothing here
//! touches Win32: the win32 popup draws from [`IslandLayout`] and resolves input with [`hit_test`].
//!
//! ```text
//! +----------------------------------------+
//! | [Tab island name             (x)]      |  name field (native EDIT inside)
//! | (o) (o) (o) (o) (o) (o) (o) (o) (o)    |  swatches, default first
//! | (fav) Title of a tab in the island     |  member rows, active row emphasised
//! | (fav) Another tab                      |
//! | [ Close Tab Island · 2 Tabs ]          |  full-width button
//! +----------------------------------------+
//! ```

use crate::{
    chrome::layout::{Metrics, Rect},
    tab_groups::COLOUR_COUNT,
};

/// Placeholder text for an island without a name. Shown by the name field while it is empty.
pub const PLACEHOLDER: &str = "Tab island name";
/// Swatch slots: slot 0 is the theme accent (no stored colour), then one slot per colour id.
pub const SWATCH_COUNT: usize = COLOUR_COUNT + 1;
/// Member rows drawn at most. Extra members are counted in [`IslandLayout::hidden_rows`].
pub const MAX_ROWS: usize = 12;

/// Logical (96 dpi) sizes. Use [`Metrics::px`] to scale them.
pub const CARD_WIDTH: i32 = 280;
pub const CARD_RADIUS: i32 = 12;
pub const PAD: i32 = 10;
/// One text line tall: the field is a single-line input, not a multi-line box.
pub const FIELD_HEIGHT: i32 = 28;
/// Height of the native EDIT: one line of the message font. It is centred vertically in the field,
/// because a single-line EDIT draws its text from the top of its own rectangle.
pub const EDIT_LINE_HEIGHT: i32 = 16;
/// Left inset of the text inside the name field, and the gap kept before the clear button.
pub const FIELD_TEXT_INSET: i32 = 10;
pub const CLEAR_SIZE: i32 = 18;
pub const CLEAR_INSET: i32 = 6;
pub const SWATCH: i32 = 20;
pub const SWATCH_GAP: i32 = 6;
pub const SECTION_GAP: i32 = 8;
pub const ROW_HEIGHT: i32 = 26;
pub const MORE_HEIGHT: i32 = 18;
pub const BUTTON_HEIGHT: i32 = 30;
/// Gap between the bar's bottom and the card top.
pub const ANCHOR_GAP: i32 = 4;
/// Minimum distance kept between the card and the client edges.
pub const EDGE_MARGIN: i32 = 8;

/// One member row: a tab in the island.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IslandRow {
    pub title: String,
    pub favicon_present: bool,
    pub active: bool,
}

/// What the menu shows, independent of where it is drawn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IslandModel {
    /// Cleaned island name, or `None` when the island has no name yet.
    pub name: Option<String>,
    /// Stored colour id, or `None` for the theme accent.
    pub colour: Option<u8>,
    /// Every member, in model order. Rows beyond [`MAX_ROWS`] are not drawn.
    pub rows: Vec<IslandRow>,
}

impl IslandModel {
    /// Label for the close button, from the member count.
    pub fn close_label(&self) -> String {
        close_label(self.rows.len())
    }
}

/// "Close Tab Island · 1 Tab" or "Close Tab Island · 4 Tabs".
pub fn close_label(count: usize) -> String {
    let noun = if count == 1 { "Tab" } else { "Tabs" };
    format!("Close Tab Island · {count} {noun}")
}

/// Swatch slot for a stored colour: slot 0 is the theme accent, and colour `id` is slot `id + 1`.
pub fn swatch_slot(colour: Option<u8>) -> usize {
    colour.map_or(0, |id| usize::from(id) + 1)
}

/// Stored colour for a swatch slot, or `None` for the theme accent and for slots out of range.
pub fn swatch_colour(slot: usize) -> Option<u8> {
    match slot {
        0 => None,
        slot if slot < SWATCH_COUNT => u8::try_from(slot - 1).ok(),
        _ => None,
    }
}

/// Device-pixel geometry of the menu, in the same client space as the anchor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IslandLayout {
    pub card: Rect,
    /// The name field's full box, with the clear button inside it at its right.
    pub field: Rect,
    /// Where the native EDIT goes: the field minus its inset and the clear button.
    pub edit: Rect,
    pub clear: Rect,
    /// One rect per swatch slot, in [`swatch_slot`] order.
    pub swatches: [Rect; SWATCH_COUNT],
    /// Drawn member rows, top to bottom. Length is `min(row_count, MAX_ROWS)`.
    pub rows: Vec<Rect>,
    /// Member rows beyond [`MAX_ROWS`]. They are counted but not drawn.
    pub hidden_rows: usize,
    /// "+N more" line below the drawn rows. `None` when no rows are hidden.
    pub more: Option<Rect>,
    pub close: Rect,
}

impl IslandLayout {
    /// Moves every rectangle by `(-dx, -dy)`, so a layout built in one client space can be used in
    /// a popup whose origin is `(dx, dy)` in that space.
    pub fn translated(&self, dx: i32, dy: i32) -> Self {
        let shift = move |r: Rect| Rect::new(r.left - dx, r.top - dy, r.right - dx, r.bottom - dy);
        Self {
            card: shift(self.card),
            field: shift(self.field),
            edit: shift(self.edit),
            clear: shift(self.clear),
            swatches: self.swatches.map(shift),
            rows: self.rows.iter().copied().map(shift).collect(),
            hidden_rows: self.hidden_rows,
            more: self.more.map(shift),
            close: shift(self.close),
        }
    }
}

/// Lays out the menu under `anchor` (the island's bar), left-aligned to it and clamped so the card
/// stays inside `client_width`.
pub fn layout(anchor: Rect, client_width: i32, dpi: u32, row_count: usize) -> IslandLayout {
    let m = Metrics::new(dpi);
    let pad = m.px(PAD);
    let margin = m.px(EDGE_MARGIN);

    let row_visible = row_count.min(MAX_ROWS);
    let hidden_rows = row_count - row_visible;

    let width = m.px(CARD_WIDTH).min((client_width - 2 * margin).max(0));
    let mut left = anchor.left;
    if left + width > client_width - margin {
        left = client_width - margin - width;
    }
    left = left.max(margin);
    let top = anchor.bottom + m.px(ANCHOR_GAP);

    let inner_left = left + pad;
    let inner_right = left + width - pad;
    let inner_width = (inner_right - inner_left).max(0);

    let field_top = top + pad;
    let field_height = m.px(FIELD_HEIGHT);
    let field = Rect::new(
        inner_left,
        field_top,
        inner_left + inner_width,
        field_top + field_height,
    );
    let clear_size = m.px(CLEAR_SIZE);
    let clear_left = field.right - m.px(CLEAR_INSET) - clear_size;
    let clear_top = field.top + (field_height - clear_size) / 2;
    let clear = Rect::new(
        clear_left,
        clear_top,
        clear_left + clear_size,
        clear_top + clear_size,
    );
    let line = m.px(EDIT_LINE_HEIGHT).min(field_height);
    let edit_top = field.top + (field_height - line) / 2;
    let edit = Rect::new(
        field.left + m.px(FIELD_TEXT_INSET),
        edit_top,
        (clear.left - m.px(CLEAR_INSET)).max(field.left),
        edit_top + line,
    );

    // Swatch row: default plus one per colour, left-aligned and non-overlapping.
    let swatch = m.px(SWATCH);
    let swatch_gap = m.px(SWATCH_GAP);
    let swatch_top = field.bottom + m.px(SECTION_GAP);
    let swatches = std::array::from_fn(|index| {
        let x = inner_left + index as i32 * (swatch + swatch_gap);
        Rect::new(x, swatch_top, x + swatch, swatch_top + swatch)
    });

    // Member rows: equal height, stacked after the swatch row.
    let row_height = m.px(ROW_HEIGHT);
    let rows_top = swatch_top + swatch + m.px(SECTION_GAP);
    let rows: Vec<Rect> = (0..row_visible)
        .map(|i| {
            let y = rows_top + i as i32 * row_height;
            Rect::new(inner_left, y, inner_left + inner_width, y + row_height)
        })
        .collect();

    let rows_bottom = rows.last().map_or(rows_top, |row| row.bottom);
    let (more, after_rows) = if hidden_rows > 0 {
        let more = Rect::new(
            inner_left,
            rows_bottom,
            inner_left + inner_width,
            rows_bottom + m.px(MORE_HEIGHT),
        );
        (Some(more), more.bottom)
    } else {
        (None, rows_bottom)
    };

    let close_top = after_rows + m.px(SECTION_GAP);
    let close = Rect::new(
        inner_left,
        close_top,
        inner_left + inner_width,
        close_top + m.px(BUTTON_HEIGHT),
    );
    let card = Rect::new(left, top, left + width, close.bottom + pad);

    IslandLayout {
        card,
        field,
        edit,
        clear,
        swatches,
        rows,
        hidden_rows,
        more,
        close,
    }
}

/// What a click or hover point lands on inside the menu.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IslandHit {
    None,
    /// The name field, outside the clear button.
    Field,
    Clear,
    /// Swatch slot, index in `0..SWATCH_COUNT`.
    Swatch(usize),
    /// Member row, index into [`IslandLayout::rows`].
    Row(usize),
    Close,
}

/// Resolves a client-relative point against the menu. Points outside the card return `None`.
pub fn hit_test(layout: &IslandLayout, x: i32, y: i32) -> IslandHit {
    if !layout.card.contains(x, y) {
        return IslandHit::None;
    }
    if layout.clear.contains(x, y) {
        return IslandHit::Clear;
    }
    if layout.field.contains(x, y) {
        return IslandHit::Field;
    }
    if let Some(slot) = layout.swatches.iter().position(|r| r.contains(x, y)) {
        return IslandHit::Swatch(slot);
    }
    if let Some(row) = layout.rows.iter().position(|r| r.contains(x, y)) {
        return IslandHit::Row(row);
    }
    if layout.close.contains(x, y) {
        return IslandHit::Close;
    }
    IslandHit::None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        chrome::paint::{group_bar_rect, group_marker_box, island_colour},
        theme,
    };

    const CLIENT_WIDTH: i32 = 900;
    const DPIS: [u32; 3] = [96, 144, 192];

    fn bar_anchor() -> Rect {
        Rect::new(CLIENT_WIDTH - 200, 40, CLIENT_WIDTH - 196, 64)
    }

    fn intersects(a: Rect, b: Rect) -> bool {
        a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
    }

    fn centre(r: Rect) -> (i32, i32) {
        ((r.left + r.right) / 2, (r.top + r.bottom) / 2)
    }

    #[test]
    fn close_label_matches_the_tab_count_wording() {
        assert_eq!(close_label(1), "Close Tab Island · 1 Tab");
        assert_eq!(close_label(4), "Close Tab Island · 4 Tabs");
        assert_eq!(close_label(0), "Close Tab Island · 0 Tabs");
    }

    #[test]
    fn swatch_slots_round_trip_and_default_is_slot_zero() {
        assert_eq!(swatch_slot(None), 0);
        assert_eq!(swatch_colour(0), None);
        for id in 0..COLOUR_COUNT as u8 {
            let slot = swatch_slot(Some(id));
            assert_eq!(slot, usize::from(id) + 1);
            assert_eq!(swatch_colour(slot), Some(id));
        }
        assert_eq!(swatch_colour(SWATCH_COUNT), None);
    }

    #[test]
    fn menu_stays_inside_client_width_at_every_dpi() {
        for dpi in DPIS {
            let margin = Metrics::new(dpi).px(EDGE_MARGIN);
            for anchor in [bar_anchor(), Rect::new(-30, 40, 4, 64)] {
                let l = layout(anchor, CLIENT_WIDTH, dpi, 3);
                assert!(l.card.left >= margin, "dpi {dpi}: left {}", l.card.left);
                assert!(l.card.right <= CLIENT_WIDTH - margin, "dpi {dpi}");
            }
        }
    }

    #[test]
    fn menu_sits_below_the_bar_and_left_aligns_to_it() {
        let anchor = Rect::new(100, 40, 104, 64);
        let l = layout(anchor, CLIENT_WIDTH, 96, 0);
        assert_eq!(l.card.left, 100);
        assert_eq!(l.card.top, anchor.bottom + ANCHOR_GAP);
    }

    #[test]
    fn parts_do_not_overlap_and_sit_inside_the_card() {
        for dpi in DPIS {
            let l = layout(bar_anchor(), CLIENT_WIDTH, dpi, 5);
            // The clear button sits inside the field by design, so it is checked against the field.
            let mut parts: Vec<Rect> = vec![l.field, l.close];
            parts.extend(l.swatches);
            parts.extend(l.rows.iter().copied());
            for (i, a) in parts.iter().enumerate() {
                assert!(l.card.contains_rect(*a), "dpi {dpi}: part {i} outside card");
                for b in parts.iter().skip(i + 1) {
                    assert!(!intersects(*a, *b), "dpi {dpi}: parts overlap");
                }
            }
            assert!(l.field.contains_rect(l.clear));
            assert!(l.card.contains_rect(l.edit));
            assert!(
                !intersects(l.edit, l.clear),
                "text must not run under clear"
            );
        }
    }

    #[test]
    fn swatch_row_has_one_slot_per_colour_plus_default() {
        let l = layout(bar_anchor(), CLIENT_WIDTH, 96, 1);
        assert_eq!(l.swatches.len(), SWATCH_COUNT);
        let (first, last) = (l.swatches[0], l.swatches[SWATCH_COUNT - 1]);
        assert!(first.left < last.left);
        assert!(l.swatches.iter().all(|s| s.top == l.swatches[0].top));
    }

    #[test]
    fn hit_test_finds_each_control_at_its_centre() {
        let l = layout(bar_anchor(), CLIENT_WIDTH, 96, 2);
        assert_eq!(
            hit_test(&l, centre(l.clear).0, centre(l.clear).1),
            IslandHit::Clear
        );
        assert_eq!(
            hit_test(&l, centre(l.close).0, centre(l.close).1),
            IslandHit::Close
        );
        let (x, y) = centre(l.swatches[3]);
        assert_eq!(hit_test(&l, x, y), IslandHit::Swatch(3));
        let (x, y) = centre(l.rows[1]);
        assert_eq!(hit_test(&l, x, y), IslandHit::Row(1));
        // Inside the field but left of the clear button is the field itself.
        let (x, y) = (l.edit.left + 2, centre(l.field).1);
        assert_eq!(
            (l.edit.top + l.edit.bottom) / 2,
            (l.field.top + l.field.bottom) / 2,
            "the one-line edit is centred in the field"
        );
        assert_eq!(hit_test(&l, x, y), IslandHit::Field);
    }

    #[test]
    fn hit_test_misses_gaps_and_outside_points() {
        let l = layout(bar_anchor(), CLIENT_WIDTH, 96, 1);
        assert_eq!(hit_test(&l, l.card.left - 1, l.card.top), IslandHit::None);
        assert_eq!(hit_test(&l, l.card.right, l.card.bottom), IslandHit::None);
        // The gap between the swatch row and the first member row.
        let gap_y = l.swatches[0].bottom + 1;
        assert_eq!(hit_test(&l, l.swatches[0].left, gap_y), IslandHit::None);
    }

    #[test]
    fn member_rows_are_capped_and_the_rest_are_counted() {
        let dpi = 96;
        let l = layout(bar_anchor(), CLIENT_WIDTH, dpi, MAX_ROWS + 5);
        assert_eq!(l.rows.len(), MAX_ROWS);
        assert_eq!(l.hidden_rows, 5);
        let more = l.more.expect("hidden rows need a more line");
        assert!(l.card.contains_rect(more));
        assert!(!intersects(more, l.close));

        let few = layout(bar_anchor(), CLIENT_WIDTH, dpi, 2);
        assert_eq!(few.rows.len(), 2);
        assert_eq!(few.hidden_rows, 0);
        assert_eq!(few.more, None);
    }

    #[test]
    fn card_height_is_the_sum_of_its_sections() {
        let dpi = 144;
        let m = Metrics::new(dpi);
        let l = layout(bar_anchor(), CLIENT_WIDTH, dpi, 3);
        let expected = m.px(PAD)
            + m.px(FIELD_HEIGHT)
            + m.px(SECTION_GAP)
            + m.px(SWATCH)
            + m.px(SECTION_GAP)
            + 3 * m.px(ROW_HEIGHT)
            + m.px(SECTION_GAP)
            + m.px(BUTTON_HEIGHT)
            + m.px(PAD);
        assert_eq!(l.card.height(), expected);
    }

    #[test]
    fn translated_layout_moves_every_rect_and_keeps_hits() {
        let l = layout(bar_anchor(), CLIENT_WIDTH, 96, 2);
        let moved = l.translated(l.card.left, l.card.top);
        assert_eq!(moved.card.left, 0);
        assert_eq!(moved.card.top, 0);
        let (x, y) = centre(moved.close);
        assert_eq!(hit_test(&moved, x, y), IslandHit::Close);
    }

    #[test]
    fn model_close_label_counts_every_member() {
        let model = IslandModel {
            name: None,
            colour: None,
            rows: (0..4)
                .map(|i| IslandRow {
                    title: format!("Tab {i}"),
                    favicon_present: true,
                    active: i == 0,
                })
                .collect(),
        };
        assert_eq!(model.close_label(), "Close Tab Island · 4 Tabs");
    }

    #[test]
    fn island_bar_uses_the_chosen_colour_or_the_theme_accent() {
        let accent = theme::palette(theme::Appearance::Light, true).tab_group_accent;
        assert_eq!(island_colour(None, accent), accent);
        assert_eq!(island_colour(Some(0), accent), theme::ISLAND_COLOURS[0]);
        assert_eq!(
            island_colour(Some(COLOUR_COUNT as u8 - 1), accent),
            theme::ISLAND_COLOURS[COLOUR_COUNT - 1]
        );
        // An id outside the palette falls back to the accent rather than panicking.
        assert_eq!(island_colour(Some(200), accent), accent);
    }

    #[test]
    fn island_bar_sits_in_its_own_box_at_the_leading_edge() {
        for dpi in DPIS {
            let m = Metrics::new(dpi);
            let island = Rect::new(300, 4, 300 + m.px(140), 4 + m.px(Metrics::ISLAND_HEIGHT));
            let marker_box = group_marker_box(island, m);
            assert!(marker_box.left >= island.left && marker_box.right <= island.right);
            assert_eq!(
                (marker_box.top, marker_box.bottom),
                (island.top, island.bottom),
                "dpi {dpi}: the box is as tall as a tab"
            );
            let bar = group_bar_rect(island, m);
            assert!(bar.left >= marker_box.left && bar.right <= marker_box.right);
            assert!(bar.top >= marker_box.top && bar.bottom <= marker_box.bottom);
            assert!(bar.width() >= 3 && bar.height() >= 10);
            assert!(bar.height() > bar.width() * 2, "dpi {dpi}: bar is tall");
            let centre_drift = (bar.top + bar.bottom) - (island.top + island.bottom);
            assert!(
                centre_drift.abs() <= 1,
                "dpi {dpi}: bar is vertically centred"
            );
            // The close target of the leading tab clears the box.
            let tabs = crate::chrome::layout::compute(1024, 700, dpi, 3, false);
            let tab = tabs.tab(0).unwrap();
            let parts = tabs.group_leader_parts(tab);
            let box_ = group_marker_box(tab, m);
            assert!(parts.close.left > box_.right && parts.close.right <= tab.right);
            assert!(parts.favicon.left > parts.close.right);
        }
    }

    #[test]
    fn bar_is_centred_in_its_box() {
        use crate::chrome::paint::{marker_facing_radius, marker_only_bar};
        for dpi in DPIS {
            let m = Metrics::new(dpi);
            let island = Rect::new(300, 4, 300 + m.px(140), 4 + m.px(Metrics::ISLAND_HEIGHT));
            let bar = group_bar_rect(island, m);
            let box_edges = group_marker_box(island, m);
            let (before, after) = (bar.left - box_edges.left, box_edges.right - bar.right);
            assert!((before - after).abs() <= 1, "dpi {dpi}: {before} vs {after}");
            let slot = Rect::new(50, 4, 50 + m.px(Metrics::GROUP_MARKER_BOX_WIDTH), 28);
            let bar = marker_only_bar(slot, m);
            assert!(((bar.left - slot.left) - (slot.right - bar.right)).abs() <= 1);
            assert!(marker_facing_radius(m) >= 2);
        }
    }

    #[test]
    fn collapsed_marker_slots_are_compact_boxes_beside_full_tabs() {
        let m = Metrics::new(96);
        let plain = crate::chrome::layout::compute(1024, 700, 96, 4, false);
        let compact = plain.with_compact_slots([false, true, false, false]);
        let marker = compact.tab(1).unwrap();
        assert_eq!(marker.width(), m.px(Metrics::GROUP_MARKER_BOX_WIDTH));
        // It hugs its neighbours, closer than two ordinary tabs sit.
        let gap = m.px(Metrics::GROUP_MARKER_GAP);
        assert!(gap < m.px(Metrics::ISLAND_GAP));
        assert_eq!(compact.tab(2).unwrap().left - marker.right, gap);
        assert_eq!(marker.left - compact.tab(0).unwrap().right, gap);
        // The space it saves goes to the full tabs.
        assert!(compact.tab(0).unwrap().width() >= plain.tab(0).unwrap().width());
    }
}
