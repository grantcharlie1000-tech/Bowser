//! Interruptible chrome transitions: tab hover and selection, and the fade of a closed tab. Values
//! are pure functions of time, so painting reads them at the instant it is given. The window only
//! needs a frame timer while `Motion::animating` is true.

use std::time::{Duration, Instant};

use crate::{
    chrome::{
        group_ui::{GroupFrame, GroupState, GroupTransition, transition_bounds},
        layout::{Metrics, Rect},
    },
    tab_groups::TabGroupKey,
    tabs::TabId,
};

/// Length of every chrome transition, long enough to read as motion and short enough not to lag.
pub const TRANSITION: Duration = Duration::from_millis(150);

/// Current visual projection for one persistent group key.
#[derive(Clone, Debug)]
pub struct GroupSpec {
    pub key: TabGroupKey,
    pub state: GroupState,
    pub count: usize,
    pub shell: Rect,
}

#[derive(Clone, Debug)]
struct GroupTrack {
    key: TabGroupKey,
    state: GroupState,
    count: usize,
    shell: Rect,
    transition: Option<GroupTransition>,
}

/// One eased value moving towards a target. Retargeting starts from the value on screen, so a
/// transition that is interrupted never jumps.
#[derive(Clone, Copy, Debug)]
struct Transition {
    from: f32,
    to: f32,
    started: Instant,
}

impl Transition {
    fn resting(value: f32, now: Instant) -> Self {
        Self {
            from: value,
            to: value,
            started: now,
        }
    }

    fn value(&self, now: Instant) -> f32 {
        let eased = ease_out(progress(self.started, now));
        self.from + (self.to - self.from) * eased
    }

    /// Returns whether the target changed.
    fn retarget(&mut self, to: f32, now: Instant, snap: bool) -> bool {
        if to == self.to {
            return false;
        }
        self.from = if snap { to } else { self.value(now) };
        self.to = to;
        self.started = now;
        true
    }

    /// Lands on the target at once, ending any running fade.
    fn settle(&mut self) {
        self.from = self.to;
    }

    fn is_active(&self, now: Instant) -> bool {
        self.from != self.to && is_running(self.started, now)
    }
}

/// Hover and selection emphasis for one tab island, each in `0.0..=1.0`.
#[derive(Clone, Copy, Debug)]
pub struct Emphasis {
    hover: Transition,
    active: Transition,
}

impl Emphasis {
    pub fn resting(hovered: bool, selected: bool, now: Instant) -> Self {
        Self {
            hover: Transition::resting(flag(hovered), now),
            active: Transition::resting(flag(selected), now),
        }
    }

    /// The hover and selection amounts at `now`.
    pub fn amounts(&self, now: Instant) -> (f32, f32) {
        (self.hover.value(now), self.active.value(now))
    }

    /// Returns whether the hover or selection target changed.
    pub fn retarget(&mut self, hovered: bool, selected: bool, now: Instant, snap: bool) -> bool {
        let hover = self.hover.retarget(flag(hovered), now, snap);
        let active = self.active.retarget(flag(selected), now, snap);
        hover || active
    }

    fn settle(&mut self) {
        self.hover.settle();
        self.active.settle();
    }

    fn is_active(&self, now: Instant) -> bool {
        self.hover.is_active(now) || self.active.is_active(now)
    }
}

/// The fading ghost of a tab that was closed. It keeps the island it occupied until the fade ends.
#[derive(Clone, Copy, Debug)]
pub struct Departure {
    pub rect: Rect,
    started: Instant,
}

impl Departure {
    /// 1.0 when the tab closes, falling to 0.0 at the end of the transition.
    pub fn amount(&self, now: Instant) -> f32 {
        1.0 - ease_out(progress(self.started, now))
    }

    pub fn is_active(&self, now: Instant) -> bool {
        is_running(self.started, now)
    }
}

/// Emphasis for every open tab, plus ghosts of closed tabs. When `snap` is set, as with reduced
/// motion, every change lands at once and no ghost is drawn.
#[derive(Debug)]
pub struct Motion {
    tabs: Vec<(TabId, Emphasis)>,
    departures: Vec<Departure>,
    snap: bool,
    /// Tabs still moving at the last frame.
    moving: Vec<TabId>,
    /// Tabs to repaint on the last frame: those moving now and those that stopped moving on the
    /// frame before, so the frame that lands a transition is painted.
    repaint: Vec<TabId>,
    /// Tabs whose target changed since the last frame. Their first frame may run late, after the
    /// fade has already ended, so each one is repainted once on the next frame regardless of time.
    changed: Vec<TabId>,
    groups: Vec<GroupTrack>,
    moving_groups: Vec<TabGroupKey>,
    repaint_groups: Vec<TabGroupKey>,
    changed_groups: Vec<TabGroupKey>,
}

impl Motion {
    pub fn new(snap: bool) -> Self {
        Self {
            tabs: Vec::with_capacity(8),
            departures: Vec::with_capacity(2),
            snap,
            moving: Vec::with_capacity(8),
            repaint: Vec::with_capacity(8),
            changed: Vec::with_capacity(8),
            groups: Vec::with_capacity(4),
            moving_groups: Vec::with_capacity(4),
            repaint_groups: Vec::with_capacity(4),
            changed_groups: Vec::with_capacity(4),
        }
    }

    /// Turns transitions on or off. Turning them off settles every running fade and drops every
    /// ghost at once. Returns whether anything was moving, so the caller can stop the frame timer
    /// and repaint the strip.
    pub fn set_snap(&mut self, snap: bool, now: Instant) -> bool {
        self.snap = snap;
        if !snap {
            return false;
        }
        let changed = !self.moving.is_empty() || !self.departures.is_empty() || self.animating(now);
        for (_, emphasis) in &mut self.tabs {
            emphasis.settle();
        }
        self.departures.clear();
        self.moving.clear();
        self.repaint.clear();
        self.changed.clear();
        self.moving_groups.clear();
        self.repaint_groups.clear();
        self.changed_groups.clear();
        for group in &mut self.groups {
            group.transition = None;
        }
        changed
    }

    pub fn emphasis(&self, id: TabId) -> Option<Emphasis> {
        self.tabs
            .iter()
            .find(|(tab, _)| *tab == id)
            .map(|(_, emphasis)| *emphasis)
    }

    pub fn departures(&self) -> &[Departure] {
        &self.departures
    }

    /// The tabs whose island the last frame asked to repaint.
    pub fn repaint(&self) -> &[TabId] {
        &self.repaint
    }

    pub fn repaint_groups(&self) -> &[TabGroupKey] {
        &self.repaint_groups
    }

    /// Synchronizes the visual projection while preserving transitions by stable group key.
    pub fn sync_groups(&mut self, specs: &[GroupSpec], now: Instant) {
        self.groups
            .retain(|group| specs.iter().any(|spec| spec.key == group.key));
        for spec in specs {
            if let Some(group) = self.groups.iter_mut().find(|group| group.key == spec.key) {
                if group.state != spec.state
                    || group.count != spec.count
                    || group.shell != spec.shell
                {
                    if !self.snap {
                        group.transition = Some(GroupTransition::new(
                            group.state,
                            spec.state,
                            group.count,
                            spec.count,
                            group.shell,
                            spec.shell,
                            now,
                        ));
                        if !self.changed_groups.contains(&spec.key) {
                            self.changed_groups.push(spec.key.clone());
                        }
                    } else {
                        group.transition = None;
                    }
                    group.state = spec.state;
                    group.count = spec.count;
                    group.shell = spec.shell;
                }
            } else {
                self.groups.push(GroupTrack {
                    key: spec.key.clone(),
                    state: spec.state,
                    count: spec.count,
                    shell: spec.shell,
                    transition: None,
                });
            }
        }
    }

    pub fn group_frame(&self, key: &TabGroupKey, now: Instant) -> Option<GroupFrame> {
        let group = self.groups.iter().find(|group| &group.key == key)?;
        Some(group.transition.map_or_else(
            || GroupFrame::resting(group.shell, group.state, group.count),
            |transition| transition.frame(now, self.snap),
        ))
    }

    pub fn group_repaint_bounds(
        &self,
        key: &TabGroupKey,
        now: Instant,
        metrics: Metrics,
    ) -> Option<Rect> {
        let frame = self.group_frame(key, now)?;
        Some(transition_bounds(frame, metrics))
    }

    /// Follows the current tab list. A tab that is gone starts a departure from `rect_of`, which
    /// gives its last island rectangle. A new tab appears already at rest.
    pub fn sync(
        &mut self,
        ids: &[TabId],
        hovered: Option<TabId>,
        selected: Option<TabId>,
        rect_of: impl Fn(TabId) -> Option<Rect>,
        now: Instant,
    ) {
        for (id, _) in &self.tabs {
            if ids.contains(id) || self.snap {
                continue;
            }
            if let Some(rect) = rect_of(*id) {
                self.departures.push(Departure { rect, started: now });
            }
        }
        self.tabs.retain(|(id, _)| ids.contains(id));
        for &id in ids {
            if self.emphasis(id).is_none() {
                let emphasis = Emphasis::resting(hovered == Some(id), selected == Some(id), now);
                self.tabs.push((id, emphasis));
            }
        }
        self.retarget(hovered, selected, now);
    }

    /// Moves every tab towards its hover and selection target. A target that has not changed keeps
    /// its running transition, so repeated pointer moves do not restart the fade.
    pub fn retarget(&mut self, hovered: Option<TabId>, selected: Option<TabId>, now: Instant) {
        let snap = self.snap;
        for (id, emphasis) in &mut self.tabs {
            let changed = emphasis.retarget(hovered == Some(*id), selected == Some(*id), now, snap);
            // A snapped change never moves, so no frame follows to drain the queue.
            if changed && !snap && !self.changed.contains(id) {
                self.changed.push(*id);
            }
        }
    }

    /// True while any tab emphasis or departure is still moving. The frame timer runs only then.
    pub fn animating(&self, now: Instant) -> bool {
        self.tabs
            .iter()
            .any(|(_, emphasis)| emphasis.is_active(now))
            || self.departures.iter().any(|d| d.is_active(now))
            || self.groups.iter().any(|group| {
                group
                    .transition
                    .is_some_and(|transition| transition.is_animating(now, self.snap))
            })
    }

    /// Advances one frame and records which tabs it must repaint. A tab still moving is repainted,
    /// and so is a tab that stopped on the previous frame, which lands its settled pixels. A tab
    /// whose target changed since the last frame is repainted once, even if its fade has already
    /// ended by the time this frame runs. Returns whether another frame is needed.
    pub fn step(&mut self, now: Instant) -> bool {
        self.repaint.clear();
        self.repaint.extend_from_slice(&self.moving);
        self.moving.clear();
        for (id, emphasis) in &self.tabs {
            if emphasis.is_active(now) {
                self.moving.push(*id);
            }
        }
        for &id in self.moving.iter().chain(&self.changed) {
            if !self.repaint.contains(&id) {
                self.repaint.push(id);
            }
        }
        self.changed.clear();
        self.repaint_groups.clear();
        self.repaint_groups
            .extend(self.moving_groups.iter().cloned());
        self.moving_groups.clear();
        for group in &self.groups {
            if group
                .transition
                .is_some_and(|transition| transition.is_animating(now, self.snap))
            {
                self.moving_groups.push(group.key.clone());
            }
        }
        for key in self.moving_groups.iter().chain(&self.changed_groups) {
            if !self.repaint_groups.contains(key) {
                self.repaint_groups.push(key.clone());
            }
        }
        self.changed_groups.clear();
        self.animating(now)
    }

    /// Forgets departures whose fade has finished. Call this only after repainting their rects.
    pub fn prune(&mut self, now: Instant) {
        self.departures.retain(|d| d.is_active(now));
        for group in &mut self.groups {
            if group
                .transition
                .is_some_and(|transition| !transition.is_animating(now, self.snap))
            {
                group.transition = None;
            }
        }
    }
}

fn flag(value: bool) -> f32 {
    if value { 1.0 } else { 0.0 }
}

fn progress(started: Instant, now: Instant) -> f32 {
    let elapsed = now.saturating_duration_since(started).as_secs_f32();
    (elapsed / TRANSITION.as_secs_f32()).clamp(0.0, 1.0)
}

fn is_running(started: Instant, now: Instant) -> bool {
    now.saturating_duration_since(started) < TRANSITION
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    #[test]
    fn a_transition_eases_to_its_target_and_then_stops() {
        let start = Instant::now();
        let mut e = Emphasis::resting(false, false, start);
        e.retarget(true, false, start, false);
        assert_eq!(e.amounts(start).0, 0.0);
        let mid = e.amounts(at(start, 75)).0;
        assert!(mid > 0.5 && mid < 1.0, "eased midpoint was {mid}");
        assert!(e.is_active(at(start, 75)));
        // Ease-out covers most of the distance early, so the first frame is already moving.
        assert!(e.amounts(at(start, 30)).0 > 0.3);
        let end = at(start, TRANSITION.as_millis() as u64);
        assert_eq!(e.amounts(end).0, 1.0);
        assert!(!e.is_active(end), "the timer must stop once the fade lands");
    }

    #[test]
    fn retargeting_mid_transition_starts_from_the_value_on_screen() {
        let start = Instant::now();
        let mut e = Emphasis::resting(false, false, start);
        e.retarget(true, false, start, false);
        let now = at(start, 60);
        let before = e.amounts(now).0;
        e.retarget(false, false, now, false);
        assert_eq!(e.amounts(now).0, before, "reversing must not jump");
        let later = e.amounts(at(start, 60 + 150)).0;
        assert_eq!(later, 0.0);
        assert!(!e.is_active(at(start, 60 + 150)));
    }

    #[test]
    fn repeating_the_same_target_does_not_restart_the_fade() {
        let start = Instant::now();
        let mut e = Emphasis::resting(false, false, start);
        e.retarget(true, false, start, false);
        e.retarget(true, false, at(start, 100), false);
        assert!(
            !e.is_active(at(start, TRANSITION.as_millis() as u64)),
            "the original transition should still end on schedule"
        );
    }

    #[test]
    fn snapping_lands_at_once_without_a_timer() {
        let start = Instant::now();
        let mut e = Emphasis::resting(false, false, start);
        e.retarget(true, false, start, true);
        assert_eq!(e.amounts(start), (1.0, 0.0));
        assert!(!e.is_active(start));
    }

    #[test]
    fn selection_moves_between_tabs_and_new_tabs_appear_at_rest() {
        let start = Instant::now();
        let (a, b, c) = (TabId(1), TabId(2), TabId(3));
        let mut motion = Motion::new(false);
        motion.sync(&[a, b], None, Some(a), |_| None, start);
        assert!(!motion.animating(start), "the first tabs appear settled");
        motion.sync(&[a, b], None, Some(b), |_| None, at(start, 10));
        assert!(motion.animating(at(start, 10)));
        assert_eq!(motion.emphasis(b).unwrap().amounts(at(start, 160)).1, 1.0);
        assert_eq!(motion.emphasis(a).unwrap().amounts(at(start, 160)).1, 0.0);
        motion.sync(&[a, b, c], None, Some(b), |_| None, at(start, 200));
        assert!(
            !motion.animating(at(start, 200)),
            "a new tab does not animate"
        );
    }

    #[test]
    fn a_closed_tab_leaves_a_fading_ghost_that_is_pruned_after_the_fade() {
        let start = Instant::now();
        let (a, b) = (TabId(1), TabId(2));
        let island = Rect::new(100, 8, 300, 40);
        let mut motion = Motion::new(false);
        motion.sync(&[a, b], None, Some(a), |_| None, start);
        motion.sync(&[a], None, Some(a), |id| (id == b).then_some(island), start);
        assert_eq!(motion.departures().len(), 1);
        assert_eq!(motion.departures()[0].rect, island);
        let fading = at(start, 75);
        assert!(motion.animating(fading));
        let amount = motion.departures()[0].amount(fading);
        assert!(amount > 0.0 && amount < 1.0, "ghost amount was {amount}");
        assert!(motion.emphasis(b).is_none(), "the closed tab is forgotten");

        let done = at(start, TRANSITION.as_millis() as u64);
        assert!(
            !motion.animating(done),
            "the timer stops when the ghost is gone"
        );
        motion.prune(done);
        assert!(motion.departures().is_empty());
    }

    #[test]
    fn snapping_closes_without_a_ghost() {
        let start = Instant::now();
        let (a, b) = (TabId(1), TabId(2));
        let mut motion = Motion::new(true);
        motion.sync(&[a, b], None, Some(a), |_| None, start);
        motion.sync(
            &[a],
            None,
            Some(a),
            |_| Some(Rect::new(0, 0, 10, 10)),
            start,
        );
        assert!(motion.departures().is_empty());
        assert!(!motion.animating(start));
    }

    #[test]
    fn reduced_motion_mid_transition_settles_fades_and_drops_ghosts() {
        let start = Instant::now();
        let (a, b, c) = (TabId(1), TabId(2), TabId(3));
        let island = Rect::new(100, 8, 300, 40);
        let mut motion = Motion::new(false);
        motion.sync(&[a, b, c], Some(c), Some(a), |_| None, start);
        motion.sync(
            &[a, b],
            Some(c),
            Some(b),
            |id| (id == c).then_some(island),
            at(start, 10),
        );
        let now = at(start, 60);
        assert!(
            motion.animating(now),
            "the selection and ghost are still fading"
        );
        assert!(
            motion.set_snap(true, now),
            "a running fade is a visible change"
        );
        assert!(
            !motion.animating(now),
            "the timer must stop once reduced motion lands"
        );
        assert!(motion.departures().is_empty());
        assert_eq!(motion.emphasis(b).unwrap().amounts(now), (0.0, 1.0));
        assert_eq!(motion.emphasis(a).unwrap().amounts(now), (0.0, 0.0));
        assert!(!motion.step(now), "no frame is needed after settling");
    }

    #[test]
    fn retargeting_while_reduced_motion_is_on_still_snaps() {
        let start = Instant::now();
        let (a, b) = (TabId(1), TabId(2));
        let mut motion = Motion::new(false);
        motion.sync(&[a, b], None, Some(a), |_| None, start);
        motion.set_snap(true, start);
        motion.sync(&[a, b], None, Some(b), |_| None, at(start, 10));
        assert_eq!(motion.emphasis(b).unwrap().amounts(at(start, 10)).1, 1.0);
        assert!(!motion.animating(at(start, 10)));
    }

    #[test]
    fn turning_motion_off_while_idle_changes_nothing_visible() {
        let start = Instant::now();
        let a = TabId(1);
        let mut motion = Motion::new(false);
        motion.sync(&[a], None, Some(a), |_| None, start);
        assert!(!motion.set_snap(true, start));
        assert!(!motion.set_snap(false, start));
    }

    #[test]
    fn a_frame_repaints_only_the_tabs_that_move_and_the_one_that_lands() {
        let start = Instant::now();
        let (a, b, c) = (TabId(1), TabId(2), TabId(3));
        let mut motion = Motion::new(false);
        motion.sync(&[a, b, c], None, Some(a), |_| None, start);
        motion.sync(&[a, b, c], None, Some(b), |_| None, at(start, 10));

        assert!(motion.step(at(start, 40)));
        assert_eq!(
            motion.repaint(),
            &[a, b],
            "the untouched tab is not repainted"
        );

        assert!(motion.step(at(start, 100)));
        assert_eq!(motion.repaint(), &[a, b]);

        let landed = at(start, 10 + TRANSITION.as_millis() as u64);
        assert!(
            !motion.step(landed),
            "the timer stops on the frame that lands"
        );
        assert_eq!(
            motion.repaint(),
            &[a, b],
            "the landing frame paints the settled tabs"
        );

        assert!(!motion.step(at(start, 200)));
        assert!(motion.repaint().is_empty(), "idle frames repaint nothing");
    }

    #[test]
    fn group_morphs_use_the_shared_timer_dirty_bounds_and_reduced_motion() {
        let start = Instant::now();
        let key = TabGroupKey::Opener(TabId(9));
        let expanded = GroupSpec {
            key: key.clone(),
            state: GroupState::Expanded,
            count: 5,
            shell: Rect::new(100, 8, 420, 32),
        };
        let collapsed = GroupSpec {
            key: key.clone(),
            state: GroupState::CollapsedWithSelection,
            count: 5,
            shell: Rect::new(100, 8, 126, 32),
        };
        let mut motion = Motion::new(false);
        motion.sync_groups(std::slice::from_ref(&expanded), start);
        assert!(!motion.animating(start), "a new group starts settled");

        let began = at(start, 10);
        motion.sync_groups(&[collapsed.clone()], began);
        assert!(
            motion.animating(began),
            "group changes use the chrome timer"
        );
        assert!(motion.step(at(start, 40)));
        assert_eq!(motion.repaint_groups(), &[key.clone()]);
        let bounds = motion
            .group_repaint_bounds(&key, at(start, 40), Metrics::new(96))
            .unwrap();
        assert!(
            bounds.right > collapsed.shell.right,
            "bounds include the ball fan"
        );

        let done = at(start, 10 + TRANSITION.as_millis() as u64);
        assert!(
            !motion.step(done),
            "the shared timer stops when groups settle"
        );
        assert_eq!(
            motion.repaint_groups(),
            &[key.clone()],
            "the landing frame paints"
        );
        motion.prune(done);
        assert!(motion.group_frame(&key, done).is_some());
        assert!(!motion.step(at(start, 200)));
        assert!(
            motion.repaint_groups().is_empty(),
            "idle groups do not repaint"
        );

        let mut reduced = Motion::new(true);
        reduced.sync_groups(std::slice::from_ref(&expanded), start);
        reduced.sync_groups(&[collapsed], began);
        assert!(
            !reduced.animating(began),
            "reduced motion settles immediately"
        );
        assert!(!reduced.step(began));
        assert!(reduced.repaint_groups().is_empty());
    }

    #[test]
    fn a_hover_that_stays_on_one_tab_repaints_only_that_tab() {
        let start = Instant::now();
        let (a, b, c) = (TabId(1), TabId(2), TabId(3));
        let mut motion = Motion::new(false);
        motion.sync(&[a, b, c], None, Some(a), |_| None, start);
        motion.sync(&[a, b, c], Some(c), Some(a), |_| None, at(start, 10));
        assert!(motion.step(at(start, 30)));
        assert_eq!(motion.repaint(), &[c]);
    }

    #[test]
    fn a_late_first_frame_still_repaints_each_changed_tab_once() {
        let start = Instant::now();
        let (a, b, c) = (TabId(1), TabId(2), TabId(3));
        let mut motion = Motion::new(false);
        motion.sync(&[a, b, c], None, Some(a), |_| None, start);

        let t0 = at(start, 10);
        motion.retarget(Some(c), Some(b), t0);
        assert!(
            !motion.animating(at(start, 210)),
            "the fades ended long ago"
        );

        assert!(!motion.step(at(start, 210)));
        assert_eq!(
            motion.repaint(),
            &[a, b, c],
            "every tab whose emphasis changed lands its settled pixels"
        );

        assert!(!motion.step(at(start, 220)));
        assert!(
            motion.repaint().is_empty(),
            "the landing repaint happens once, not on every idle frame"
        );
    }

    #[test]
    fn reduced_motion_hover_changes_queue_nothing() {
        let start = Instant::now();
        let (a, b, c) = (TabId(1), TabId(2), TabId(3));
        let mut motion = Motion::new(false);
        motion.sync(&[a, b, c], None, Some(a), |_| None, start);
        motion.set_snap(true, start);
        for step in 0..1000 {
            let now = at(start, 10 + step);
            let hovered = [a, b, c][step as usize % 3];
            motion.retarget(Some(hovered), Some(a), now);
            assert!(
                motion.changed.is_empty(),
                "snapped change queued at step {step}"
            );
        }
        let now = at(start, 2000);
        assert!(!motion.step(now));
        assert!(motion.repaint().is_empty());
    }

    #[test]
    fn repeated_reversals_queue_each_changed_tab_once() {
        let start = Instant::now();
        let (a, b, c) = (TabId(1), TabId(2), TabId(3));
        let mut motion = Motion::new(false);
        motion.sync(&[a, b, c], None, Some(a), |_| None, start);
        for step in 0..30 {
            let now = at(start, step);
            let hovered = [a, b, c][step as usize % 3];
            motion.retarget(Some(hovered), Some(a), now);
        }
        assert_eq!(motion.changed, vec![a, b, c], "one pending id per tab");
    }
}
