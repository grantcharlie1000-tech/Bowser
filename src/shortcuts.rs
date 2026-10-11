//! Keyboard chord to browser command mapping (see docs/superpowers/specs/2026-10-09-bowser-glass-ui-design.md).
//!
//! Pure: the caller supplies the virtual-key code and modifier state. Modifiers must match exactly,
//! so Ctrl+Alt chords (AltGr on some layouts) never trigger a command. Escape maps to `Stop`
//! whether or not a page is loading; the caller decides when it applies.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    NewTab,
    CloseTab,
    NextTab,
    PrevTab,
    /// 0-based index, for Ctrl+1 through Ctrl+8.
    SelectTab(usize),
    /// Ctrl+9 selects the last tab whatever the count.
    SelectLastTab,
    FocusOmnibox,
    Reload,
    HardReload,
    Stop,
    Back,
    Forward,
    /// Ctrl+, opens Passwords & Passkeys.
    OpenPasswords,
    /// Ctrl+Shift+L offers credentials for the current exact origin.
    FillPassword,
    /// Ctrl+Shift+Delete opens the profile's Privacy dashboard.
    OpenPrivacyDashboard,
    /// Ctrl+Shift+B switches the ad/tracker blocker off or on for the current site.
    ToggleBlocker,
    /// Ctrl+Shift+P opens the profile menu (switch, new, delete).
    ShowProfileMenu,
    /// Ctrl+Shift+E opens the extensions menu (enable, disable, remove, install).
    ShowExtensionsMenu,
    /// Ctrl+Shift+Z undoes the latest tab group or tab placement change.
    UndoTabOrganisation,
    /// Ctrl+Shift+Y toggles native video picture-in-picture.
    TogglePictureInPicture,
    ToggleSplit,
    FocusSplitLeft,
    FocusSplitRight,
    CloseSplitPane,
}

const VK_TAB: u32 = 0x09;
const VK_ESCAPE: u32 = 0x1B;
const VK_PRIOR: u32 = 0x21;
const VK_NEXT: u32 = 0x22;
const VK_LEFT: u32 = 0x25;
const VK_RIGHT: u32 = 0x27;
const VK_1: u32 = 0x31;
const VK_8: u32 = 0x38;
const VK_9: u32 = 0x39;
const VK_B: u32 = 0x42;
const VK_D: u32 = 0x44;
const VK_E: u32 = 0x45;
const VK_L: u32 = 0x4C;
const VK_P: u32 = 0x50;
const VK_R: u32 = 0x52;
const VK_T: u32 = 0x54;
const VK_W: u32 = 0x57;
const VK_Y: u32 = 0x59;
const VK_Z: u32 = 0x5A;
const VK_DELETE: u32 = 0x2E;
const VK_OEM_COMMA: u32 = 0xBC;
const VK_F4: u32 = 0x73;
const VK_OEM_5: u32 = 0xDC;
const VK_F5: u32 = 0x74;
const VK_F6: u32 = 0x75;
const VK_BROWSER_BACK: u32 = 0xA6;
const VK_BROWSER_FORWARD: u32 = 0xA7;
const VK_BROWSER_REFRESH: u32 = 0xA8;
const VK_BROWSER_STOP: u32 = 0xA9;

pub fn command_for(key: u32, ctrl: bool, shift: bool, alt: bool) -> Option<Command> {
    match (key, ctrl, shift, alt) {
        (VK_T, true, false, false) => Some(Command::NewTab),
        (VK_W, true, false, false) | (VK_F4, true, false, false) => Some(Command::CloseTab),
        (VK_TAB, true, false, false) | (VK_NEXT, true, false, false) => Some(Command::NextTab),
        (VK_TAB, true, true, false) | (VK_PRIOR, true, false, false) => Some(Command::PrevTab),
        (key @ VK_1..=VK_8, true, false, false) => Some(Command::SelectTab((key - VK_1) as usize)),
        (VK_9, true, false, false) => Some(Command::SelectLastTab),
        (VK_L, true, false, false) | (VK_D, false, false, true) | (VK_F6, false, false, false) => {
            Some(Command::FocusOmnibox)
        }
        (VK_F5, false, false, false)
        | (VK_R, true, false, false)
        | (VK_BROWSER_REFRESH, false, false, false) => Some(Command::Reload),
        (VK_F5, true, false, false) | (VK_R, true, true, false) => Some(Command::HardReload),
        (VK_ESCAPE, false, false, false) | (VK_BROWSER_STOP, false, false, false) => {
            Some(Command::Stop)
        }
        (VK_LEFT, false, false, true) | (VK_BROWSER_BACK, false, false, false) => {
            Some(Command::Back)
        }
        (VK_RIGHT, false, false, true) | (VK_BROWSER_FORWARD, false, false, false) => {
            Some(Command::Forward)
        }
        (VK_OEM_COMMA, true, false, false) => Some(Command::OpenPasswords),
        (VK_L, true, true, false) => Some(Command::FillPassword),
        (VK_DELETE, true, true, false) => Some(Command::OpenPrivacyDashboard),
        (VK_B, true, true, false) => Some(Command::ToggleBlocker),
        (VK_P, true, true, false) => Some(Command::ShowProfileMenu),
        (VK_E, true, true, false) => Some(Command::ShowExtensionsMenu),
        (VK_Z, true, true, false) => Some(Command::UndoTabOrganisation),
        (VK_Y, true, true, false) => Some(Command::TogglePictureInPicture),
        (VK_OEM_5, true, true, false) => Some(Command::ToggleSplit),
        (VK_LEFT, true, true, false) => Some(Command::FocusSplitLeft),
        (VK_RIGHT, true, true, false) => Some(Command::FocusSplitRight),
        (VK_W, true, true, false) => Some(Command::CloseSplitPane),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every binding, as (key, ctrl, shift, alt, command). Anything not listed must map to `None`.
    fn bindings() -> Vec<(u32, bool, bool, bool, Command)> {
        let mut table = vec![
            (VK_T, true, false, false, Command::NewTab),
            (VK_W, true, false, false, Command::CloseTab),
            (VK_F4, true, false, false, Command::CloseTab),
            (VK_TAB, true, false, false, Command::NextTab),
            (VK_TAB, true, true, false, Command::PrevTab),
            (VK_NEXT, true, false, false, Command::NextTab),
            (VK_PRIOR, true, false, false, Command::PrevTab),
            (VK_9, true, false, false, Command::SelectLastTab),
            (VK_L, true, false, false, Command::FocusOmnibox),
            (VK_D, false, false, true, Command::FocusOmnibox),
            (VK_F6, false, false, false, Command::FocusOmnibox),
            (VK_F5, false, false, false, Command::Reload),
            (VK_R, true, false, false, Command::Reload),
            (VK_BROWSER_REFRESH, false, false, false, Command::Reload),
            (VK_F5, true, false, false, Command::HardReload),
            (VK_R, true, true, false, Command::HardReload),
            (VK_ESCAPE, false, false, false, Command::Stop),
            (VK_BROWSER_STOP, false, false, false, Command::Stop),
            (VK_LEFT, false, false, true, Command::Back),
            (VK_BROWSER_BACK, false, false, false, Command::Back),
            (VK_RIGHT, false, false, true, Command::Forward),
            (VK_BROWSER_FORWARD, false, false, false, Command::Forward),
            (VK_OEM_COMMA, true, false, false, Command::OpenPasswords),
            (VK_L, true, true, false, Command::FillPassword),
            (VK_DELETE, true, true, false, Command::OpenPrivacyDashboard),
            (VK_B, true, true, false, Command::ToggleBlocker),
            (VK_P, true, true, false, Command::ShowProfileMenu),
            (VK_E, true, true, false, Command::ShowExtensionsMenu),
            (VK_Z, true, true, false, Command::UndoTabOrganisation),
            (VK_Y, true, true, false, Command::TogglePictureInPicture),
            (VK_OEM_5, true, true, false, Command::ToggleSplit),
            (VK_LEFT, true, true, false, Command::FocusSplitLeft),
            (VK_RIGHT, true, true, false, Command::FocusSplitRight),
            (VK_W, true, true, false, Command::CloseSplitPane),
        ];
        for index in 0..8 {
            table.push((
                VK_1 + index as u32,
                true,
                false,
                false,
                Command::SelectTab(index),
            ));
        }
        table
    }

    #[test]
    fn ctrl_shift_l_requests_password_fill() {
        assert_eq!(
            command_for(VK_L, true, true, false),
            Some(Command::FillPassword)
        );
    }

    #[test]
    fn every_listed_binding_maps_to_its_command() {
        for (key, ctrl, shift, alt, expected) in bindings() {
            assert_eq!(
                command_for(key, ctrl, shift, alt),
                Some(expected),
                "key {key:#04X} ctrl={ctrl} shift={shift} alt={alt}"
            );
        }
    }

    #[test]
    fn no_unlisted_key_or_modifier_combination_is_bound() {
        let table = bindings();
        for key in 0..=0xFF_u32 {
            for modifiers in 0..8_u8 {
                let (ctrl, shift, alt) =
                    (modifiers & 1 != 0, modifiers & 2 != 0, modifiers & 4 != 0);
                let expected = table
                    .iter()
                    .find(|binding| {
                        binding.0 == key && (binding.1, binding.2, binding.3) == (ctrl, shift, alt)
                    })
                    .map(|binding| binding.4);
                assert_eq!(
                    command_for(key, ctrl, shift, alt),
                    expected,
                    "key {key:#04X} ctrl={ctrl} shift={shift} alt={alt}"
                );
            }
        }
    }

    #[test]
    fn near_miss_chords_are_not_bound() {
        for (key, ctrl, shift, alt) in [
            (VK_T, true, true, false),
            (VK_T, true, false, true),
            (VK_W, false, false, false),
            (VK_L, false, false, false),
            (VK_D, true, false, false),
            (VK_LEFT, false, false, false),
            (VK_F5, false, true, false),
            (VK_9, false, false, false),
            (VK_1, false, false, true),
        ] {
            assert_eq!(command_for(key, ctrl, shift, alt), None, "key {key:#04X}");
        }
    }

    #[test]
    fn ctrl_digits_select_tabs_by_zero_based_index_and_nine_selects_last() {
        assert_eq!(
            command_for(VK_1, true, false, false),
            Some(Command::SelectTab(0))
        );
        assert_eq!(
            command_for(VK_8, true, false, false),
            Some(Command::SelectTab(7))
        );
        assert_eq!(
            command_for(VK_9, true, false, false),
            Some(Command::SelectLastTab)
        );
    }
}
