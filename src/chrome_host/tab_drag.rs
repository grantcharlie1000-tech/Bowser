//! The tab drag ghost: wire types, messages and the pure geometry behind the native layered window
//! in `win32::tab_ghost` (docs/design/tab-drag.md, docs/html-chrome-protocol.md "Tab drag ghost").
//!
//! The HTML strip is a WebView2 exactly one row tall, so it cannot draw a held tab anywhere else.
//! While a tab is held, Rust shows a click-through, topmost, per-pixel-alpha window with a picture of
//! the tab that follows the system cursor in screen coordinates. The page only says when the drag
//! starts (size and grab point), whether the tab is attached to the strip or floating, and how it ends.

/// Largest ghost side accepted from the page, in CSS pixels.
pub const MAX_GHOST_SIDE: u32 = 4096;
/// Largest settle coordinate accepted from the page, in CSS pixels of the tab view.
pub const MAX_GHOST_COORD: u32 = 16_384;
/// The release glide to the slot (ms); `GHOST_SETTLE_MS` in ui/src/tab-drag-logic.ts.
pub const GLIDE_MS: u32 = 200;
/// The fade of a ghost dropped on the page (ms).
pub const FADE_MS: u32 = 120;
/// How long the ghost stays over the page's own copy after `land` (ms), so they never both vanish.
pub const LAND_HOLD_MS: u32 = 34;
/// A ghost that hears nothing from its page for this long is taken down (the page keeps it alive
/// every 500 ms).
pub const SILENCE_MS: u32 = 2000;
/// A held ghost whose mouse button has been up this long is taken down (a lost `end`).
pub const BUTTON_UP_MS: u32 = 300;
/// The lift: the held tab is drawn this much larger, about the point under the cursor.
pub const LIFT_SCALE: f32 = 1.03;
/// Constant opacity of a floating ghost (over the page), out of 255; attached ghosts are opaque.
pub const FLOAT_ALPHA: u8 = 235;

/// What the ghost looks like: a strip tab, a sidebar row, or a pinned sidebar tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GhostKind {
    Tab,
    Row,
    Tile,
}

/// How a drag ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GhostEnd {
    /// Glide to (`x`, `y`), the slot's top-left in CSS pixels of the tab view, then land.
    Settle { x: u32, y: u32 },
    /// Released over the page: fade where it is when a split drop zone is showing (the split
    /// opens), otherwise glide home to (`x`, `y`) like `Settle` (nothing to drop on there).
    Drop { x: u32, y: u32 },
    /// Land at once.
    None,
}

/// One validated `tabdrag` command. `id` is a wire tab id (`TabId + 1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabDragCommand {
    /// The tab was lifted: a `w` x `h` CSS px box grabbed at (`gx`, `gy`) inside it.
    Start { id: u32, kind: GhostKind, w: u32, h: u32, gx: u32, gy: u32, close: bool },
    /// Keepalive, and whether the tab is floating off the strip.
    Move { id: u32, floating: bool },
    /// The tab was released (or the drag cancelled).
    End { id: u32, end: GhostEnd },
}

impl TabDragCommand {
    pub fn id(self) -> u32 {
        match self {
            Self::Start { id, .. } | Self::Move { id, .. } | Self::End { id, .. } => id,
        }
    }
}

/// Rust's answers to the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GhostSignal {
    /// The ghost is on screen: the page hides its own copy (the gap stays).
    Shown,
    /// The ghost has arrived: the page shows its copy now; the ghost goes a moment later.
    Land,
    /// The ghost was taken down without landing (watchdog, failure, unknown tab).
    Gone,
}

/// `{"v":1,"t":"tabghost","a":"shown"|"land"|"gone","id":N}`.
pub fn ghost_json(signal: GhostSignal, id: u32) -> String {
    let a = match signal {
        GhostSignal::Shown => "shown",
        GhostSignal::Land => "land",
        GhostSignal::Gone => "gone",
    };
    format!("{{\"v\":1,\"t\":\"tabghost\",\"a\":\"{a}\",\"id\":{id}}}")
}

/// Validates the fields of a `start` (the parser has checked types and ranges of each field).
pub fn valid_start(w: u32, h: u32, gx: u32, gy: u32) -> bool {
    (1..=MAX_GHOST_SIDE).contains(&w) && (1..=MAX_GHOST_SIDE).contains(&h) && gx <= w && gy <= h
}

// ----- geometry (physical pixels) -----

/// The ghost's drop shadow: an ambient edge and a key layer dropped below, in CSS px (scaled by the
/// caller). Stronger than `--bw-sh-flyout` because it is seen over arbitrary page content.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GhostShadow {
    pub ambient_sigma: f32,
    pub ambient_alpha: f32,
    pub key_drop: f32,
    pub key_sigma: f32,
    pub key_alpha: f32,
}

/// The lifted shadow (CSS px): 0 6px 16px black 26%, plus a 1px ambient edge at 16%.
pub const LIFTED_SHADOW: GhostShadow =
    GhostShadow { ambient_sigma: 1.0, ambient_alpha: 0.16, key_drop: 6.0, key_sigma: 8.0, key_alpha: 0.26 };
/// The resting shadow a landed tab has (`--bw-sh-tip`: 0 2px 4px black 14%).
pub const REST_SHADOW: GhostShadow =
    GhostShadow { ambient_sigma: 0.5, ambient_alpha: 0.0, key_drop: 2.0, key_sigma: 2.0, key_alpha: 0.14 };

impl GhostShadow {
    /// This shadow at `scale` device pixels per CSS pixel.
    pub fn scaled(self, scale: f32) -> Self {
        Self {
            ambient_sigma: self.ambient_sigma * scale,
            ambient_alpha: self.ambient_alpha,
            key_drop: self.key_drop * scale,
            key_sigma: self.key_sigma * scale,
            key_alpha: self.key_alpha,
        }
    }

    /// Linear blend from `self` (t = 0) to `other` (t = 1).
    pub fn mix(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let l = |a: f32, b: f32| a + (b - a) * t;
        Self {
            ambient_sigma: l(self.ambient_sigma, other.ambient_sigma),
            ambient_alpha: l(self.ambient_alpha, other.ambient_alpha),
            key_drop: l(self.key_drop, other.key_drop),
            key_sigma: l(self.key_sigma, other.key_sigma),
            key_alpha: l(self.key_alpha, other.key_alpha),
        }
    }

    /// Room the shadow needs around the body (left/right/top, and below), in whole pixels: three
    /// standard deviations of the wider layer, plus the key layer's drop below.
    pub fn margins(self) -> (i32, i32) {
        let reach = (3.0 * self.ambient_sigma.max(self.key_sigma)).ceil().max(0.0);
        (reach as i32, (reach + self.key_drop.max(0.0)).ceil() as i32)
    }
}

/// Device pixels per CSS pixel for a window at `dpi`.
pub fn dpi_scale(dpi: u32) -> f32 {
    dpi.max(1) as f32 / 96.0
}

/// The body (the tab itself) in device pixels for a `w` x `h` CSS box at `scale` (DPI scale times
/// the lift), never smaller than 1 x 1.
pub fn body_size(w: u32, h: u32, scale: f32) -> (i32, i32) {
    let side = |v: u32| ((v as f32 * scale).round() as i32).max(1);
    (side(w), side(h))
}

/// The body's top-left (screen px) for the cursor at `cursor` holding the tab at (`gx`, `gy`) CSS
/// px, drawn at `scale`: the grabbed point stays exactly under the cursor whatever the lift.
pub fn body_origin(cursor: (i32, i32), grab: (u32, u32), scale: f32) -> (i32, i32) {
    (
        cursor.0.saturating_sub((grab.0 as f32 * scale).round() as i32),
        cursor.1.saturating_sub((grab.1 as f32 * scale).round() as i32),
    )
}

/// The slot's top-left in screen px, for (`x`, `y`) CSS px of a view whose top-left is `view` on
/// screen, at `dpi_scale`.
pub fn slot_origin(view: (i32, i32), dpi_scale: f32, x: u32, y: u32) -> (i32, i32) {
    let px = |v: u32| (v as f32 * dpi_scale).round() as i32;
    (view.0.saturating_add(px(x)), view.1.saturating_add(px(y)))
}

/// The layered window's rectangle (x, y, width, height) for a body at `origin` of `body` size with
/// `margins` (`GhostShadow::margins`).
pub fn window_rect(origin: (i32, i32), body: (i32, i32), margins: (i32, i32)) -> (i32, i32, i32, i32) {
    let (side, below) = margins;
    (origin.0 - side, origin.1 - side, body.0 + 2 * side, body.1 + side + below)
}

/// CSS `cubic-bezier(0, 0, 0, 1)` (`--bw-ease-out`) at time `t` in [0, 1]. With both x control
/// points at 0, x(s) = s^3 and y(s) = 3s^2 - 2s^3, so s = cbrt(t).
pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let s = t.cbrt();
    (3.0 * s * s - 2.0 * s * s * s).clamp(0.0, 1.0)
}

/// Point between `from` and `to` at progress `t` (already eased).
pub fn lerp_point(from: (i32, i32), to: (i32, i32), t: f32) -> (i32, i32) {
    let l = |a: i32, b: i32| (a as f32 + (b - a) as f32 * t.clamp(0.0, 1.0)).round() as i32;
    (l(from.0, to.0), l(from.1, to.1))
}

/// What the watchdog does with a held (not yet released) ghost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Watch {
    Keep,
    /// Take the ghost down and tell the page it is gone.
    Abort,
}

/// The held ghost's watchdog: silence from the page (`silent_ms` since its last message), the
/// mouse button up for `button_up_ms` (a lost release), or the window no longer active or visible.
pub fn watchdog(silent_ms: u32, button_up_ms: u32, window_ok: bool) -> Watch {
    if !window_ok || silent_ms >= SILENCE_MS || button_up_ms >= BUTTON_UP_MS { Watch::Abort } else { Watch::Keep }
}

/// Where the parts of the ghost go, in device pixels inside the body (origin at its top-left).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GhostLayout {
    pub radius: f32,
    /// Favicon box: left, top, side.
    pub icon: (i32, i32, i32),
    /// Title span: left and right (`None` when there is no title: pinned tabs and tiles).
    pub title: Option<(i32, i32)>,
    /// Centre of the close glyph, and its half span (`None` when the close button is not shown).
    pub close: Option<(f32, f32, f32)>,
}

/// The CSS geometry of each kind (tabs.css `.strip .tab`, sidebar.css `.side-tab`), at `scale`:
///
/// - tab: radius 9, 1px border + 7px to the 16px favicon, 4px gap, title, a 20px close box 2px from
///   the right edge with a 16px glyph whose X spans 8px; pinned tabs centre the favicon alone;
/// - row: radius 9, 8px to the favicon, 8px gap, title, a 20px close box 4px from the right with a
///   12px glyph (X spans 6px);
/// - tile: radius 6, the favicon centred.
pub fn layout(kind: GhostKind, body: (i32, i32), scale: f32, pinned: bool, close: bool) -> GhostLayout {
    let px = |v: f32| (v * scale).round() as i32;
    let icon_side = px(16.0).max(1);
    let icon_top = (body.1 - icon_side) / 2;
    let centred = GhostLayout { radius: 0.0, icon: ((body.0 - icon_side) / 2, icon_top, icon_side), title: None, close: None };
    match kind {
        GhostKind::Tile => GhostLayout { radius: 6.0 * scale, ..centred },
        GhostKind::Tab if pinned => GhostLayout { radius: 9.0 * scale, ..centred },
        GhostKind::Tab | GhostKind::Row => {
            let (inset, gap, pad, half) = if kind == GhostKind::Tab { (8.0, 4.0, 2.0, 4.0) } else { (8.0, 8.0, 4.0, 3.0) };
            let icon_left = px(inset);
            let close_box = 20.0 * scale;
            let close_centre_x = body.0 as f32 - pad * scale - close_box / 2.0;
            let title_left = icon_left + icon_side + px(gap);
            let title_right = if close { (body.0 as f32 - pad * scale - close_box - 4.0 * scale).round() as i32 } else { body.0 - px(6.0) };
            GhostLayout {
                radius: 9.0 * scale,
                icon: (icon_left, icon_top, icon_side),
                title: (title_right - title_left >= px(8.0)).then_some((title_left, title_right)),
                close: close.then_some((close_centre_x, body.1 as f32 / 2.0, half * scale)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_host::protocol::{parse_command, Command};

    #[test]
    fn grab_point_stays_under_the_cursor_at_every_scale_and_dpi() {
        for dpi in [96, 120, 144, 192, 288] {
            for lift in [1.0, LIFT_SCALE] {
                let scale = dpi_scale(dpi) * lift;
                let cursor = (1000, 500);
                let origin = body_origin(cursor, (37, 13), scale);
                // The grabbed CSS point, scaled, lands on the cursor (within rounding).
                let back = (origin.0 + (37.0 * scale).round() as i32, origin.1 + (13.0 * scale).round() as i32);
                assert_eq!(back, cursor, "dpi {dpi} lift {lift}");
            }
        }
        // Negative screen coordinates (a monitor left of the primary) work the same.
        assert_eq!(body_origin((-500, -20), (10, 5), 2.0), (-520, -30));
    }

    #[test]
    fn sizes_scale_with_dpi_and_never_vanish() {
        assert_eq!(body_size(180, 26, 1.0), (180, 26));
        assert_eq!(body_size(180, 26, 1.5), (270, 39));
        assert_eq!(body_size(180, 26, 2.0 * LIFT_SCALE), (371, 54));
        assert_eq!(body_size(1, 1, 0.1), (1, 1));
        assert_eq!(dpi_scale(144), 1.5);
        assert_eq!(dpi_scale(0), 1.0 / 96.0);
    }

    #[test]
    fn window_rect_leaves_room_for_the_shadow() {
        let m = LIFTED_SHADOW.scaled(1.0).margins();
        assert_eq!(m, (24, 30));
        assert_eq!(window_rect((100, 50), (180, 26), m), (76, 26, 228, 80));
        let m2 = LIFTED_SHADOW.scaled(2.0).margins();
        assert_eq!(m2, (48, 60));
        assert_eq!(REST_SHADOW.margins(), (6, 8));
    }

    #[test]
    fn slot_origin_converts_view_css_to_screen() {
        assert_eq!(slot_origin((200, 30), 1.5, 100, 4), (350, 36));
        assert_eq!(slot_origin((-1920, 0), 1.0, 10, 0), (-1910, 0));
    }

    #[test]
    fn ease_out_matches_the_css_curve() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert_eq!(ease_out(-1.0), 0.0);
        assert_eq!(ease_out(2.0), 1.0);
        // cubic-bezier(0,0,0,1) is steep: half way through time it is ~0.89 of the way.
        assert!((ease_out(0.5) - 0.8899).abs() < 0.001, "{}", ease_out(0.5));
        let mut last = 0.0;
        for i in 0..=100 {
            let v = ease_out(i as f32 / 100.0);
            assert!(v >= last);
            last = v;
        }
        assert_eq!(lerp_point((0, 0), (100, -50), 0.5), (50, -25));
        assert_eq!(lerp_point((10, 10), (20, 20), 1.0), (20, 20));
    }

    #[test]
    fn watchdog_takes_the_ghost_down_on_silence_release_or_deactivation() {
        assert_eq!(watchdog(0, 0, true), Watch::Keep);
        assert_eq!(watchdog(SILENCE_MS - 1, BUTTON_UP_MS - 1, true), Watch::Keep);
        assert_eq!(watchdog(SILENCE_MS, 0, true), Watch::Abort);
        assert_eq!(watchdog(0, BUTTON_UP_MS, true), Watch::Abort);
        assert_eq!(watchdog(0, 0, false), Watch::Abort);
    }

    #[test]
    fn shadow_blends_from_lifted_to_rest() {
        assert_eq!(LIFTED_SHADOW.mix(REST_SHADOW, 0.0), LIFTED_SHADOW);
        assert_eq!(LIFTED_SHADOW.mix(REST_SHADOW, 1.0), REST_SHADOW);
        assert_eq!(LIFTED_SHADOW.mix(REST_SHADOW, 7.0), REST_SHADOW);
    }

    #[test]
    fn layout_follows_the_css_geometry() {
        let tab = layout(GhostKind::Tab, (200, 26), 1.0, false, true);
        assert_eq!(tab.icon, (8, 5, 16));
        assert_eq!(tab.title, Some((28, 174)));
        assert_eq!(tab.close, Some((188.0, 13.0, 4.0)));
        assert_eq!(tab.radius, 9.0);
        let no_close = layout(GhostKind::Tab, (200, 26), 1.0, false, false);
        assert_eq!(no_close.title, Some((28, 194)));
        assert_eq!(no_close.close, None);
        let pinned = layout(GhostKind::Tab, (32, 26), 1.0, true, true);
        assert_eq!(pinned.icon, (8, 5, 16));
        assert_eq!(pinned.title, None);
        assert_eq!(pinned.close, None);
        let tile = layout(GhostKind::Tile, (32, 32), 2.0, true, false);
        assert_eq!(tile.icon, (0, 0, 32));
        assert_eq!(tile.radius, 12.0);
        let row = layout(GhostKind::Row, (360, 39), 1.5, false, true);
        assert_eq!(row.icon, (12, 7, 24));
        assert_eq!(row.title.map(|t| t.0), Some(48));
        // A tab too narrow for a title draws its favicon alone.
        assert_eq!(layout(GhostKind::Tab, (40, 26), 1.0, false, true).title, None);
    }

    #[test]
    fn signals_are_exact() {
        assert_eq!(ghost_json(GhostSignal::Shown, 4), r#"{"v":1,"t":"tabghost","a":"shown","id":4}"#);
        assert_eq!(ghost_json(GhostSignal::Land, 4), r#"{"v":1,"t":"tabghost","a":"land","id":4}"#);
        assert_eq!(ghost_json(GhostSignal::Gone, 1), r#"{"v":1,"t":"tabghost","a":"gone","id":1}"#);
    }

    fn ok(raw: &str) -> TabDragCommand {
        match parse_command(raw) {
            Ok(Command::TabDrag(command)) => command,
            other => panic!("{raw}: {other:?}"),
        }
    }

    /// Wire commands the page sends, as test/tab-drag.mjs builds them.
    const ACCEPTED: [&str; 6] = [
        r#"{"v":1,"t":"tabdrag","a":"start","id":3,"k":"tab","r":"180,26,40,12,1"}"#,
        r#"{"v":1,"t":"tabdrag","a":"start","id":9,"k":"tile","r":"32,32,32,0,0"}"#,
        r#"{"v":1,"t":"tabdrag","a":"move","id":3,"f":true}"#,
        r#"{"v":1,"t":"tabdrag","a":"end","id":3,"m":"settle","p":"120,7"}"#,
        r#"{"v":1,"t":"tabdrag","a":"end","id":3,"m":"drop","p":"40,4"}"#,
        r#"{"v":1,"t":"tabdrag","a":"end","id":3,"m":"none"}"#,
    ];

    const REJECTED: [&str; 14] = [
        r#"{"v":1,"t":"tabdrag","a":"start","id":3,"k":"tab","r":"180,26,181,12,1"}"#,
        r#"{"v":1,"t":"tabdrag","a":"start","id":3,"k":"tab","r":"0,26,0,12,1"}"#,
        r#"{"v":1,"t":"tabdrag","a":"start","id":3,"k":"tab","r":"4097,26,0,12,1"}"#,
        r#"{"v":1,"t":"tabdrag","a":"start","id":3,"k":"tab","r":"180,26,0,12,2"}"#,
        r#"{"v":1,"t":"tabdrag","a":"start","id":3,"k":"tab","r":"180,26,0,12"}"#,
        r#"{"v":1,"t":"tabdrag","a":"start","id":3,"k":"pill","r":"180,26,0,12,1"}"#,
        r#"{"v":1,"t":"tabdrag","a":"start","id":0,"k":"tab","r":"180,26,0,12,1"}"#,
        r#"{"v":1,"t":"tabdrag","a":"move","id":3}"#,
        r#"{"v":1,"t":"tabdrag","a":"move","id":3,"f":true,"p":"1,1"}"#,
        r#"{"v":1,"t":"tabdrag","a":"end","id":3,"m":"settle","p":"120"}"#,
        r#"{"v":1,"t":"tabdrag","a":"end","id":3,"m":"settle","p":"16385,0"}"#,
        r#"{"v":1,"t":"tabdrag","a":"end","id":3,"m":"drop"}"#,
        r#"{"v":1,"t":"tabdrag","a":"end","id":3,"m":"fly"}"#,
        r#"{"v":1,"t":"tabdrag","a":"lift","id":3}"#,
    ];

    #[test]
    fn tab_drag_commands_take_exactly_their_fields() {
        assert_eq!(ok(ACCEPTED[0]), TabDragCommand::Start { id: 3, kind: GhostKind::Tab, w: 180, h: 26, gx: 40, gy: 12, close: true });
        assert_eq!(ok(ACCEPTED[1]), TabDragCommand::Start { id: 9, kind: GhostKind::Tile, w: 32, h: 32, gx: 32, gy: 0, close: false });
        assert_eq!(ok(ACCEPTED[2]), TabDragCommand::Move { id: 3, floating: true });
        assert_eq!(ok(ACCEPTED[3]), TabDragCommand::End { id: 3, end: GhostEnd::Settle { x: 120, y: 7 } });
        assert_eq!(ok(ACCEPTED[4]), TabDragCommand::End { id: 3, end: GhostEnd::Drop { x: 40, y: 4 } });
        assert_eq!(ok(ACCEPTED[5]), TabDragCommand::End { id: 3, end: GhostEnd::None });
        assert_eq!(ok(ACCEPTED[5]).id(), 3);
        for raw in REJECTED {
            assert!(parse_command(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn tab_drag_messages_match_committed_fixture() {
        let list = |items: &[&str]| items.iter().map(|raw| format!("{raw:?}")).collect::<Vec<_>>().join(",");
        let content = format!(
            "{{\"shown\":{},\"land\":{},\"gone\":{},\"accepted\":[{}],\"rejected\":[{}]}}\n",
            ghost_json(GhostSignal::Shown, 4),
            ghost_json(GhostSignal::Land, 4),
            ghost_json(GhostSignal::Gone, 4),
            list(&ACCEPTED),
            list(&REJECTED),
        );
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/test/fixtures/tab-drag-messages.json");
        if std::env::var_os("BOWSER_WRITE_FIXTURES").is_some_and(|value| value == "1") {
            std::fs::write(&path, content).expect("write tab drag fixture");
        } else {
            assert_eq!(std::fs::read_to_string(path).expect("committed tab drag fixture"), content);
        }
    }
}
