//! Fetch body reads must finish before configuration changes or request completion commands.
//! Cancellation invalidates the rewrite, but never sends a command while a body is in flight.

#[derive(Default)]
pub(crate) struct ResponseLifecycle {
    epoch: u64,
    reading: usize,
    deferred: Option<String>,
}

impl ResponseLifecycle {
    pub fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }

    pub fn configure(&mut self, params: String) -> Option<String> {
        self.invalidate();
        if self.reading == 0 {
            Some(params)
        } else {
            self.deferred = Some(params);
            None
        }
    }

    pub fn begin(&mut self) -> u64 {
        self.reading += 1;
        self.epoch
    }

    /// The caller continues a cancelled/disabled request unchanged after the body callback.
    /// It sends any deferred configuration only after queuing that request's final answer.
    pub fn finish(&mut self, epoch: u64, policy_allows: bool) -> (bool, Option<String>) {
        self.reading -= 1;
        let rewrite = self.epoch == epoch && policy_allows;
        let configure = (self.reading == 0).then(|| self.deferred.take()).flatten();
        (rewrite, configure)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_cancels_rewrite_without_touching_the_pending_request() {
        let mut state = ResponseLifecycle::default();
        let token = state.begin();
        state.invalidate();
        assert_eq!(state.finish(token, true), (false, None));
    }

    #[test]
    fn disabling_then_reenabling_waits_for_all_bodies_and_cancels_old_rewrites() {
        let mut state = ResponseLifecycle::default();
        let first = state.begin();
        let second = state.begin();
        assert_eq!(state.configure("off".into()), None);
        assert_eq!(state.configure("on".into()), None);
        assert_eq!(state.finish(second, true), (false, None));
        assert_eq!(state.finish(first, true), (false, Some("on".into())));
        let next = state.begin();
        assert_eq!(state.finish(next, true), (true, None));
    }

    #[test]
    fn a_setting_changed_during_body_read_is_rechecked_even_without_refresh() {
        let mut state = ResponseLifecycle::default();
        let token = state.begin();
        assert_eq!(state.finish(token, false), (false, None));
        assert_eq!(state.configure("off".into()), Some("off".into()));
    }
}
