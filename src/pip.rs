//! Pure policy for choosing a video and tracking one browser-owned PiP session.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoCandidate {
    pub playing: bool,
    pub picture_in_picture_disabled: bool,
    pub ready_state: u8,
    pub width: u32,
    pub height: u32,
}

impl VideoCandidate {
    pub fn eligible(self) -> bool {
        self.playing
            && !self.picture_in_picture_disabled
            && self.ready_state >= 2
            && self.width > 0
            && self.height > 0
    }

    fn visible_area(self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

/// Returns the index of the largest eligible visible video; ties favor the earlier DOM element.
pub fn select_video(candidates: &[VideoCandidate]) -> Option<usize> {
    candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.eligible())
        .max_by(|(left_index, left), (right_index, right)| {
            left.visible_area()
                .cmp(&right.visible_area())
                .then_with(|| right_index.cmp(left_index))
        })
        .map(|(index, _)| index)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipState {
    Idle,
    Entering(u32),
    Active(u32),
    Exiting(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipEvent {
    EnterRequested(u32),
    Entered,
    EnterFailed,
    ExitRequested,
    Exited,
    Navigated(u32),
    TabClosed(u32),
    MediaStopped(u32),
}

impl PipState {
    pub fn transition(self, event: PipEvent) -> Self {
        match (self, event) {
            (Self::Idle, PipEvent::EnterRequested(tab)) => Self::Entering(tab),
            (Self::Entering(tab), PipEvent::Entered) => Self::Active(tab),
            (Self::Entering(_), PipEvent::EnterFailed | PipEvent::Exited) => Self::Idle,
            (Self::Active(tab), PipEvent::ExitRequested) => Self::Exiting(tab),
            (Self::Exiting(_), PipEvent::Exited | PipEvent::EnterFailed) => Self::Idle,
            (Self::Entering(tab) | Self::Active(tab) | Self::Exiting(tab), event)
                if matches!(
                    event,
                    PipEvent::Navigated(owner)
                        | PipEvent::TabClosed(owner)
                        | PipEvent::MediaStopped(owner)
                        if owner == tab
                ) =>
            {
                Self::Idle
            }
            (state, _) => state,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(
        playing: bool,
        disabled: bool,
        ready_state: u8,
        width: u32,
        height: u32,
    ) -> VideoCandidate {
        VideoCandidate {
            playing,
            picture_in_picture_disabled: disabled,
            ready_state,
            width,
            height,
        }
    }

    #[test]
    fn selection_skips_ineligible_videos_and_picks_largest_visible_playing_video() {
        let candidates = [
            video(false, false, 4, 1920, 1080),
            video(true, true, 4, 1280, 720),
            video(true, false, 1, 1280, 720),
            video(true, false, 4, 0, 720),
            video(true, false, 4, 640, 360),
            video(true, false, 4, 1280, 720),
        ];

        assert_eq!(select_video(&candidates), Some(5));
    }

    #[test]
    fn selection_returns_none_when_no_video_is_eligible() {
        assert_eq!(select_video(&[video(false, false, 4, 100, 100)]), None);
    }

    #[test]
    fn selection_keeps_dom_order_when_eligible_videos_have_equal_area() {
        let candidates = [
            video(true, false, 4, 640, 360),
            video(true, false, 4, 480, 480),
        ];
        assert_eq!(select_video(&candidates), Some(0));
    }

    #[test]
    fn media_stop_navigation_and_tab_close_return_session_to_idle() {
        let active = PipState::Active(7);
        assert_eq!(active.transition(PipEvent::MediaStopped(7)), PipState::Idle);
        assert_eq!(active.transition(PipEvent::Navigated(7)), PipState::Idle);
        assert_eq!(active.transition(PipEvent::TabClosed(7)), PipState::Idle);
    }

    #[test]
    fn failed_entry_and_exit_complete_without_leaving_a_stale_owner() {
        assert_eq!(
            PipState::Idle
                .transition(PipEvent::EnterRequested(7))
                .transition(PipEvent::EnterFailed),
            PipState::Idle
        );
        assert_eq!(
            PipState::Active(7)
                .transition(PipEvent::ExitRequested)
                .transition(PipEvent::Exited),
            PipState::Idle
        );
    }

    #[test]
    fn cleanup_for_a_different_tab_does_not_end_the_picture_in_picture_session() {
        let active = PipState::Active(7);
        assert_eq!(active.transition(PipEvent::TabClosed(8)), active);
        assert_eq!(active.transition(PipEvent::MediaStopped(8)), active);
    }
}
