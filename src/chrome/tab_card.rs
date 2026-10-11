//! Pure model and geometry for the tab hover card.
//!
//! Hovering a tab shows a card below it with a marker row (five marker swatches plus a "+"), the
//! hovered tab's title and domain, and the other tabs in its group. Nothing here touches Win32:
//! the painter draws from [`CardLayout`] and the input handler resolves clicks with [`hit_test`].
//!
//! ```text
//! +----------------------------------------+
//! | [H] [F] [B] [P] [E]  [+]               |  swatch row
//! | Title of the hovered tab               |  title line
//! | example.com                            |  domain line
//! | (fav) Other tab in the group       ___ |  member rows, linked rows underlined
//! | (fav) Another member                   |
//! +----------------------------------------+
//! ```

use crate::chrome::layout::{Metrics, Rect};

/// Number of marker choices. Swatch slots are `0..MARKER_COUNT`, followed by the "+" slot.
pub const MARKER_COUNT: usize = 5;

/// Marker choices in display order. Painters decide how each one looks.
pub const MARKERS: [Marker; MARKER_COUNT] = [
    Marker::Heart,
    Marker::Flame,
    Marker::Bag,
    Marker::Palm,
    Marker::Eyes,
];

/// Logical (96 dpi) card sizes. Use [`Metrics::px`] to scale them.
pub const CARD_WIDTH: i32 = 260;
pub const CARD_RADIUS: i32 = 10;
pub const PAD: i32 = 10;
pub const SWATCH: i32 = 26;
pub const SWATCH_GAP: i32 = 6;
pub const TITLE_HEIGHT: i32 = 20;
pub const DOMAIN_HEIGHT: i32 = 16;
pub const SECTION_GAP: i32 = 6;
pub const ROW_HEIGHT: i32 = 28;
/// The "+N more" line under the member rows. Only present when rows are hidden.
pub const MORE_HEIGHT: i32 = 18;
/// Gap between the anchor (tab) bottom and the card top.
pub const ANCHOR_GAP: i32 = 4;
/// Minimum distance kept between the card and the client edges.
pub const EDGE_MARGIN: i32 = 8;
/// Member rows drawn at most. Extra rows are counted in [`CardLayout::hidden_rows`].
pub const MAX_ROWS: usize = 8;

pub const NEW_TAB_TITLE: &str = "New tab";

/// A marker the user can attach to a tab.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Marker {
    Heart,
    Flame,
    Bag,
    Palm,
    Eyes,
}

impl Marker {
    /// Human-readable name, used for tooltips and accessibility text.
    pub fn label(self) -> &'static str {
        match self {
            Marker::Heart => "Heart",
            Marker::Flame => "Flame",
            Marker::Bag => "Bag",
            Marker::Palm => "Palm",
            Marker::Eyes => "Eyes",
        }
    }

    /// Stable id in `0..MARKER_COUNT`. Persisted in session state, so do not renumber.
    pub fn id(self) -> u8 {
        match self {
            Marker::Heart => 0,
            Marker::Flame => 1,
            Marker::Bag => 2,
            Marker::Palm => 3,
            Marker::Eyes => 4,
        }
    }

    /// Inverse of [`Marker::id`]. Returns `None` for ids outside `0..MARKER_COUNT`.
    pub fn from_id(id: u8) -> Option<Marker> {
        MARKERS.get(usize::from(id)).copied()
    }
}

/// Marker a swatch click leaves on the tab: choosing the active marker again clears it, and any
/// other choice replaces it.
pub fn toggle_marker(current: Option<Marker>, chosen: Marker) -> Option<Marker> {
    if current == Some(chosen) {
        None
    } else {
        Some(chosen)
    }
}

/// Marker after the "+" button: steps through [`MARKERS`] and wraps to no marker after the last.
pub fn next_marker(current: Option<Marker>) -> Option<Marker> {
    match current {
        None => MARKERS.first().copied(),
        Some(marker) => MARKERS.get(usize::from(marker.id()) + 1).copied(),
    }
}

/// One member row of the card: another tab in the hovered tab's group.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CardRow {
    pub tab_index: usize,
    pub title: String,
    pub favicon_present: bool,
    pub active: bool,
    /// Connected by a ctrl-click opener link; drawn with a thin accent underline.
    pub linked: bool,
}

/// Input describing the hovered tab. Kept free of App state so the model stays pure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TabCardInput {
    pub title: String,
    pub domain: Option<String>,
    pub marker: Option<Marker>,
}

/// What the card shows, independent of where it is drawn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CardModel {
    pub marker: Option<Marker>,
    pub title: String,
    pub domain: Option<String>,
    pub rows: Vec<CardRow>,
}

/// Builds the card contents. An empty or blank title falls back to "New tab", matching
/// `tabs::display_title`. The domain passes through unchanged.
pub fn build(hovered: &TabCardInput, members: &[CardRow]) -> CardModel {
    let title = if hovered.title.trim().is_empty() {
        NEW_TAB_TITLE.to_owned()
    } else {
        hovered.title.clone()
    };
    CardModel {
        marker: hovered.marker,
        title,
        domain: hovered.domain.clone(),
        rows: members.to_vec(),
    }
}

/// Device-pixel geometry of the card. Member rows are capped at [`MAX_ROWS`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CardLayout {
    pub card: Rect,
    /// One rect per marker, in [`MARKERS`] order.
    pub swatches: [Rect; MARKER_COUNT],
    pub new_marker: Rect,
    pub title: Rect,
    pub domain: Rect,
    /// Drawn member rows, top to bottom. Length is `min(row_count, MAX_ROWS)`.
    pub rows: Vec<Rect>,
    /// Member rows beyond [`MAX_ROWS`]. They are counted but not drawn.
    pub hidden_rows: usize,
    /// Line for the hidden-row count, below the drawn rows. `None` when no rows are hidden.
    pub more: Option<Rect>,
}

/// Lays out the card under `anchor` (the hovered tab's rect), left-aligned to it and clamped so
/// the card stays inside `client_width`.
pub fn layout(anchor: Rect, client_width: i32, dpi: u32, row_count: usize) -> CardLayout {
    let m = Metrics::new(dpi);
    let pad = m.px(PAD);
    let swatch = m.px(SWATCH);
    let swatch_gap = m.px(SWATCH_GAP);
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

    // Swatch row: six slots (five markers, then "+"), left-aligned and non-overlapping.
    let swatch_top = top + pad;
    let slot = |index: i32| {
        let x = inner_left + index * (swatch + swatch_gap);
        Rect::new(x, swatch_top, x + swatch, swatch_top + swatch)
    };
    let swatches = std::array::from_fn(|i| slot(i as i32));
    let new_marker = slot(MARKER_COUNT as i32);

    let title_top = swatch_top + swatch + m.px(SECTION_GAP);
    let title_height = m.px(TITLE_HEIGHT);
    let title = Rect::new(
        inner_left,
        title_top,
        inner_left + inner_width,
        title_top + title_height,
    );

    let domain_top = title_top + title_height;
    let domain_height = m.px(DOMAIN_HEIGHT);
    let domain = Rect::new(
        inner_left,
        domain_top,
        inner_left + inner_width,
        domain_top + domain_height,
    );

    // Member rows: equal height, stacked after the domain line.
    let rows_top = domain_top + domain_height + m.px(SECTION_GAP);
    let row_height = m.px(ROW_HEIGHT);
    let rows: Vec<Rect> = (0..row_visible)
        .map(|i| {
            let y = rows_top + i as i32 * row_height;
            Rect::new(inner_left, y, inner_left + inner_width, y + row_height)
        })
        .collect();

    let rows_bottom = rows.last().map_or(domain.bottom, |r| r.bottom);
    let (more, content_bottom) = if hidden_rows > 0 {
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
    let card = Rect::new(left, top, left + width, content_bottom + pad);

    CardLayout {
        card,
        swatches,
        new_marker,
        title,
        domain,
        rows,
        hidden_rows,
        more,
    }
}

/// What a click or hover point lands on inside the card.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CardHit {
    None,
    /// Marker swatch, index in `0..MARKER_COUNT`.
    Swatch(usize),
    NewMarker,
    /// Member row, index into [`CardLayout::rows`].
    Row(usize),
}

/// Resolves a client-relative point against the card. Points outside the card return `None`.
pub fn hit_test(layout: &CardLayout, x: i32, y: i32) -> CardHit {
    if !layout.card.contains(x, y) {
        return CardHit::None;
    }
    if let Some(i) = layout.swatches.iter().position(|r| r.contains(x, y)) {
        return CardHit::Swatch(i);
    }
    if layout.new_marker.contains(x, y) {
        return CardHit::NewMarker;
    }
    if let Some(i) = layout.rows.iter().position(|r| r.contains(x, y)) {
        return CardHit::Row(i);
    }
    CardHit::None
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENT_WIDTH: i32 = 800;
    const DPIS: [u32; 3] = [96, 144, 192];

    fn member(index: usize) -> CardRow {
        CardRow {
            tab_index: index,
            title: format!("Tab {index}"),
            favicon_present: true,
            active: false,
            linked: index % 2 == 0,
        }
    }

    fn hovered(title: &str) -> TabCardInput {
        TabCardInput {
            title: title.to_owned(),
            domain: Some("example.com".to_owned()),
            marker: Some(Marker::Bag),
        }
    }

    /// An anchor (tab rect) hugging the right edge of the client area.
    fn right_edge_anchor() -> Rect {
        Rect::new(CLIENT_WIDTH - 120, 62, CLIENT_WIDTH - 20, 90)
    }

    fn intersects(a: Rect, b: Rect) -> bool {
        a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
    }

    #[test]
    fn marker_id_round_trips() {
        for (index, marker) in MARKERS.iter().enumerate() {
            assert_eq!(usize::from(marker.id()), index);
            assert_eq!(Marker::from_id(marker.id()), Some(*marker));
            assert!(!marker.label().is_empty());
        }
        assert_eq!(Marker::from_id(MARKER_COUNT as u8), None);
        assert_eq!(Marker::from_id(u8::MAX), None);
        assert_eq!(Marker::Heart.label(), "Heart");
        assert_eq!(Marker::Eyes.label(), "Eyes");
    }

    #[test]
    fn build_falls_back_to_new_tab_for_blank_titles() {
        assert_eq!(build(&hovered(""), &[]).title, "New tab");
        assert_eq!(build(&hovered("   \t"), &[]).title, "New tab");
        assert_eq!(build(&hovered("Te Ara"), &[]).title, "Te Ara");
    }

    #[test]
    fn build_passes_domain_marker_and_rows_through() {
        let members = [member(1), member(2)];
        let model = build(&hovered("Docs"), &members);
        assert_eq!(model.domain.as_deref(), Some("example.com"));
        assert_eq!(model.marker, Some(Marker::Bag));
        assert_eq!(model.rows, members);

        let bare = build(
            &TabCardInput {
                title: "Docs".to_owned(),
                domain: None,
                marker: None,
            },
            &[],
        );
        assert_eq!(bare.domain, None);
        assert_eq!(bare.marker, None);
        assert!(bare.rows.is_empty());
    }

    #[test]
    fn card_stays_inside_client_width_near_right_edge() {
        for dpi in DPIS {
            let margin = Metrics::new(dpi).px(EDGE_MARGIN);
            let l = layout(right_edge_anchor(), CLIENT_WIDTH, dpi, 3);
            assert!(l.card.left >= margin, "dpi {dpi}: left {}", l.card.left);
            assert!(
                l.card.right <= CLIENT_WIDTH - margin,
                "dpi {dpi}: right {}",
                l.card.right
            );
        }
    }

    #[test]
    fn card_stays_inside_client_width_near_left_edge() {
        for dpi in DPIS {
            let margin = Metrics::new(dpi).px(EDGE_MARGIN);
            let anchor = Rect::new(-40, 62, 60, 90);
            let l = layout(anchor, CLIENT_WIDTH, dpi, 0);
            assert!(l.card.left >= margin, "dpi {dpi}: left {}", l.card.left);
            assert!(l.card.right <= CLIENT_WIDTH - margin);
        }
    }

    #[test]
    fn card_sits_below_anchor() {
        let anchor = Rect::new(100, 62, 200, 90);
        let l = layout(anchor, CLIENT_WIDTH, 96, 0);
        assert_eq!(l.card.left, 100);
        assert_eq!(l.card.top, anchor.bottom + ANCHOR_GAP);
    }

    #[test]
    fn swatches_do_not_overlap_and_sit_inside_card() {
        for dpi in DPIS {
            let l = layout(right_edge_anchor(), CLIENT_WIDTH, dpi, 2);
            let mut all: Vec<Rect> = l.swatches.to_vec();
            all.push(l.new_marker);
            for (i, a) in all.iter().enumerate() {
                assert!(l.card.contains_rect(*a), "dpi {dpi}: slot {i} outside card");
                for b in all.iter().skip(i + 1) {
                    assert!(!intersects(*a, *b), "dpi {dpi}: swatches overlap");
                }
            }
        }
    }

    #[test]
    fn member_rows_do_not_overlap_and_are_equal_height() {
        for dpi in DPIS {
            let l = layout(right_edge_anchor(), CLIENT_WIDTH, dpi, 5);
            assert_eq!(l.rows.len(), 5);
            let height = l.rows[0].height();
            assert_eq!(height, Metrics::new(dpi).px(ROW_HEIGHT));
            for (i, a) in l.rows.iter().enumerate() {
                assert_eq!(a.height(), height);
                assert!(l.card.contains_rect(*a));
                for b in l.rows.iter().skip(i + 1) {
                    assert!(!intersects(*a, *b), "dpi {dpi}: rows overlap");
                }
            }
        }
    }

    #[test]
    fn card_height_is_sum_of_parts() {
        let dpi = 144;
        let m = Metrics::new(dpi);
        let l = layout(right_edge_anchor(), CLIENT_WIDTH, dpi, 3);
        let expected = m.px(PAD)
            + m.px(SWATCH)
            + m.px(SECTION_GAP)
            + m.px(TITLE_HEIGHT)
            + m.px(DOMAIN_HEIGHT)
            + m.px(SECTION_GAP)
            + 3 * m.px(ROW_HEIGHT)
            + m.px(PAD);
        assert_eq!(l.card.height(), expected);
    }

    #[test]
    fn card_without_rows_ends_after_domain() {
        let dpi = 96;
        let l = layout(right_edge_anchor(), CLIENT_WIDTH, dpi, 0);
        assert!(l.rows.is_empty());
        assert_eq!(l.card.bottom, l.domain.bottom + Metrics::new(dpi).px(PAD));
    }

    #[test]
    fn hit_test_finds_swatch_and_new_marker_centres() {
        let l = layout(right_edge_anchor(), CLIENT_WIDTH, 96, 2);
        let c = |r: Rect| ((r.left + r.right) / 2, (r.top + r.bottom) / 2);

        let (x, y) = c(l.swatches[2]);
        assert_eq!(hit_test(&l, x, y), CardHit::Swatch(2));

        let (x, y) = c(l.new_marker);
        assert_eq!(hit_test(&l, x, y), CardHit::NewMarker);
    }

    #[test]
    fn hit_test_finds_row_centres() {
        let l = layout(right_edge_anchor(), CLIENT_WIDTH, 96, 3);
        for (i, r) in l.rows.iter().enumerate() {
            let (x, y) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
            assert_eq!(hit_test(&l, x, y), CardHit::Row(i));
        }
    }

    #[test]
    fn hit_test_misses_title_and_outside_points() {
        let l = layout(right_edge_anchor(), CLIENT_WIDTH, 96, 1);
        let (x, y) = (
            (l.title.left + l.title.right) / 2,
            (l.title.top + l.title.bottom) / 2,
        );
        assert_eq!(hit_test(&l, x, y), CardHit::None);
        assert_eq!(hit_test(&l, l.card.left - 1, l.card.top), CardHit::None);
        assert_eq!(hit_test(&l, l.card.right, l.card.bottom), CardHit::None);
    }

    #[test]
    fn toggle_marker_clears_the_active_choice_and_replaces_others() {
        assert_eq!(toggle_marker(None, Marker::Flame), Some(Marker::Flame));
        assert_eq!(toggle_marker(Some(Marker::Flame), Marker::Flame), None);
        assert_eq!(
            toggle_marker(Some(Marker::Flame), Marker::Eyes),
            Some(Marker::Eyes)
        );
    }

    #[test]
    fn next_marker_steps_through_all_markers_then_clears() {
        let mut current = None;
        let mut seen = Vec::new();
        for _ in 0..=MARKER_COUNT {
            current = next_marker(current);
            seen.push(current);
        }
        let expected: Vec<Option<Marker>> = MARKERS
            .iter()
            .map(|marker| Some(*marker))
            .chain(std::iter::once(None))
            .collect();
        assert_eq!(seen, expected);
    }

    #[test]
    fn more_line_exists_only_when_rows_are_hidden_and_stays_inside_card() {
        for dpi in DPIS {
            let plain = layout(right_edge_anchor(), CLIENT_WIDTH, dpi, MAX_ROWS);
            assert_eq!(plain.more, None);

            let hidden = layout(right_edge_anchor(), CLIENT_WIDTH, dpi, MAX_ROWS + 4);
            let more = hidden.more.expect("hidden rows need a more line");
            assert_eq!(more.height(), Metrics::new(dpi).px(MORE_HEIGHT));
            assert!(hidden.card.contains_rect(more));
            assert!(!intersects(more, hidden.rows[MAX_ROWS - 1]));
            assert_eq!(hidden.card.bottom, more.bottom + Metrics::new(dpi).px(PAD));
        }
    }

    #[test]
    fn hidden_rows_count_rows_over_the_cap() {
        let dpi = 96;
        let l = layout(right_edge_anchor(), CLIENT_WIDTH, dpi, 11);
        assert_eq!(l.rows.len(), MAX_ROWS);
        assert_eq!(l.hidden_rows, 3);

        let exact = layout(right_edge_anchor(), CLIENT_WIDTH, dpi, MAX_ROWS);
        assert_eq!(exact.rows.len(), MAX_ROWS);
        assert_eq!(exact.hidden_rows, 0);

        let few = layout(right_edge_anchor(), CLIENT_WIDTH, dpi, 2);
        assert_eq!(few.rows.len(), 2);
        assert_eq!(few.hidden_rows, 0);
    }
}
