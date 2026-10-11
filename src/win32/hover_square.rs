//! Leave rules for hover popups: the tab card and the island menu. A popup opened from a trigger stays
//! open while the pointer is inside its square, the bounding rectangle of the trigger, the gap between
//! the trigger and the popup, and the popup, grown by a small tolerance. Pure functions, so the rules
//! are unit tested without a window.

use windows::Win32::Foundation::{POINT, RECT};

/// Logical tolerance added on every side of the square, so a pointer that grazes an edge keeps the
/// popup open.
pub(crate) const TOLERANCE: i32 = 4;

/// What a hover popup does once the pointer has left its trigger or the popup itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Leave {
    /// The pointer is on the popup, or a field in it has the keyboard. Nothing is scheduled.
    Keep,
    /// The pointer is inside the square but off the popup, such as in the gap. Check again shortly.
    Watch,
    /// The pointer is outside the square. Close now.
    Close,
}

/// The square around `trigger` and `popup` (both in screen coordinates), grown by `tolerance`.
pub(crate) fn square(trigger: RECT, popup: RECT, tolerance: i32) -> RECT {
    RECT {
        left: trigger.left.min(popup.left) - tolerance,
        top: trigger.top.min(popup.top) - tolerance,
        right: trigger.right.max(popup.right) + tolerance,
        bottom: trigger.bottom.max(popup.bottom) + tolerance,
    }
}

/// Half-open containment, as hit testing uses everywhere else in the chrome.
pub(crate) fn contains(rect: RECT, point: POINT) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

/// Decides what a pointer that has just left the trigger or the popup should do.
pub(crate) fn on_leave(in_popup: bool, in_square: bool, focused: bool) -> Leave {
    if in_popup || focused {
        Leave::Keep
    } else if in_square {
        Leave::Watch
    } else {
        Leave::Close
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    fn at(x: i32, y: i32) -> POINT {
        POINT { x, y }
    }

    /// A 100×24 trigger with a 260×200 popup hanging 4px below it, as the tab card is laid out.
    fn trigger_and_popup() -> (RECT, RECT) {
        (rect(100, 0, 200, 24), rect(100, 28, 360, 228))
    }

    #[test]
    fn the_square_encloses_trigger_gap_and_popup_with_tolerance() {
        let (trigger, popup) = trigger_and_popup();
        assert_eq!(square(trigger, popup, TOLERANCE), rect(96, -4, 364, 232));
    }

    #[test]
    fn the_gap_between_trigger_and_popup_is_inside_the_square() {
        let (trigger, popup) = trigger_and_popup();
        let square = square(trigger, popup, 0);
        // Between the trigger's bottom (24) and the popup's top (28).
        assert!(contains(square, at(150, 26)));
        assert!(!contains(square, at(150, 228)));
    }

    #[test]
    fn a_popup_wider_than_its_trigger_widens_the_square_on_both_sides() {
        let (trigger, popup) = trigger_and_popup();
        let square = square(trigger, popup, 0);
        assert!(contains(square, at(101, 150)));
        assert!(contains(square, at(359, 150)));
        assert!(!contains(square, at(360, 150)));
        assert!(!contains(square, at(99, 150)));
    }

    #[test]
    fn containment_is_half_open_on_the_right_and_bottom() {
        let r = rect(0, 0, 10, 10);
        assert!(contains(r, at(0, 0)));
        assert!(contains(r, at(9, 9)));
        assert!(!contains(r, at(10, 5)));
        assert!(!contains(r, at(5, 10)));
        assert!(!contains(r, at(-1, 5)));
    }

    #[test]
    fn crossing_the_gap_keeps_the_popup_and_leaving_the_square_closes_it() {
        assert_eq!(on_leave(false, true, false), Leave::Watch);
        assert_eq!(on_leave(false, false, false), Leave::Close);
    }

    #[test]
    fn the_popup_itself_keeps_it_open_wherever_the_square_is() {
        assert_eq!(on_leave(true, true, false), Leave::Keep);
        assert_eq!(on_leave(true, false, false), Leave::Keep);
    }

    #[test]
    fn a_focused_text_field_keeps_the_popup_open_even_outside_the_square() {
        assert_eq!(on_leave(false, false, true), Leave::Keep);
        assert_eq!(on_leave(false, true, true), Leave::Keep);
    }
}
