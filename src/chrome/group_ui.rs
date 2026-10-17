//! Pure visual model and geometry for tab group shells, balls, and transitions.

use std::time::{Duration, Instant};

use crate::chrome::layout::{Metrics, Rect};

/// Time used by group morphs. It matches the existing chrome motion cadence.
pub const GROUP_TRANSITION: Duration = Duration::from_millis(150);
/// At most this many balls are drawn in a collapsed selected group's fan.
pub const MAX_VISIBLE_STACK_BALLS: usize = 4;
/// Minimum ball hit target in logical pixels.
pub const MIN_HIT_LOGICAL: i32 = 20;
/// Minimum gap between the ball hit target and a neighbouring tab in logical pixels.
pub const MIN_HIT_GAP_LOGICAL: i32 = 3;

/// The three visible states of a collapsible group.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupState {
    Expanded,
    CollapsedWithSelection,
    CollapsedWithoutSelection,
}

/// Input that changes a group's visual state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupAction {
    Collapse,
    SwitchOutside,
    Reopen,
    SelectHidden,
}

/// Pure state transition used by the persisted model and the visual animation layer.
pub fn transition(state: GroupState, action: GroupAction) -> GroupState {
    match (state, action) {
        (GroupState::Expanded, GroupAction::Collapse) => GroupState::CollapsedWithSelection,
        (GroupState::CollapsedWithSelection, GroupAction::SwitchOutside) => {
            GroupState::CollapsedWithoutSelection
        }
        (
            GroupState::CollapsedWithSelection | GroupState::CollapsedWithoutSelection,
            GroupAction::Reopen | GroupAction::SelectHidden,
        ) => GroupState::Expanded,
        _ => state,
    }
}

/// One interruptible state/geometry transition. The caller samples it from the existing chrome
/// motion timer; it owns no timer and is idle once the 150 ms transition settles.
#[derive(Clone, Copy, Debug)]
pub struct GroupTransition {
    pub from: GroupState,
    pub to: GroupState,
    pub from_count: usize,
    pub to_count: usize,
    pub from_shell: Rect,
    pub to_shell: Rect,
    started: Instant,
}

impl GroupTransition {
    pub fn new(
        from: GroupState,
        to: GroupState,
        from_count: usize,
        to_count: usize,
        from_shell: Rect,
        to_shell: Rect,
        started: Instant,
    ) -> Self {
        Self {
            from,
            to,
            from_count,
            to_count,
            from_shell,
            to_shell,
            started,
        }
    }

    /// Cubic ease-out progress in `0..=1`; reduced motion lands immediately.
    pub fn progress(self, now: Instant, reduced_motion: bool) -> f32 {
        if reduced_motion {
            return 1.0;
        }
        let elapsed = now.saturating_duration_since(self.started).as_secs_f32();
        let raw = (elapsed / GROUP_TRANSITION.as_secs_f32()).clamp(0.0, 1.0);
        1.0 - (1.0 - raw).powi(3)
    }

    pub fn is_animating(self, now: Instant, reduced_motion: bool) -> bool {
        !reduced_motion && now.saturating_duration_since(self.started) < GROUP_TRANSITION
    }

    pub fn frame(self, now: Instant, reduced_motion: bool) -> GroupFrame {
        GroupFrame {
            from: self.from,
            state: self.to,
            from_count: self.from_count,
            count: self.to_count,
            from_shell: self.from_shell,
            shell: self.to_shell,
            progress: self.progress(now, reduced_motion),
        }
    }
}

/// A sampled group visual state. `shell` is the destination shell; geometry interpolates from
/// `from_shell` so the collapsed capsule grows and shrinks with its right edge.
#[derive(Clone, Copy, Debug)]
pub struct GroupFrame {
    pub from: GroupState,
    pub state: GroupState,
    pub from_count: usize,
    pub count: usize,
    pub from_shell: Rect,
    pub shell: Rect,
    pub progress: f32,
}

impl GroupFrame {
    pub fn resting(shell: Rect, state: GroupState, count: usize) -> Self {
        Self {
            from: state,
            state,
            from_count: count,
            count,
            from_shell: shell,
            shell,
            progress: 1.0,
        }
    }
}

/// Paint-ready geometry for one group control.
#[derive(Clone, Debug)]
pub struct GroupGeometry {
    pub shell: Rect,
    pub ball_rects: Vec<Rect>,
    pub ball_opacity: Vec<f32>,
    pub hit_rect: Rect,
    pub left_radius: i32,
    pub right_radius: i32,
    /// Horizontal inset from the shell's left edge to the first tab's close/favicon/title area.
    pub content_inset: i32,
    /// Number painted as `+N` on the last visible ball when the stack is capped.
    pub overflow_count: Option<usize>,
}

/// A tab face is 5% shorter than the normal group shell and stays vertically centred.
pub fn shrink_tab(tab: Rect, _metrics: Metrics) -> Rect {
    let height = (tab.height() * 95 + 50) / 100;
    let top = tab.top + (tab.height() - height) / 2;
    Rect::new(tab.left, top, tab.right, top + height)
}

/// Interpolates a group's shell, ball fan, content inset, and end radii for one frame.
pub fn group_geometry(frame: GroupFrame, metrics: Metrics) -> GroupGeometry {
    let progress = frame.progress.clamp(0.0, 1.0);
    let shell = interpolate_rect(frame.from_shell, frame.shell, progress);
    let diameter = (shell.height() * 95 + 50) / 100;
    let left_radius = shell.height() / 2;
    let normal_right = metrics.tab_radius();
    let from_right = right_radius(frame.from, frame.from_shell.height(), normal_right);
    let to_right = right_radius(frame.state, shell.height(), normal_right);
    let right_radius =
        (from_right as f32 + (to_right - from_right) as f32 * progress).round() as i32;
    let base = ball_at_left(shell, diameter);

    let from_count = ball_count(frame.from, frame.from_count);
    let to_count = ball_count(frame.state, frame.count);
    let draw_count = from_count.max(to_count).min(MAX_VISIBLE_STACK_BALLS);
    let mut ball_rects = Vec::with_capacity(draw_count);
    let mut ball_opacity = Vec::with_capacity(draw_count);
    for index in 0..draw_count {
        let (from_x, from_y) = fan_offset(frame.from, index, metrics);
        let (to_x, to_y) = fan_offset(frame.state, index, metrics);
        let x = (from_x as f32 + (to_x - from_x) as f32 * progress).round() as i32;
        let y = (from_y as f32 + (to_y - from_y) as f32 * progress).round() as i32;
        let rect = offset_rect(base, x, y);
        let existed_before = index < from_count;
        let exists_after = index < to_count;
        let opacity = match (existed_before, exists_after) {
            (true, true) => 1.0,
            (false, true) => progress,
            (true, false) => 1.0 - progress,
            (false, false) => 0.0,
        };
        ball_rects.push(rect);
        ball_opacity.push(opacity);
    }

    let mut hit_rect = base;
    for rect in
        ball_rects
            .iter()
            .zip(&ball_opacity)
            .enumerate()
            .filter_map(|(index, (rect, opacity))| {
                (*opacity > 0.01
                    && (frame.state != GroupState::CollapsedWithoutSelection || index == 0))
                    .then_some(*rect)
            })
    {
        hit_rect = union(hit_rect, rect);
    }
    if frame.state == GroupState::CollapsedWithoutSelection && progress >= 1.0 {
        hit_rect = union(hit_rect, shell);
    }
    let from_inset = content_inset(frame.from, frame.from_count, diameter, metrics);
    let to_inset = content_inset(frame.state, frame.count, diameter, metrics);
    let content_inset =
        (from_inset as f32 + (to_inset - from_inset) as f32 * progress).round() as i32;
    let shown_overflow = if progress >= 0.5 {
        overflow_count(frame.state, frame.count)
    } else {
        overflow_count(frame.from, frame.from_count)
    };

    GroupGeometry {
        shell,
        ball_rects,
        ball_opacity,
        hit_rect,
        left_radius,
        right_radius,
        content_inset,
        overflow_count: shown_overflow,
    }
}

/// Conservative dirty bounds for a group animation, including stacked balls at both endpoints.
pub fn transition_bounds(frame: GroupFrame, metrics: Metrics) -> Rect {
    let at_start = group_geometry(
        GroupFrame::resting(frame.from_shell, frame.from, frame.from_count),
        metrics,
    );
    let at_end = group_geometry(
        GroupFrame::resting(frame.shell, frame.state, frame.count),
        metrics,
    );
    union(
        union(frame.from_shell, frame.shell),
        union(at_start.hit_rect, at_end.hit_rect),
    )
}

fn ball_count(state: GroupState, members: usize) -> usize {
    match state {
        GroupState::CollapsedWithSelection => members.max(1),
        GroupState::Expanded | GroupState::CollapsedWithoutSelection => 1,
    }
}

fn fan_offset(state: GroupState, index: usize, metrics: Metrics) -> (i32, i32) {
    if state != GroupState::CollapsedWithSelection || index == 0 {
        return (0, 0);
    }
    let step = metrics.px(6);
    let step_y = metrics.px(1).max(1);
    let rank = index as i32;
    let y = if index % 2 == 1 {
        -((rank + 1) / 2) * step_y
    } else {
        (rank / 2) * step_y
    };
    (rank * step, y)
}

fn content_inset(state: GroupState, members: usize, diameter: i32, metrics: Metrics) -> i32 {
    if state == GroupState::CollapsedWithoutSelection {
        return 0;
    }
    let balls = ball_count(state, members).clamp(1, MAX_VISIBLE_STACK_BALLS);
    diameter + metrics.px(6) * (balls as i32 - 1) + metrics.px(4)
}

fn overflow_count(state: GroupState, members: usize) -> Option<usize> {
    if state == GroupState::CollapsedWithSelection && members > MAX_VISIBLE_STACK_BALLS {
        Some(members - MAX_VISIBLE_STACK_BALLS)
    } else {
        None
    }
}

fn right_radius(state: GroupState, shell_height: i32, normal: i32) -> i32 {
    if state == GroupState::CollapsedWithoutSelection {
        shell_height / 2
    } else {
        normal
    }
}

fn ball_at_left(shell: Rect, diameter: i32) -> Rect {
    let center_x = shell.left + shell.height() / 2;
    let center_y = (shell.top + shell.bottom) / 2;
    let left = center_x - diameter / 2;
    let top = center_y - diameter / 2;
    Rect::new(left, top, left + diameter, top + diameter)
}

fn offset_rect(rect: Rect, x: i32, y: i32) -> Rect {
    Rect::new(rect.left + x, rect.top + y, rect.right + x, rect.bottom + y)
}

fn union(a: Rect, b: Rect) -> Rect {
    Rect::new(
        a.left.min(b.left),
        a.top.min(b.top),
        a.right.max(b.right),
        a.bottom.max(b.bottom),
    )
}

fn interpolate_rect(from: Rect, to: Rect, progress: f32) -> Rect {
    let lerp = |a: i32, b: i32| (a as f32 + (b - a) as f32 * progress).round() as i32;
    Rect::new(
        lerp(from.left, to.left),
        lerp(from.top, to.top),
        lerp(from.right, to.right),
        lerp(from.bottom, to.bottom),
    )
}

pub fn state_from_model(collapsed: bool, selected_member: bool) -> GroupState {
    match (collapsed, selected_member) {
        (false, _) => GroupState::Expanded,
        (true, true) => GroupState::CollapsedWithSelection,
        (true, false) => GroupState::CollapsedWithoutSelection,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::chrome::{
        group_ui::*,
        layout::{self, Hit, Metrics, Rect},
    };

    #[test]
    fn group_actions_follow_the_three_visual_states() {
        assert_eq!(
            transition(GroupState::Expanded, GroupAction::Collapse),
            GroupState::CollapsedWithSelection
        );
        assert_eq!(
            transition(
                GroupState::CollapsedWithSelection,
                GroupAction::SwitchOutside
            ),
            GroupState::CollapsedWithoutSelection
        );
        for action in [GroupAction::Reopen, GroupAction::SelectHidden] {
            assert_eq!(
                transition(GroupState::CollapsedWithSelection, action),
                GroupState::Expanded
            );
            assert_eq!(
                transition(GroupState::CollapsedWithoutSelection, action),
                GroupState::Expanded
            );
        }
        assert_eq!(
            transition(GroupState::Expanded, GroupAction::SwitchOutside),
            GroupState::Expanded
        );
    }

    #[test]
    fn shell_and_ball_keep_the_requested_five_percent_proportions() {
        for dpi in [96, 120, 144, 192] {
            let metrics = Metrics::new(dpi);
            let shell = Rect::new(80, 8, 440, 8 + metrics.px(Metrics::ISLAND_HEIGHT));
            let tab = Rect::new(100, shell.top, 220, shell.bottom);
            let shrunk = shrink_tab(tab, metrics);
            let geometry =
                group_geometry(GroupFrame::resting(shell, GroupState::Expanded, 3), metrics);

            assert_eq!(
                shrunk.height(),
                (tab.height() * 95 + 50) / 100,
                "shrunk tab height at {dpi} dpi"
            );
            assert_eq!(geometry.ball_rects.len(), 1);
            assert_eq!(geometry.ball_rects[0].height(), shrunk.height());
            assert_eq!(geometry.left_radius, shell.height() / 2);
            assert_eq!(geometry.right_radius, metrics.tab_radius());
            assert!(geometry.shell.contains_rect(geometry.ball_rects[0]));
        }
    }

    #[test]
    fn group_tab_controls_stay_right_of_the_ball_and_inside_the_shrunken_tab() {
        for dpi in [96, 120, 144, 192] {
            let metrics = Metrics::new(dpi);
            let expanded = layout::compute_with_active(1200, 700, dpi, 3, 1, false)
                .with_group_slots([false; 3], [false, true, false], [false; 3]);
            let leader = expanded.tab(1).unwrap();
            let second = expanded.tab(2).unwrap();
            let shell = Rect::new(leader.left, leader.top, second.right, leader.bottom);
            let geometry =
                group_geometry(GroupFrame::resting(shell, GroupState::Expanded, 2), metrics);
            let parts = expanded.group_tab_parts(leader, geometry.content_inset);
            let tab = shrink_tab(leader, metrics);

            assert!(parts.close.left >= geometry.ball_rects[0].right);
            assert!(parts.favicon.left >= geometry.ball_rects[0].right);
            assert!(tab.contains_rect(parts.close));
            assert!(tab.contains_rect(parts.favicon));
            assert!(
                !parts.title.is_empty(),
                "wide titles stay visible at {dpi} dpi"
            );
            assert!(tab.contains_rect(parts.title));

            let collapsed = layout::compute_with_active(420, 700, dpi, 49, 1, false)
                .with_group_slots(
                    [false; 49],
                    [false; 49],
                    [false, true].into_iter().chain([false; 47]),
                );
            let selected = collapsed.tab(1).unwrap();
            let stack = group_geometry(
                GroupFrame::resting(selected, GroupState::CollapsedWithSelection, 8),
                metrics,
            );
            let selected_parts = collapsed.group_tab_parts(selected, stack.content_inset);
            let selected_tab = shrink_tab(selected, metrics);

            assert!(selected_parts.close.left >= stack.hit_rect.right);
            assert!(selected_tab.contains_rect(selected_parts.close));
            assert!(selected_tab.contains_rect(selected_parts.favicon));
            assert!(selected_parts.favicon.left >= stack.hit_rect.right);
        }
    }

    #[test]
    fn collapsed_selected_groups_fan_balls_and_cap_the_stack() {
        let metrics = Metrics::new(96);
        let shell = Rect::new(100, 8, 420, 32);
        let geometry = group_geometry(
            GroupFrame::resting(shell, GroupState::CollapsedWithSelection, 9),
            metrics,
        );

        assert_eq!(geometry.ball_rects.len(), MAX_VISIBLE_STACK_BALLS);
        assert!(geometry.ball_rects[1].left > geometry.ball_rects[0].left);
        assert_ne!(geometry.ball_rects[1].top, geometry.ball_rects[0].top);
        assert_eq!(geometry.overflow_count, Some(5));
        assert!(geometry.hit_rect.right >= geometry.ball_rects.last().unwrap().right);
        assert!(geometry.content_inset > geometry.ball_rects[0].width());
    }

    #[test]
    fn a_collapsed_unselected_group_is_a_single_capsule() {
        let metrics = Metrics::new(96);
        let shell = Rect::new(100, 8, 100 + metrics.px(Metrics::ISLAND_HEIGHT), 32);
        let geometry = group_geometry(
            GroupFrame::resting(shell, GroupState::CollapsedWithoutSelection, 5),
            metrics,
        );

        assert_eq!(geometry.ball_rects.len(), 1);
        assert_eq!(geometry.right_radius, geometry.left_radius);
        assert_eq!(geometry.overflow_count, None);

        let merging = group_geometry(
            GroupFrame {
                from: GroupState::CollapsedWithSelection,
                state: GroupState::CollapsedWithoutSelection,
                from_count: 6,
                count: 6,
                from_shell: Rect::new(100, 8, 196, 32),
                shell,
                progress: 0.5,
            },
            metrics,
        );
        assert!(
            merging.ball_rects.len() > 1,
            "the painted fan is still merging"
        );
        assert!(
            merging.hit_rect.right <= shell.left + metrics.px(Metrics::ISLAND_HEIGHT),
            "only the leading ball claims input while the fan merges"
        );
    }

    #[test]
    fn group_morph_progress_runs_to_completion_and_reduced_motion_snaps() {
        let started = Instant::now();
        let transition = GroupTransition::new(
            GroupState::CollapsedWithoutSelection,
            GroupState::Expanded,
            4,
            4,
            Rect::new(100, 8, 124, 32),
            Rect::new(100, 8, 420, 32),
            started,
        );
        assert_eq!(transition.progress(started, false), 0.0);
        let middle = transition.progress(started + Duration::from_millis(75), false);
        assert!(middle > 0.5 && middle < 1.0);
        assert_eq!(transition.progress(started + GROUP_TRANSITION, false), 1.0);
        assert!(transition.is_animating(started + Duration::from_millis(75), false));
        assert!(!transition.is_animating(started + GROUP_TRANSITION, false));
        assert_eq!(transition.progress(started, true), 1.0);
        assert!(!transition.is_animating(started, true));

        let mid_geometry = group_geometry(
            transition.frame(started + Duration::from_millis(25), false),
            Metrics::new(96),
        );
        assert!(mid_geometry.shell.width() > 24 && mid_geometry.shell.width() < 320);
        assert!(mid_geometry.right_radius > 9 && mid_geometry.right_radius < 12);
    }

    #[test]
    fn transition_bounds_cover_the_fanned_stack_until_it_settles() {
        let shell = Rect::new(100, 8, 126, 32);
        let frame = GroupFrame {
            from: GroupState::CollapsedWithSelection,
            state: GroupState::CollapsedWithoutSelection,
            from_count: 8,
            count: 8,
            from_shell: shell,
            shell: Rect::new(100, 8, 124, 32),
            progress: 0.65,
        };
        let bounds = transition_bounds(frame, Metrics::new(96));
        let stack_start = group_geometry(
            GroupFrame::resting(frame.from_shell, frame.from, frame.from_count),
            Metrics::new(96),
        );
        let stack_end = group_geometry(
            GroupFrame::resting(frame.shell, frame.state, frame.count),
            Metrics::new(96),
        );

        assert!(bounds.contains_rect(stack_start.hit_rect));
        assert!(bounds.contains_rect(stack_end.hit_rect));
        assert!(
            bounds.right > shell.right,
            "the fan extends beyond its shell"
        );
    }

    #[test]
    fn ball_hit_targets_precede_adjacent_tabs_at_all_supported_dpi_cases() {
        for dpi in [96, 120, 144, 192] {
            for state in [
                GroupState::Expanded,
                GroupState::CollapsedWithSelection,
                GroupState::CollapsedWithoutSelection,
            ] {
                let metrics = Metrics::new(dpi);
                let slot_count = if state == GroupState::CollapsedWithSelection {
                    23
                } else {
                    24
                };
                let active = if state == GroupState::CollapsedWithSelection {
                    1
                } else {
                    2
                };
                let layout = layout::compute_with_active(900, 700, dpi, slot_count, active, false)
                    .with_group_slots(
                        [false, state == GroupState::CollapsedWithoutSelection, false],
                        [false, state == GroupState::Expanded, false],
                        [false, state == GroupState::CollapsedWithSelection, false],
                    );
                let first = layout.tab(1).unwrap();
                let last = if state == GroupState::Expanded {
                    layout.tab(2).unwrap()
                } else {
                    first
                };
                let shell = Rect::new(first.left, first.top, last.right, last.bottom);
                let geometry = group_geometry(GroupFrame::resting(shell, state, 3), metrics);
                let ball = geometry.hit_rect;
                assert!(
                    ball.right <= first.right,
                    "{state:?}, {dpi} dpi stack stays inside its slot"
                );
                let logical_width = ball.width() * 96 / dpi as i32;
                let logical_height = ball.height() * 96 / dpi as i32;
                assert!(
                    logical_width >= MIN_HIT_LOGICAL,
                    "{state:?}, {dpi} dpi width"
                );
                assert!(
                    logical_height >= MIN_HIT_LOGICAL,
                    "{state:?}, {dpi} dpi height"
                );

                let adjacent = layout.tab(0).unwrap();
                let gap = (ball.left - adjacent.right) * 96 / dpi as i32;
                assert!(gap >= MIN_HIT_GAP_LOGICAL, "{state:?}, {dpi} dpi gap {gap}");
                assert_eq!(
                    layout::hit_test_with_group_balls(
                        &layout,
                        &[(1, ball)],
                        (ball.left + ball.right) / 2,
                        (ball.top + ball.bottom) / 2,
                    ),
                    Hit::GroupBall(1),
                    "{state:?}, {dpi} dpi ball"
                );
                assert_eq!(
                    layout::hit_test_with_group_balls(
                        &layout,
                        &[(1, ball)],
                        (adjacent.left + adjacent.right) / 2,
                        (adjacent.top + adjacent.bottom) / 2,
                    ),
                    Hit::Tab(0),
                    "{state:?}, {dpi} dpi adjacent tab"
                );
            }
        }
    }
}
