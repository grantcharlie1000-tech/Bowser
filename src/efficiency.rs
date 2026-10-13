//! Efficiency mode: trims background work when the laptop is on battery or in Battery Saver.
//!
//! Everything that decides anything is a pure function over plain values, so the rules are tested
//! without Windows. The only system calls are `GetSystemPowerStatus` (a cheap read) and
//! `SetProcessInformation` for EcoQoS on Bowser's own process.
//!
//! When efficiency is active it changes four things and nothing else:
//! * hidden tabs are asked to suspend after [`EFFICIENT_SUSPEND_AFTER`] instead of 30 s;
//! * the tab sleep delay is capped at [`EFFICIENT_SLEEP_CAP_SECS`] (never lengthened);
//! * every background tab is held at the low memory target straight away;
//! * Bowser's own process runs at EcoQoS while the window is minimised.
//!
//! Which tabs may sleep is unchanged: the eligibility rules in `app.rs` still exclude audible,
//! picture-in-picture, capturing, loading, pinned and active tabs.

use std::time::Duration;

use crate::settings::EfficiencyMode;

/// A hidden tab is asked to suspend after this long when efficiency is not active.
pub const NORMAL_SUSPEND_AFTER: Duration = Duration::from_secs(30);
/// ... and after this long when it is.
pub const EFFICIENT_SUSPEND_AFTER: Duration = Duration::from_secs(10);
/// While efficient, a configured sleep delay longer than this is shortened to it.
pub const EFFICIENT_SLEEP_CAP_SECS: u64 = 120;
/// Fallback re-check of the power source, in case a power broadcast is missed. Deliberately slow.
pub const POWER_FALLBACK_MS: u32 = 60_000;

/// What the machine reports about its power source.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PowerState {
    pub on_battery: bool,
    pub battery_saver: bool,
}

/// What the app remembers between evaluations.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EfficiencyState {
    pub power: PowerState,
    pub active: bool,
    /// False until the first evaluation, so the first result is always reported.
    pub evaluated: bool,
    /// Whether EcoQoS is currently set on the process.
    pub eco_qos: bool,
}

impl EfficiencyState {
    /// Records a fresh reading. Returns whether `active` changed or this was the first reading.
    pub fn evaluate(&mut self, mode: EfficiencyMode, power: PowerState) -> bool {
        let active = efficiency_active(mode, power);
        let changed = !self.evaluated || active != self.active;
        self.power = power;
        self.active = active;
        self.evaluated = true;
        changed
    }
}

/// The EcoQoS change to make, if any, given what is set now and what is wanted.
pub fn eco_qos_change(applied: bool, wanted: bool) -> Option<bool> {
    (applied != wanted).then_some(wanted)
}

/// Whether efficiency mode is in force. `Auto` follows the power source.
pub fn efficiency_active(mode: EfficiencyMode, power: PowerState) -> bool {
    match mode {
        EfficiencyMode::On => true,
        EfficiencyMode::Off => false,
        EfficiencyMode::Auto => power.on_battery || power.battery_saver,
    }
}

/// Turns the raw `SYSTEM_POWER_STATUS` bytes into a [`PowerState`]. `ac_line_status` is 0 offline,
/// 1 online and 255 unknown; only an explicit 0 counts as battery. `system_status_flag` is 1 while
/// Battery Saver is on.
pub fn power_from_status(ac_line_status: u8, system_status_flag: u8) -> PowerState {
    PowerState {
        on_battery: ac_line_status == 0,
        battery_saver: system_status_flag == 1,
    }
}

/// Delays that govern hidden tabs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackgroundPolicy {
    /// How long a tab stays hidden before it is asked to suspend.
    pub suspend_after: Duration,
    /// How long before a suspended hidden tab is slept (discarded). `None` when the user turned
    /// tab sleep off, which efficiency mode never overrides.
    pub sleep_after: Option<Duration>,
    /// Whether background tabs are forced to the low memory target immediately.
    pub force_low_memory: bool,
}

pub fn background_policy(active: bool, sleep_enabled: bool, sleep_after_secs: u64) -> BackgroundPolicy {
    let configured = sleep_enabled.then_some(sleep_after_secs);
    if active {
        BackgroundPolicy {
            suspend_after: EFFICIENT_SUSPEND_AFTER,
            sleep_after: configured.map(|secs| Duration::from_secs(secs.min(EFFICIENT_SLEEP_CAP_SECS))),
            force_low_memory: true,
        }
    } else {
        BackgroundPolicy {
            suspend_after: NORMAL_SUSPEND_AFTER,
            sleep_after: configured.map(Duration::from_secs),
            force_low_memory: false,
        }
    }
}

/// EcoQoS is applied only while nobody can be looking at the window: efficiency is active and the
/// window is minimised. It is never applied while the user interacts or a page animates in front.
pub fn ecoqos_wanted(active: bool, minimized: bool) -> bool {
    active && minimized
}

/// Parses the debug-only `BOWSER_FORCE_POWER` value: `battery`, `saver` or `ac`.
pub fn parse_forced_power(value: &str) -> Option<PowerState> {
    match value.trim().to_ascii_lowercase().as_str() {
        "battery" => Some(PowerState { on_battery: true, battery_saver: false }),
        "saver" => Some(PowerState { on_battery: false, battery_saver: true }),
        "ac" => Some(PowerState::default()),
        _ => None,
    }
}

/// Test hook: debug builds honour `BOWSER_FORCE_POWER`; release builds ignore it entirely.
fn forced_power() -> Option<PowerState> {
    if cfg!(debug_assertions) {
        std::env::var("BOWSER_FORCE_POWER").ok().and_then(|value| parse_forced_power(&value))
    } else {
        None
    }
}

/// The machine's current power source. A failed query reads as mains power, so a failure never
/// switches efficiency on.
pub fn current_power() -> PowerState {
    if let Some(forced) = forced_power() {
        return forced;
    }
    let mut status = windows::Win32::System::Power::SYSTEM_POWER_STATUS::default();
    if unsafe { windows::Win32::System::Power::GetSystemPowerStatus(&mut status) }.is_err() {
        return PowerState::default();
    }
    power_from_status(status.ACLineStatus, status.SystemStatusFlag)
}

/// Turns EcoQoS (execution-speed throttling) on or off for this process only. Returns whether
/// Windows accepted the change.
pub fn set_process_eco_qos(on: bool) -> bool {
    use windows::Win32::System::Threading::{
        GetCurrentProcess, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
        ProcessPowerThrottling, SetProcessInformation,
    };
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: if on { PROCESS_POWER_THROTTLING_EXECUTION_SPEED } else { 0 },
    };
    unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            (&state as *const PROCESS_POWER_THROTTLING_STATE).cast(),
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    }
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BATTERY: PowerState = PowerState { on_battery: true, battery_saver: false };
    const SAVER: PowerState = PowerState { on_battery: false, battery_saver: true };
    const AC: PowerState = PowerState { on_battery: false, battery_saver: false };

    #[test]
    fn auto_follows_battery_or_battery_saver() {
        assert!(!efficiency_active(EfficiencyMode::Auto, AC));
        assert!(efficiency_active(EfficiencyMode::Auto, BATTERY));
        assert!(efficiency_active(EfficiencyMode::Auto, SAVER));
        assert!(efficiency_active(EfficiencyMode::Auto, PowerState { on_battery: true, battery_saver: true }));
    }

    #[test]
    fn on_and_off_ignore_the_power_source() {
        for power in [AC, BATTERY, SAVER] {
            assert!(efficiency_active(EfficiencyMode::On, power));
            assert!(!efficiency_active(EfficiencyMode::Off, power));
        }
    }

    #[test]
    fn power_status_bytes_map_to_state() {
        assert_eq!(power_from_status(1, 0), AC);
        assert_eq!(power_from_status(0, 0), BATTERY);
        assert_eq!(power_from_status(1, 1), SAVER);
        // Unknown line status is not treated as battery.
        assert_eq!(power_from_status(255, 0), AC);
        assert_eq!(power_from_status(255, 2), AC);
    }

    #[test]
    fn efficient_policy_shortens_delays_and_forces_low_memory() {
        let normal = background_policy(false, true, 300);
        assert_eq!(normal.suspend_after, Duration::from_secs(30));
        assert_eq!(normal.sleep_after, Some(Duration::from_secs(300)));
        assert!(!normal.force_low_memory);
        let efficient = background_policy(true, true, 300);
        assert_eq!(efficient.suspend_after, Duration::from_secs(10));
        assert_eq!(efficient.sleep_after, Some(Duration::from_secs(120)));
        assert!(efficient.force_low_memory);
    }

    #[test]
    fn efficiency_never_lengthens_or_enables_sleep() {
        assert_eq!(background_policy(true, true, 45).sleep_after, Some(Duration::from_secs(45)));
        assert_eq!(background_policy(true, true, 30).sleep_after, Some(Duration::from_secs(30)));
        assert_eq!(background_policy(true, true, 120).sleep_after, Some(Duration::from_secs(120)));
        assert_eq!(background_policy(true, true, 604_800).sleep_after, Some(Duration::from_secs(120)));
        assert_eq!(background_policy(true, false, 300).sleep_after, None);
        assert_eq!(background_policy(false, false, 300).sleep_after, None);
    }

    #[test]
    fn efficient_suspend_is_still_shorter_than_the_shortest_sleep() {
        let shortest = crate::settings::MIN_SLEEP_AFTER_SECS;
        let policy = background_policy(true, true, shortest);
        assert!(policy.suspend_after < policy.sleep_after.unwrap());
    }

    #[test]
    fn eco_qos_only_while_efficient_and_minimised() {
        assert!(ecoqos_wanted(true, true));
        assert!(!ecoqos_wanted(true, false));
        assert!(!ecoqos_wanted(false, true));
        assert!(!ecoqos_wanted(false, false));
    }

    #[test]
    fn forced_power_parses_known_words_only() {
        assert_eq!(parse_forced_power("battery"), Some(BATTERY));
        assert_eq!(parse_forced_power(" Saver "), Some(SAVER));
        assert_eq!(parse_forced_power("AC"), Some(AC));
        assert_eq!(parse_forced_power(""), None);
        assert_eq!(parse_forced_power("low"), None);
    }

    #[test]
    fn evaluation_reports_the_first_reading_and_each_change_only() {
        let mut state = EfficiencyState::default();
        assert!(state.evaluate(EfficiencyMode::Auto, AC), "first reading is reported");
        assert!(!state.active);
        assert!(!state.evaluate(EfficiencyMode::Auto, AC));
        assert!(state.evaluate(EfficiencyMode::Auto, BATTERY));
        assert!(state.active);
        assert!(!state.evaluate(EfficiencyMode::Auto, SAVER), "battery to saver stays active");
        assert!(state.evaluate(EfficiencyMode::Off, SAVER));
        assert!(!state.active);
        assert!(state.evaluate(EfficiencyMode::On, AC));
        assert!(state.active);
    }

    #[test]
    fn eco_qos_changes_only_when_the_wish_differs() {
        assert_eq!(eco_qos_change(false, true), Some(true));
        assert_eq!(eco_qos_change(true, false), Some(false));
        assert_eq!(eco_qos_change(true, true), None);
        assert_eq!(eco_qos_change(false, false), None);
    }

    #[test]
    fn power_fallback_timer_is_not_a_fast_poll() {
        assert!(POWER_FALLBACK_MS >= 30_000);
    }
}
