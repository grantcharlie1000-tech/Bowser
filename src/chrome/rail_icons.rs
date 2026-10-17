//! Outlines for the rail's icons, drawn on a 24-unit grid in the app's "Clean" icon style: two
//! units of padding, one weight, round caps and joins, two-unit corners. The painter scales each
//! icon into its button and strokes it with the rail's pen. Every outline is built from round-
//! ended segments and filled dots, so the painter needs no path support and every icon stays
//! antialiased at any dpi. The SVG sources live in `icons/rail/`; each icon below is a direct port
//! of its file, using the `Path` builder so the commands read like the SVG path data.

use std::f32::consts::{PI, TAU};

use crate::chrome::rail::{RailIcon, WorkspaceGlyph};

/// Side of the square every icon is designed in.
pub const GRID: f32 = 24.0;

/// Samples along each Bézier curve, around each rounded corner and around each ring.
const CURVE_STEPS: usize = 16;
const CORNER_STEPS: usize = 6;
const RING_STEPS: usize = 40;

type Point = (f32, f32);

/// One stroke or dot, in grid units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Primitive {
    /// A stroke with round ends.
    Segment { from: (f32, f32), to: (f32, f32) },
    /// A filled dot.
    Dot { centre: (f32, f32), radius: f32 },
}

/// The primitives that draw `icon`. A letter tile has none, because the painter draws its text.
pub fn primitives(icon: RailIcon) -> Vec<Primitive> {
    let mut out = Vec::new();
    match icon {
        RailIcon::Home => home(&mut out),
        RailIcon::Customize => customize(&mut out),
        RailIcon::Plus => {
            segment(&mut out, (12.0, 4.5), (12.0, 19.5));
            segment(&mut out, (4.5, 12.0), (19.5, 12.0));
        }
        RailIcon::Heart => heart_outline(&mut out),
        RailIcon::Clock => {
            ring(&mut out, (12.0, 12.0), 9.0);
            Path::new(&mut out, (12.0, 7.0)).l(12.0, 12.0).l(15.5, 14.0);
        }
        RailIcon::Dots => {
            for y in [5.5, 12.0, 18.5] {
                dot(&mut out, (12.0, y), 1.6);
            }
        }
        RailIcon::AppTile => {
            for (x, y) in [(3.5, 3.5), (13.5, 3.5), (3.5, 13.5)] {
                chain(&mut out, &rounded_box(x, y, 7.0, 7.0, 2.0), true);
            }
            segment(&mut out, (17.0, 13.5), (17.0, 20.5));
            segment(&mut out, (13.5, 17.0), (20.5, 17.0));
        }
        RailIcon::Feed => {
            // A dot and two arcs (the feed mark): the same strokes as icons/extra/rss.svg.
            dot(&mut out, (5.0, 19.0), 1.6);
            Path::new(&mut out, (4.0, 11.0)).c((8.97, 11.0), (13.0, 15.03), (13.0, 20.0));
            Path::new(&mut out, (4.0, 4.0)).c((12.84, 4.0), (20.0, 11.16), (20.0, 20.0));
        }
        RailIcon::Letter => {}
        RailIcon::Glyph(glyph) => workspace_glyph(&mut out, glyph),
    }
    out
}

/// Length a filled dot adds to an icon's draw-on, in grid units. A dot has no stroke, so it takes a
/// short nominal span and pops in as the trace reaches it.
const DOT_SPAN: f32 = 2.0;

impl Primitive {
    /// How much of the draw-on timeline this primitive takes, in grid units.
    pub fn span(&self) -> f32 {
        match *self {
            Primitive::Segment { from, to } => (to.0 - from.0).hypot(to.1 - from.1),
            Primitive::Dot { .. } => DOT_SPAN,
        }
    }
}

/// An icon's primitives and the total span of its draw-on, built once and reused every frame.
#[derive(Clone, Debug)]
pub struct CachedIcon {
    icon: RailIcon,
    pub primitives: Vec<Primitive>,
    /// Sum of every primitive's span.
    pub span: f32,
}

/// Builds each icon's primitives the first time it is painted and keeps them, so a rail that
/// animates does not rebuild and reallocate its outlines on every frame.
#[derive(Debug, Default)]
pub struct IconCache {
    icons: Vec<CachedIcon>,
}

impl IconCache {
    pub fn get(&mut self, icon: RailIcon) -> &CachedIcon {
        let index = match self.icons.iter().position(|cached| cached.icon == icon) {
            Some(index) => index,
            None => {
                let primitives = primitives(icon);
                let span = primitives.iter().map(Primitive::span).sum();
                self.icons.push(CachedIcon {
                    icon,
                    primitives,
                    span,
                });
                self.icons.len() - 1
            }
        };
        &self.icons[index]
    }

    pub fn len(&self) -> usize {
        self.icons.len()
    }

    pub fn is_empty(&self) -> bool {
        self.icons.is_empty()
    }
}

/// The part of the primitive that starts `offset` along an icon's draw-on, and spans `span`, which
/// a trace that has covered `covered` has drawn, in `0.0..=1.0`.
pub fn traced_fraction(offset: f32, span: f32, covered: f32) -> f32 {
    if span <= 0.0 {
        return if covered > offset { 1.0 } else { 0.0 };
    }
    ((covered - offset) / span).clamp(0.0, 1.0)
}

fn segment(out: &mut Vec<Primitive>, from: Point, to: Point) {
    out.push(Primitive::Segment { from, to });
}

fn dot(out: &mut Vec<Primitive>, centre: Point, radius: f32) {
    out.push(Primitive::Dot { centre, radius });
}

/// Joins `points` with segments. A closed chain also joins the last point back to the first.
/// Repeated points, which sampled corners can produce, add no stroke and are skipped.
fn chain(out: &mut Vec<Primitive>, points: &[Point], closed: bool) {
    let mut joins: Vec<(Point, Point)> = points.windows(2).map(|pair| (pair[0], pair[1])).collect();
    if closed && points.len() > 2 {
        joins.push((points[points.len() - 1], points[0]));
    }
    for (from, to) in joins {
        if from != to {
            segment(out, from, to);
        }
    }
}

fn ring(out: &mut Vec<Primitive>, centre: Point, radius: f32) {
    ellipse(out, centre, radius, radius);
}

fn ellipse(out: &mut Vec<Primitive>, centre: Point, rx: f32, ry: f32) {
    let points: Vec<_> = (0..RING_STEPS)
        .map(|i| {
            let t = i as f32 / RING_STEPS as f32 * TAU;
            (centre.0 + rx * t.cos(), centre.1 + ry * t.sin())
        })
        .collect();
    chain(out, &points, true);
}

/// The outline of a rectangle with rounded corners, sampled at each corner.
fn rounded_box(x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<Point> {
    let corners = [
        (x + w - r, y + r, -PI / 2.0),
        (x + w - r, y + h - r, 0.0),
        (x + r, y + h - r, PI / 2.0),
        (x + r, y + r, PI),
    ];
    let mut points = Vec::with_capacity(4 * (CORNER_STEPS + 1));
    for (cx, cy, start) in corners {
        for step in 0..=CORNER_STEPS {
            let t = start + step as f32 * (PI / 2.0) / CORNER_STEPS as f32;
            points.push((cx + r * t.cos(), cy + r * t.sin()));
        }
    }
    points
}

/// A quadratic Bézier curve from `a` to `b` bent towards `control`.
fn quadratic(a: Point, control: Point, b: Point) -> Vec<Point> {
    (0..=CURVE_STEPS)
        .map(|i| {
            let t = i as f32 / CURVE_STEPS as f32;
            let u = 1.0 - t;
            (
                u * u * a.0 + 2.0 * u * t * control.0 + t * t * b.0,
                u * u * a.1 + 2.0 * u * t * control.1 + t * t * b.1,
            )
        })
        .collect()
}

/// A cubic Bézier curve from `a` to `b` with controls `c1` and `c2`.
fn cubic(a: Point, c1: Point, c2: Point, b: Point) -> Vec<Point> {
    (0..=CURVE_STEPS)
        .map(|i| {
            let t = i as f32 / CURVE_STEPS as f32;
            let u = 1.0 - t;
            let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            (
                w[0] * a.0 + w[1] * c1.0 + w[2] * c2.0 + w[3] * b.0,
                w[0] * a.1 + w[1] * c1.1 + w[2] * c2.1 + w[3] * b.1,
            )
        })
        .collect()
}

/// A pen that follows SVG path commands (`M`, `L`, `H`, `V`, `Q`, `C`, `Z`) and emits the sampled
/// segments.
struct Path<'a> {
    out: &'a mut Vec<Primitive>,
    start: Point,
    at: Point,
}

impl<'a> Path<'a> {
    fn new(out: &'a mut Vec<Primitive>, start: Point) -> Self {
        Path {
            out,
            start,
            at: start,
        }
    }

    /// Lifts the pen and starts a new subpath.
    fn m(mut self, x: f32, y: f32) -> Self {
        self.start = (x, y);
        self.at = (x, y);
        self
    }

    fn l(mut self, x: f32, y: f32) -> Self {
        segment(self.out, self.at, (x, y));
        self.at = (x, y);
        self
    }

    fn h(self, x: f32) -> Self {
        let y = self.at.1;
        self.l(x, y)
    }

    fn v(self, y: f32) -> Self {
        let x = self.at.0;
        self.l(x, y)
    }

    fn q(mut self, cx: f32, cy: f32, x: f32, y: f32) -> Self {
        chain(self.out, &quadratic(self.at, (cx, cy), (x, y)), false);
        self.at = (x, y);
        self
    }

    fn c(mut self, c1: Point, c2: Point, to: Point) -> Self {
        chain(self.out, &cubic(self.at, c1, c2, to), false);
        self.at = to;
        self
    }

    /// Closes the subpath with a straight stroke back to its start.
    fn z(mut self) -> Self {
        if self.at != self.start {
            segment(self.out, self.at, self.start);
        }
        self.at = self.start;
        self
    }
}

/// The house's walls, shared by the rail's Home button and the Home workspace glyph.
fn house_walls(out: &mut Vec<Primitive>) {
    Path::new(out, (5.0, 10.0))
        .v(18.5)
        .q(5.0, 20.5, 7.0, 20.5)
        .h(17.0)
        .q(19.0, 20.5, 19.0, 18.5)
        .v(10.0);
}

fn home(out: &mut Vec<Primitive>) {
    Path::new(out, (3.0, 11.5)).l(12.0, 3.5).l(21.0, 11.5);
    house_walls(out);
    Path::new(out, (10.0, 20.5))
        .v(16.0)
        .q(10.0, 14.5, 11.5, 14.5)
        .h(12.5)
        .q(14.0, 14.5, 14.0, 16.0)
        .v(20.5);
}

fn customize(out: &mut Vec<Primitive>) {
    segment(out, (3.0, 7.0), (13.5, 7.0));
    segment(out, (18.5, 7.0), (21.0, 7.0));
    segment(out, (3.0, 17.0), (5.5, 17.0));
    segment(out, (10.5, 17.0), (21.0, 17.0));
    ring(out, (16.0, 7.0), 2.5);
    ring(out, (8.0, 17.0), 2.5);
}

/// The heart outline, drawn from its bottom tip.
fn heart_outline(out: &mut Vec<Primitive>) {
    Path::new(out, (12.0, 20.5))
        .c((6.0, 16.0), (3.0, 13.0), (3.0, 9.3))
        .c((3.0, 6.5), (5.0, 4.5), (7.5, 4.5))
        .c((9.4, 4.5), (11.0, 5.5), (12.0, 7.2))
        .c((13.0, 5.5), (14.6, 4.5), (16.5, 4.5))
        .c((19.0, 4.5), (21.0, 6.5), (21.0, 9.3))
        .c((21.0, 13.0), (18.0, 16.0), (12.0, 20.5))
        .z();
}

/// A five-point star with a short inner radius, centred a little low so it looks centred.
fn star(out: &mut Vec<Primitive>) {
    let points = [
        (12.0, 3.4),
        (14.53, 9.42),
        (21.04, 9.96),
        (16.09, 14.23),
        (17.58, 20.59),
        (12.0, 17.2),
        (6.42, 20.59),
        (7.91, 14.23),
        (2.96, 9.96),
        (9.47, 9.42),
    ];
    chain(out, &points, true);
}

fn workspace_glyph(out: &mut Vec<Primitive>, glyph: WorkspaceGlyph) {
    match glyph {
        WorkspaceGlyph::Briefcase => {
            chain(out, &rounded_box(3.0, 7.5, 18.0, 12.0, 2.0), true);
            Path::new(out, (9.0, 7.5))
                .v(6.0)
                .q(9.0, 4.5, 10.5, 4.5)
                .h(13.5)
                .q(15.0, 4.5, 15.0, 6.0)
                .v(7.5)
                .m(3.0, 13.0)
                .h(21.0);
        }
        WorkspaceGlyph::Home => {
            Path::new(out, (3.0, 11.5)).l(12.0, 4.0).l(21.0, 11.5);
            house_walls(out);
            Path::new(out, (16.0, 7.2)).v(4.5).h(18.5).v(9.2);
            ring(out, (12.0, 14.0), 2.0);
        }
        WorkspaceGlyph::Code => {
            Path::new(out, (8.5, 7.5)).l(4.0, 12.0).l(8.5, 16.5);
            Path::new(out, (15.5, 7.5)).l(20.0, 12.0).l(15.5, 16.5);
            segment(out, (13.5, 5.5), (10.5, 18.5));
        }
        WorkspaceGlyph::Book => {
            Path::new(out, (12.0, 7.0))
                .c((10.5, 5.5), (8.0, 5.0), (3.5, 5.5))
                .v(18.0)
                .c((8.0, 17.5), (10.5, 18.0), (12.0, 19.5))
                .c((13.5, 18.0), (16.0, 17.5), (20.5, 18.0))
                .v(5.5)
                .c((16.0, 5.0), (13.5, 5.5), (12.0, 7.0))
                .z()
                .m(12.0, 7.0)
                .v(19.5);
        }
        WorkspaceGlyph::Cart => {
            Path::new(out, (2.5, 4.5))
                .h(5.0)
                .l(7.7, 15.5)
                .h(18.2)
                .l(20.7, 8.0)
                .h(5.9);
            dot(out, (9.5, 19.5), 1.6);
            dot(out, (16.5, 19.5), 1.6);
        }
        WorkspaceGlyph::Game => {
            chain(out, &rounded_box(2.5, 6.5, 19.0, 11.0, 5.0), true);
            segment(out, (7.0, 10.5), (7.0, 14.5));
            segment(out, (5.0, 12.5), (9.0, 12.5));
            dot(out, (15.5, 11.5), 1.1);
            dot(out, (18.0, 14.0), 1.1);
        }
        WorkspaceGlyph::Music => {
            dot(out, (6.5, 17.5), 2.3);
            dot(out, (16.5, 15.5), 2.3);
            Path::new(out, (8.8, 17.5)).v(6.5).l(18.8, 4.5).v(15.5);
            segment(out, (8.8, 10.5), (18.8, 8.5));
        }
        WorkspaceGlyph::Heart => {
            heart_outline(out);
            Path::new(out, (6.5, 9.5)).q(6.7, 7.8, 8.3, 7.3);
        }
        WorkspaceGlyph::Star => star(out),
        WorkspaceGlyph::Bolt => {
            Path::new(out, (13.0, 2.5))
                .l(5.5, 13.5)
                .h(11.5)
                .l(10.5, 21.5)
                .l(18.5, 10.5)
                .h(12.5)
                .z();
        }
        WorkspaceGlyph::Globe => {
            ring(out, (12.0, 12.0), 9.0);
            ellipse(out, (12.0, 12.0), 4.0, 9.0);
            segment(out, (3.0, 12.0), (21.0, 12.0));
        }
        WorkspaceGlyph::Leaf => {
            Path::new(out, (5.0, 19.0))
                .c((4.0, 10.5), (9.0, 5.0), (19.0, 4.5))
                .c((19.5, 14.5), (14.5, 20.0), (5.0, 19.0))
                .z()
                .m(3.5, 20.5)
                .l(13.0, 11.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::rail::RailIcon;

    fn all_icons() -> Vec<RailIcon> {
        [
            RailIcon::Home,
            RailIcon::Customize,
            RailIcon::Plus,
            RailIcon::Heart,
            RailIcon::Clock,
            RailIcon::Dots,
            RailIcon::AppTile,
            RailIcon::Letter,
        ]
        .into_iter()
        .chain(WorkspaceGlyph::ALL.map(RailIcon::Glyph))
        .collect()
    }

    fn points(primitive: &Primitive) -> Vec<(f32, f32)> {
        match *primitive {
            Primitive::Segment { from, to } => vec![from, to],
            Primitive::Dot { centre, radius } => {
                vec![
                    (centre.0 - radius, centre.1 - radius),
                    (centre.0 + radius, centre.1 + radius),
                ]
            }
        }
    }

    #[test]
    fn every_drawn_icon_has_strokes_and_the_letter_tile_has_none() {
        for icon in all_icons() {
            let prims = primitives(icon);
            match icon {
                RailIcon::Letter => assert!(prims.is_empty(), "letters are drawn as text"),
                _ => assert!(!prims.is_empty(), "{icon:?} has no outline"),
            }
        }
    }

    #[test]
    fn every_outline_stays_inside_the_grid() {
        for icon in all_icons() {
            for prim in primitives(icon) {
                for (x, y) in points(&prim) {
                    assert!(
                        (-0.01..=GRID + 0.01).contains(&x) && (-0.01..=GRID + 0.01).contains(&y),
                        "{icon:?} reaches ({x}, {y}), outside the {GRID}-unit grid"
                    );
                }
            }
        }
    }

    #[test]
    fn every_icon_outline_is_distinct() {
        let icons = all_icons();
        for (i, a) in icons.iter().enumerate() {
            for b in &icons[i + 1..] {
                if matches!(a, RailIcon::Letter) || matches!(b, RailIcon::Letter) {
                    continue;
                }
                assert_ne!(
                    primitives(*a),
                    primitives(*b),
                    "{a:?} and {b:?} draw the same shape"
                );
            }
        }
    }

    #[test]
    fn no_outline_is_degenerate() {
        for icon in all_icons() {
            for prim in primitives(icon) {
                match prim {
                    Primitive::Segment { from, to } => {
                        assert!(from != to, "{icon:?} has a zero-length stroke")
                    }
                    Primitive::Dot { radius, .. } => {
                        assert!(radius > 0.0, "{icon:?} has an empty dot")
                    }
                }
            }
        }
    }

    #[test]
    fn the_cache_builds_each_icon_once_and_matches_a_fresh_build() {
        let mut cache = IconCache::default();
        assert!(cache.is_empty());
        for icon in all_icons() {
            let first = cache.get(icon).primitives.clone();
            assert_eq!(first, primitives(icon), "{icon:?} caches its own outline");
        }
        let built = cache.len();
        assert_eq!(built, all_icons().len());
        for icon in all_icons() {
            cache.get(icon);
        }
        assert_eq!(cache.len(), built, "asking again builds nothing");
    }

    #[test]
    fn every_drawn_icon_has_a_draw_on_span_and_the_letter_has_none() {
        let mut cache = IconCache::default();
        for icon in all_icons() {
            let span = cache.get(icon).span;
            match icon {
                RailIcon::Letter => assert_eq!(span, 0.0),
                _ => assert!(span > 0.0, "{icon:?} has nothing to draw on"),
            }
        }
    }

    #[test]
    fn a_trace_covers_primitives_in_order() {
        assert_eq!(traced_fraction(0.0, 10.0, 0.0), 0.0);
        assert_eq!(traced_fraction(0.0, 10.0, 5.0), 0.5);
        assert_eq!(traced_fraction(0.0, 10.0, 25.0), 1.0);
        assert_eq!(traced_fraction(10.0, 10.0, 5.0), 0.0, "later strokes wait their turn");
        assert_eq!(traced_fraction(10.0, 10.0, 15.0), 0.5);
        assert_eq!(traced_fraction(4.0, 0.0, 5.0), 1.0, "a zero span is whole once reached");
        assert_eq!(traced_fraction(4.0, 0.0, 3.0), 0.0);
    }

    #[test]
    fn dots_take_a_short_nominal_span() {
        let dot = Primitive::Dot { centre: (1.0, 1.0), radius: 3.0 };
        assert_eq!(dot.span(), DOT_SPAN);
        let line = Primitive::Segment { from: (0.0, 0.0), to: (3.0, 4.0) };
        assert_eq!(line.span(), 5.0);
    }
}
