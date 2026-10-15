//! When a dragged tab over the page means "split" (docs/design/tab-drag.md, docs/design/split-view.md).
//!
//! `split_view::drop_zone` gives every point of the page a zone, so a tab pulled off the strip
//! showed a split preview from its very first pixel over the page: it enters through the page's top
//! edge (or the left edge, from the vertical sidebar), and that edge's half (or, over a split, a
//! quarter, the corners being a quarter of each axis) was offered at once. A tab dragged down "to
//! look", or on its way to the far side, flashed "Open at the top" all the way.
//!
//! The gate here keeps the edge rule but asks for intent:
//!
//! - **Edge bands.** A zone shows only within [`BAND_PERMILLE`] of its edge. The middle of the page
//!   is neutral: no preview, and a release there cancels (the tab glides home).
//! - **The entry edge is disarmed** until the pointer has been past that edge's band once. Coming
//!   down from the strip, "top" is only offered after going deeper into the page and coming back
//!   up; from the sidebar, "left" likewise.
//! - **Corners** (only where `split_view::corners_allowed`) are the [`CORNER_PERMILLE`] squares,
//!   smaller than the bands, so a drop aimed at an edge near its end stays an edge.
//! - **Hysteresis**: a shown zone holds [`HOLD_PERMILLE`] further, so the preview does not flicker
//!   on a band's boundary or where two bands meet.

use crate::split_view::{DropZone, Quadrant};

/// Depth of each edge's band, in per-mille of that axis.
pub const BAND_PERMILLE: i32 = 250;
/// A corner zone: within this of both axes' ends (per-mille).
pub const CORNER_PERMILLE: i32 = 150;
/// How much further a zone that is already shown holds (per-mille).
pub const HOLD_PERMILLE: i32 = 40;

/// What one drag has done so far over the page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DropIntent {
    /// The edge the pointer came in through (its first point's nearest edge).
    pub entry: Option<DropZone>,
    /// The pointer has been past the entry edge's band: that edge is offered too.
    pub armed: bool,
}

fn per_mille(value: i32, start: i32, end: i32) -> i32 {
    let span = i64::from((end - start).max(1));
    (i64::from(value - start) * 1000 / span) as i32
}

/// Distance (per-mille) from (u, v) to `edge`.
fn edge_distance(edge: DropZone, u: i32, v: i32) -> i32 {
    match edge {
        DropZone::Left => u,
        DropZone::Right => 1000 - u,
        DropZone::Top => v,
        DropZone::Bottom => 1000 - v,
        _ => i32::MAX,
    }
}

/// The two edges a corner touches.
fn corner_edges(corner: Quadrant) -> [DropZone; 2] {
    [
        if corner.row() == 0 { DropZone::Top } else { DropZone::Bottom },
        if corner.col() == 0 { DropZone::Left } else { DropZone::Right },
    ]
}

/// The zone the dragged tab offers at (`x`, `y`) over `page` (left, top, right, bottom), given the
/// zone shown last (`previous`), whether corner zones are allowed, and the drag's `intent` (updated).
/// `None` outside the page (which also forgets the intent: a re-entry starts over) and in the
/// neutral middle.
pub fn intent_zone(page: (i32, i32, i32, i32), x: i32, y: i32, previous: Option<DropZone>, corners: bool, intent: &mut DropIntent) -> Option<DropZone> {
    let (left, top, right, bottom) = page;
    if x < left || x >= right || y < top || y >= bottom {
        *intent = DropIntent::default();
        return None;
    }
    let (u, v) = (per_mille(x, left, right), per_mille(y, top, bottom));
    let entry = *intent.entry.get_or_insert_with(|| {
        DropZone::ALL.into_iter().min_by_key(|edge| edge_distance(*edge, u, v)).unwrap_or(DropZone::Top)
    });
    if !intent.armed && edge_distance(entry, u, v) >= BAND_PERMILLE {
        intent.armed = true;
    }
    let offered = |edge: DropZone| intent.armed || edge != entry;
    if corners {
        let corner = Quadrant::at(usize::from(v >= 500), usize::from(u >= 500));
        let limit = CORNER_PERMILLE + if previous == Some(corner.zone()) { HOLD_PERMILLE } else { 0 };
        let [row_edge, col_edge] = corner_edges(corner);
        if edge_distance(row_edge, u, v) < limit && edge_distance(col_edge, u, v) < limit && offered(row_edge) && offered(col_edge) {
            return Some(corner.zone());
        }
    }
    // A corner shown last counts as either of its edges for the hold.
    let held = |edge: DropZone| match previous {
        Some(zone) if zone == edge => true,
        Some(zone) => zone.quadrant().is_some_and(|q| corner_edges(q).contains(&edge)),
        None => false,
    };
    let reach = |edge: DropZone| BAND_PERMILLE + if held(edge) { HOLD_PERMILLE } else { 0 };
    let mut best: Option<(DropZone, i32)> = None;
    for edge in DropZone::ALL {
        let d = edge_distance(edge, u, v);
        if d >= reach(edge) || !offered(edge) {
            continue;
        }
        if best.is_none_or(|(_, best_d)| d < best_d) {
            best = Some((edge, d));
        }
    }
    let (nearest, nearest_d) = best?;
    // Where two bands meet, the edge shown last stays until the other is clearly nearer.
    match previous {
        Some(prev) if prev != nearest && DropZone::ALL.contains(&prev) && offered(prev) && edge_distance(prev, u, v) < reach(prev) && edge_distance(prev, u, v) - nearest_d < HOLD_PERMILLE => Some(prev),
        _ => Some(nearest),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::split_view::{drop_zone, drop_zone_cornered};

    const PAGE: (i32, i32, i32, i32) = (0, 100, 1000, 900); // 1000 x 800 below a 100px toolbar

    /// Replays a pointer path through the gate, as the strip's `over` messages do.
    fn replay(points: &[(i32, i32)], corners: bool) -> Vec<Option<DropZone>> {
        let mut intent = DropIntent::default();
        let mut previous = None;
        points
            .iter()
            .map(|&(x, y)| {
                previous = intent_zone(PAGE, x, y, previous, corners, &mut intent);
                previous
            })
            .collect()
    }

    /// A tab pulled straight down off the strip into the page, 10px a frame.
    fn pull_down(x: i32, to: i32) -> Vec<(i32, i32)> {
        (0..).map(|i| (x, 100 + i * 10)).take_while(|&(_, y)| y < to).collect()
    }

    #[test]
    fn the_old_rule_offers_a_split_from_the_first_pixel_over_the_page() {
        // The reported bug: entering through the top edge shows "Open at the top" immediately, and
        // over a split the top corners a quarter of the way in.
        assert_eq!(drop_zone(PAGE, 500, 100, None), Some(DropZone::Top));
        assert_eq!(drop_zone(PAGE, 500, 101, None), Some(DropZone::Top));
        assert_eq!(drop_zone_cornered(PAGE, 200, 110, None, true), Some(DropZone::TopLeft));
        // Every point of the page has a zone: there is nowhere to drag without a preview.
        for y in (100..900).step_by(40) {
            for x in (0..1000).step_by(40) {
                assert!(drop_zone(PAGE, x, y, None).is_some());
            }
        }
    }

    #[test]
    fn pulling_a_tab_down_into_the_page_shows_no_preview() {
        for x in [100, 300, 500, 700, 900] {
            let path = pull_down(x, 500);
            let zones = replay(&path, true);
            // Down the middle columns nothing is offered until the side bands; at the far left and
            // right the side band (not the top) is what shows.
            for (point, zone) in path.iter().zip(&zones) {
                assert_ne!(*zone, Some(DropZone::Top), "{point:?}");
                assert!(!matches!(zone, Some(DropZone::TopLeft | DropZone::TopRight)), "{point:?}: {zone:?}");
            }
            if (300..=700).contains(&x) {
                assert!(zones.iter().all(Option::is_none), "x={x}: {zones:?}");
            }
        }
    }

    #[test]
    fn the_entry_edge_is_offered_after_going_deeper_and_coming_back() {
        let mut path = pull_down(500, 600);
        path.extend((0..).map(|i| (500, 600 - i * 10)).take_while(|&(_, y)| y > 110));
        let zones = replay(&path, false);
        assert_eq!(zones.last().copied().flatten(), Some(DropZone::Top), "{zones:?}");
        // And from the sidebar (entering through the left edge), left is offered only after going in.
        let side: Vec<(i32, i32)> = (0..60).map(|i| (i * 10, 500)).chain((0..50).map(|i| (590 - i * 12, 500))).collect();
        let zones = replay(&side, false);
        assert!(zones[..20].iter().all(|zone| *zone != Some(DropZone::Left)), "{zones:?}");
        assert_eq!(zones.last().copied().flatten(), Some(DropZone::Left));
    }

    #[test]
    fn side_and_bottom_bands_show_at_once_and_the_middle_is_neutral() {
        let mut intent = DropIntent { entry: Some(DropZone::Top), armed: false };
        assert_eq!(intent_zone(PAGE, 100, 500, None, false, &mut intent), Some(DropZone::Left));
        assert_eq!(intent_zone(PAGE, 900, 500, None, false, &mut intent), Some(DropZone::Right));
        assert_eq!(intent_zone(PAGE, 500, 850, None, false, &mut intent), Some(DropZone::Bottom));
        assert_eq!(intent_zone(PAGE, 500, 500, Some(DropZone::Left), false, &mut intent), None);
        // Outside the page: nothing, and the intent starts over.
        assert_eq!(intent_zone(PAGE, 500, 50, None, false, &mut intent), None);
        assert_eq!(intent, DropIntent::default());
    }

    #[test]
    fn corners_are_small_and_only_where_allowed() {
        let mut armed = DropIntent { entry: Some(DropZone::Top), armed: true };
        // Inside both 150 bands: a corner.
        assert_eq!(intent_zone(PAGE, 100, 200, None, true, &mut armed), Some(DropZone::TopLeft));
        // In the left band but 200 per-mille from the top: the left edge, not a quarter.
        assert_eq!(intent_zone(PAGE, 100, 260, None, true, &mut armed), Some(DropZone::Left));
        // Corners off: the edge.
        assert_eq!(intent_zone(PAGE, 100, 200, None, false, &mut armed), Some(DropZone::Left));
        // A corner on the disarmed entry edge is not offered either.
        let mut fresh = DropIntent { entry: Some(DropZone::Top), armed: false };
        assert_eq!(intent_zone(PAGE, 100, 200, None, true, &mut fresh), Some(DropZone::Left));
    }

    #[test]
    fn boundaries_hold_the_shown_zone() {
        let mut intent = DropIntent { entry: Some(DropZone::Top), armed: true };
        // Just past the left band's edge (250 + 20): held while it was shown, not when fresh.
        assert_eq!(intent_zone(PAGE, 270, 500, Some(DropZone::Left), false, &mut intent), Some(DropZone::Left));
        assert_eq!(intent_zone(PAGE, 270, 500, None, false, &mut intent), None);
        // Where the left and bottom bands meet, left stays until bottom is clearly nearer.
        assert_eq!(intent_zone(PAGE, 200, 700, Some(DropZone::Left), false, &mut intent), Some(DropZone::Left));
        assert_eq!(intent_zone(PAGE, 200, 860, Some(DropZone::Left), false, &mut intent), Some(DropZone::Bottom));
        // A corner holds 40 further too.
        assert_eq!(intent_zone(PAGE, 170, 250, Some(DropZone::TopLeft), true, &mut intent), Some(DropZone::TopLeft));
        // Jitter on a boundary never alternates.
        let mut previous = Some(DropZone::Left);
        for i in 0..40 {
            let x = 248 + (i % 2) * 6;
            previous = intent_zone(PAGE, x, 500, previous, false, &mut intent);
            assert_eq!(previous, Some(DropZone::Left));
        }
    }
    /// Zone maps of the old and the gated rule (for the design screenshots) when
    /// `BOWSER_ZONE_PNG_DIR` is set; otherwise does nothing.
    #[test]
    fn dump_zone_maps_when_asked() {
        let Some(dir) = std::env::var_os("BOWSER_ZONE_PNG_DIR") else { return };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let (w, h) = (500, 400);
        let page = (0, 0, w, h);
        let colour = |zone: Option<DropZone>| -> [u8; 3] {
            match zone {
                None => [0x26, 0x2A, 0x33],
                Some(DropZone::Left) => [0x3E, 0x63, 0xDD],
                Some(DropZone::Right) => [0x30, 0xA4, 0x6C],
                Some(DropZone::Top) => [0xE5, 0x48, 0x4D],
                Some(DropZone::Bottom) => [0xF5, 0xA5, 0x24],
                Some(_) => [0x8E, 0x4E, 0xC6],
            }
        };
        let maps: [(&str, Box<dyn Fn(i32, i32) -> Option<DropZone>>); 5] = [
            ("zones-before-split-shown", Box::new(move |x, y| drop_zone_cornered(page, x, y, None, true))),
            ("zones-after-entering-from-strip", Box::new(move |x, y| intent_zone(page, x, y, None, true, &mut DropIntent { entry: Some(DropZone::Top), armed: false }))),
            ("zones-after-entering-from-sidebar", Box::new(move |x, y| intent_zone(page, x, y, None, true, &mut DropIntent { entry: Some(DropZone::Left), armed: false }))),
            ("zones-after-armed", Box::new(move |x, y| intent_zone(page, x, y, None, true, &mut DropIntent { entry: Some(DropZone::Top), armed: true }))),
            ("zones-before-no-split", Box::new(move |x, y| drop_zone_cornered(page, x, y, None, false))),
        ];
        for (name, zone_at) in maps {
            let mut rgb = Vec::with_capacity((w * h * 3) as usize);
            for y in 0..h {
                for x in 0..w {
                    rgb.extend_from_slice(&colour(zone_at(x, y)));
                }
            }
            write_png(&dir.join(format!("{name}.png")), w as u32, h as u32, &rgb);
        }
    }

    /// A minimal RGB PNG (stored deflate blocks).
    fn write_png(path: &std::path::Path, width: u32, height: u32, rgb: &[u8]) {
        let mut raw = Vec::new();
        for row in rgb.chunks((width * 3) as usize) {
            raw.push(0u8);
            raw.extend_from_slice(row);
        }
        let mut z = vec![0x78, 0x01];
        let chunks: Vec<&[u8]> = raw.chunks(65_535).collect();
        for (i, chunk) in chunks.iter().enumerate() {
            z.push(u8::from(i + 1 == chunks.len()));
            let len = chunk.len() as u16;
            z.extend_from_slice(&len.to_le_bytes());
            z.extend_from_slice(&(!len).to_le_bytes());
            z.extend_from_slice(chunk);
        }
        let (mut a, mut b) = (1u32, 0u32);
        for &byte in &raw {
            a = (a + u32::from(byte)) % 65_521;
            b = (b + a) % 65_521;
        }
        z.extend_from_slice(&((b << 16) | a).to_be_bytes());
        let crc = |data: &[u8]| {
            let mut c = 0xFFFF_FFFFu32;
            for &byte in data {
                c ^= u32::from(byte);
                for _ in 0..8 {
                    c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                }
            }
            !c
        };
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let mut chunk = |kind: &[u8; 4], data: &[u8]| {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let mut body = kind.to_vec();
            body.extend_from_slice(data);
            out.extend_from_slice(&body);
            out.extend_from_slice(&crc(&body).to_be_bytes());
        };
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[8, 2, 0, 0, 0]);
        chunk(b"IHDR", &header);
        chunk(b"IDAT", &z);
        chunk(b"IEND", &[]);
        std::fs::write(path, out).expect("write png");
    }
}
