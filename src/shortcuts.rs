//! Keyboard chord to browser command mapping (see docs/superpowers/specs/2026-10-09-bowser-glass-ui-design.md).
//!
//! Pure: the caller supplies the virtual-key code and modifier state. Modifiers must match exactly,
//! so Ctrl+Alt chords (AltGr on some layouts) never trigger a command, except Ctrl+Alt+R for reader
//! view (which means AltGr+R does on layouts that report it as Ctrl+Alt). Escape maps to `Stop`
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
    /// Ctrl+Shift+M opens the profile menu (switch, new, delete).
    ShowProfileMenu,
    /// Ctrl+Shift+P opens a new private (InPrivate) tab. It took the chord that used to open the
    /// profile menu, which moved to Ctrl+Shift+M.
    NewPrivateTab,
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
    ShowCommandPalette,
    ToggleNotes,
    AddBookmark,
    /// Ctrl+H opens the History side panel.
    ShowHistory,
    /// Ctrl+J opens bowser://downloads in a tab (the bubble is the toolbar button's).
    ShowDownloads,
    /// Ctrl+Plus (and Ctrl+=, Ctrl+Numpad Plus) zooms the page in one step.
    ZoomIn,
    /// Ctrl+Minus (and Ctrl+Numpad Minus) zooms the page out one step.
    ZoomOut,
    /// Ctrl+0 (and Ctrl+Numpad 0) returns the page to 100%.
    ZoomReset,
    /// Ctrl+Alt+R turns reader view on or off for the active page. The one binding that uses
    /// Ctrl and Alt together (see `ctrl_alt_r_is_the_only_ctrl_alt_chord`).
    ToggleReader,
    /// F12 and Ctrl+Shift+I toggle the docked DevTools pane for the active tab.
    OpenDevTools,
    /// Ctrl+N opens another window for this profile.
    NewWindow,
    /// Ctrl+Shift+C copies the active tab's address. Only bound while `ctrl_shift_c_copy_link` is on
    /// (see [`GatedChords`]); otherwise the chord belongs to the page.
    CopyLink,
    /// Ctrl+S collapses or expands the vertical sidebar. Only bound while `ctrl_s_vertical_tabs` is
    /// on and the vertical layout is active (see [`GatedChords`]); otherwise the page keeps Ctrl+S.
    ToggleSidebar,
}

/// Commands that flip a setting. An auto-repeated key press must not flip it again.
pub fn is_toggle(command: Command) -> bool {
    matches!(
        command,
        Command::ToggleSplit
            | Command::ToggleBlocker
            | Command::ToggleNotes
            | Command::TogglePictureInPicture
            | Command::ToggleReader
            | Command::OpenDevTools
            | Command::ToggleSidebar
    )
}

/// Which setting-gated chords are bound right now. [`command_for`] stays the fixed table; a chord that
/// only exists while a setting is on must not be swallowed while it is off, so it is reached through
/// [`command_for_gated`] with the live state. The default binds none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GatedChords {
    /// `ctrl_shift_c_copy_link`.
    pub copy_link: bool,
    /// `ctrl_s_vertical_tabs` while the vertical tab layout is active.
    pub toggle_sidebar: bool,
}

impl GatedChords {
    pub fn new(copy_link: bool, ctrl_s_vertical_tabs: bool, vertical_layout: bool) -> Self {
        Self {
            copy_link,
            toggle_sidebar: ctrl_s_vertical_tabs && vertical_layout,
        }
    }
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
const VK_0: u32 = 0x30;
const VK_NUMPAD0: u32 = 0x60;
const VK_ADD: u32 = 0x6B;
const VK_SUBTRACT: u32 = 0x6D;
const VK_OEM_PLUS: u32 = 0xBB;
const VK_OEM_MINUS: u32 = 0xBD;
const VK_B: u32 = 0x42;
const VK_D: u32 = 0x44;
const VK_E: u32 = 0x45;
const VK_H: u32 = 0x48;
const VK_I: u32 = 0x49;
const VK_J: u32 = 0x4A;
const VK_K: u32 = 0x4B;
const VK_L: u32 = 0x4C;
const VK_M: u32 = 0x4D;
const VK_N: u32 = 0x4E;
const VK_P: u32 = 0x50;
const VK_C: u32 = 0x43;
const VK_R: u32 = 0x52;
const VK_S: u32 = 0x53;
const VK_T: u32 = 0x54;
const VK_W: u32 = 0x57;
const VK_Y: u32 = 0x59;
const VK_Z: u32 = 0x5A;
const VK_DELETE: u32 = 0x2E;
const VK_OEM_COMMA: u32 = 0xBC;
const VK_F4: u32 = 0x73;
const VK_F12: u32 = 0x7B;
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
        (VK_P, true, true, false) => Some(Command::NewPrivateTab),
        (VK_M, true, true, false) => Some(Command::ShowProfileMenu),
        (VK_K, true, true, false) => Some(Command::ShowCommandPalette),
        (VK_N, true, false, false) => Some(Command::NewWindow),
        (VK_N, true, true, false) => Some(Command::ToggleNotes),
        (VK_D, true, false, false) => Some(Command::AddBookmark),
        (VK_E, true, true, false) => Some(Command::ShowExtensionsMenu),
        (VK_Z, true, true, false) => Some(Command::UndoTabOrganisation),
        (VK_Y, true, true, false) => Some(Command::TogglePictureInPicture),
        (VK_OEM_5, true, true, false) => Some(Command::ToggleSplit),
        (VK_LEFT, true, true, false) => Some(Command::FocusSplitLeft),
        (VK_RIGHT, true, true, false) => Some(Command::FocusSplitRight),
        (VK_W, true, true, false) => Some(Command::CloseSplitPane),
        (VK_H, true, false, false) => Some(Command::ShowHistory),
        (VK_J, true, false, false) => Some(Command::ShowDownloads),
        // Ctrl+= and Ctrl+Shift+= (Ctrl+Plus on most layouts) both zoom in, as in Chrome.
        (VK_OEM_PLUS, true, _, false) | (VK_ADD, true, false, false) => Some(Command::ZoomIn),
        (VK_OEM_MINUS, true, false, false) | (VK_SUBTRACT, true, false, false) => {
            Some(Command::ZoomOut)
        }
        (VK_0, true, false, false) | (VK_NUMPAD0, true, false, false) => Some(Command::ZoomReset),
        (VK_R, true, false, true) => Some(Command::ToggleReader),
        (VK_F12, false, false, false) | (VK_I, true, true, false) => Some(Command::OpenDevTools),
        _ => None,
    }
}

/// [`command_for`] plus the chords whose setting is on in `gated`. A chord whose setting is off maps
/// to `None`, so callers that swallow mapped keys leave it to the page.
pub fn command_for_gated(
    key: u32,
    ctrl: bool,
    shift: bool,
    alt: bool,
    gated: GatedChords,
) -> Option<Command> {
    command_for(key, ctrl, shift, alt).or(match (key, ctrl, shift, alt) {
        (VK_C, true, true, false) if gated.copy_link => Some(Command::CopyLink),
        (VK_S, true, false, false) if gated.toggle_sidebar => Some(Command::ToggleSidebar),
        _ => None,
    })
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
            (VK_P, true, true, false, Command::NewPrivateTab),
            (VK_M, true, true, false, Command::ShowProfileMenu),
            (VK_K, true, true, false, Command::ShowCommandPalette),
            (VK_N, true, false, false, Command::NewWindow),
            (VK_N, true, true, false, Command::ToggleNotes),
            (VK_D, true, false, false, Command::AddBookmark),
            (VK_E, true, true, false, Command::ShowExtensionsMenu),
            (VK_Z, true, true, false, Command::UndoTabOrganisation),
            (VK_Y, true, true, false, Command::TogglePictureInPicture),
            (VK_OEM_5, true, true, false, Command::ToggleSplit),
            (VK_LEFT, true, true, false, Command::FocusSplitLeft),
            (VK_RIGHT, true, true, false, Command::FocusSplitRight),
            (VK_W, true, true, false, Command::CloseSplitPane),
            (VK_H, true, false, false, Command::ShowHistory),
            (VK_J, true, false, false, Command::ShowDownloads),
            (VK_OEM_PLUS, true, false, false, Command::ZoomIn),
            (VK_OEM_PLUS, true, true, false, Command::ZoomIn),
            (VK_ADD, true, false, false, Command::ZoomIn),
            (VK_OEM_MINUS, true, false, false, Command::ZoomOut),
            (VK_SUBTRACT, true, false, false, Command::ZoomOut),
            (VK_0, true, false, false, Command::ZoomReset),
            (VK_NUMPAD0, true, false, false, Command::ZoomReset),
            (VK_R, true, false, true, Command::ToggleReader),
            (VK_F12, false, false, false, Command::OpenDevTools),
            (VK_I, true, true, false, Command::OpenDevTools),
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
        let table = bindings();
        for (index, (key, ctrl, shift, alt, expected)) in table.iter().copied().enumerate() {
            assert!(
                table[index + 1..].iter().all(|other| {
                    (key, ctrl, shift, alt) != (other.0, other.1, other.2, other.3)
                }),
                "duplicate binding for key {key:#04X} ctrl={ctrl} shift={shift} alt={alt}"
            );
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
            (VK_D, true, true, false),
            (VK_LEFT, false, false, false),
            (VK_F5, false, true, false),
            (VK_9, false, false, false),
            (VK_1, false, false, true),
            (VK_N, true, true, true),
            (VK_R, false, false, true), // Alt+R alone
            (VK_R, true, true, true),   // Ctrl+Shift+Alt+R
            (0x58, true, true, false), // Ctrl+Shift+X is not bound.
            (VK_J, true, true, false),  // Ctrl+Shift+J is not bound.
            (VK_J, false, false, false),
            (VK_J, true, false, true),  // Ctrl+Alt+J
            (0x79, false, true, false), // Shift+F10 is the page's native context menu.
        ] {
            assert_eq!(command_for(key, ctrl, shift, alt), None, "key {key:#04X}");
        }
    }

    #[test]
    fn zoom_chords_follow_chrome_and_never_catch_alt_or_unrelated_shift() {
        for key in [VK_OEM_PLUS, VK_ADD] {
            assert_eq!(command_for(key, true, false, false), Some(Command::ZoomIn));
        }
        // Ctrl+Plus on a US layout is Ctrl+Shift+=.
        assert_eq!(command_for(VK_OEM_PLUS, true, true, false), Some(Command::ZoomIn));
        for key in [VK_OEM_MINUS, VK_SUBTRACT] {
            assert_eq!(command_for(key, true, false, false), Some(Command::ZoomOut));
        }
        for key in [VK_0, VK_NUMPAD0] {
            assert_eq!(command_for(key, true, false, false), Some(Command::ZoomReset));
        }
        for (key, ctrl, shift, alt) in [
            (VK_OEM_PLUS, false, false, false),
            (VK_OEM_PLUS, true, false, true), // AltGr layouts
            (VK_OEM_MINUS, false, false, false),
            (VK_OEM_MINUS, true, true, false),
            (VK_0, false, false, false),
            (VK_0, true, true, false),
            (VK_0, true, false, true),
            (VK_ADD, true, true, false),
        ] {
            assert_eq!(command_for(key, ctrl, shift, alt), None, "key {key:#04X} ctrl={ctrl} shift={shift} alt={alt}");
        }
        assert!(!is_toggle(Command::ZoomIn), "holding Ctrl+Plus keeps zooming");
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

    #[test]
    fn private_tab_took_ctrl_shift_p_and_the_profile_menu_moved_to_ctrl_shift_m() {
        assert_eq!(
            command_for(VK_P, true, true, false),
            Some(Command::NewPrivateTab)
        );
        assert_eq!(
            command_for(VK_M, true, true, false),
            Some(Command::ShowProfileMenu)
        );
        // Neighbouring chords stay unbound, including AltGr (Ctrl+Alt) forms.
        assert_eq!(command_for(VK_P, true, false, false), None);
        assert_eq!(command_for(VK_P, true, true, true), None);
        assert_eq!(command_for(VK_M, true, false, false), None);
    }

    #[test]
    fn ctrl_alt_r_is_the_only_ctrl_alt_chord() {
        assert_eq!(command_for(VK_R, true, false, true), Some(Command::ToggleReader));
        assert!(is_toggle(Command::ToggleReader), "auto-repeat must not flip reader view again");
        for key in 0..=0xFF_u32 {
            for shift in [false, true] {
                let bound = command_for(key, true, shift, true);
                assert_eq!(
                    bound.is_some(),
                    key == VK_R && !shift,
                    "key {key:#04X} shift={shift}"
                );
            }
        }
        // Ctrl+R still reloads and Ctrl+Shift+R still hard-reloads.
        assert_eq!(command_for(VK_R, true, false, false), Some(Command::Reload));
        assert_eq!(command_for(VK_R, true, true, false), Some(Command::HardReload));
    }

    #[test]
    fn dev_tools_opens_on_f12_and_ctrl_shift_i_only() {
        assert_eq!(
            command_for(VK_F12, false, false, false),
            Some(Command::OpenDevTools)
        );
        assert_eq!(
            command_for(VK_I, true, true, false),
            Some(Command::OpenDevTools)
        );
        assert!(
            is_toggle(Command::OpenDevTools),
            "key-repeat must not flap the docked pane open and shut"
        );
        // Neighbouring chords stay unbound: F12 with modifiers and Ctrl+I without Shift reach the page.
        for (key, ctrl, shift, alt) in [
            (VK_F12, true, false, false),  // Ctrl+F12
            (VK_F12, false, true, false),  // Shift+F12
            (VK_I, true, false, false),    // Ctrl+I, italics in editors
            (VK_I, false, false, false),   // I
            (VK_I, true, true, true),      // Ctrl+Shift+Alt+I
        ] {
            assert_eq!(command_for(key, ctrl, shift, alt), None, "key {key:#04X}");
        }
    }

    #[test]
    fn setting_gated_chords_are_unbound_in_the_fixed_table() {
        // command_for never claims them, so nothing that swallows keys with it can eat them.
        assert_eq!(command_for(VK_C, true, true, false), None);
        assert_eq!(command_for(VK_S, true, false, false), None);
    }

    #[test]
    fn copy_link_chord_is_bound_only_while_its_setting_is_on() {
        let on = GatedChords::new(true, false, false);
        let off = GatedChords::default();
        assert_eq!(
            command_for_gated(VK_C, true, true, false, on),
            Some(Command::CopyLink)
        );
        assert_eq!(command_for_gated(VK_C, true, true, false, off), None);
        // Neighbours stay with the page whatever the setting: Ctrl+C, Ctrl+Shift+Alt+C, Shift+C.
        for (ctrl, shift, alt) in [(true, false, false), (true, true, true), (false, true, false)] {
            assert_eq!(command_for_gated(VK_C, ctrl, shift, alt, on), None);
        }
    }

    #[test]
    fn sidebar_chord_needs_its_setting_and_the_vertical_layout() {
        let vertical = GatedChords::new(false, true, true);
        assert_eq!(
            command_for_gated(VK_S, true, false, false, vertical),
            Some(Command::ToggleSidebar)
        );
        // Setting on but a horizontal layout, or vertical layout with the setting off: the page gets it.
        assert_eq!(GatedChords::new(false, true, false), GatedChords::default());
        assert_eq!(GatedChords::new(false, false, true), GatedChords::default());
        assert_eq!(
            command_for_gated(VK_S, true, false, false, GatedChords::default()),
            None
        );
        for (ctrl, shift, alt) in [(true, true, false), (true, false, true), (false, false, false)] {
            assert_eq!(command_for_gated(VK_S, ctrl, shift, alt, vertical), None);
        }
        assert!(is_toggle(Command::ToggleSidebar), "holding Ctrl+S must not flap the sidebar");
    }

    #[test]
    fn gated_lookup_leaves_every_fixed_binding_alone() {
        let all = GatedChords::new(true, true, true);
        for key in 0..=0xFF_u32 {
            for modifiers in 0..8_u8 {
                let (ctrl, shift, alt) =
                    (modifiers & 1 != 0, modifiers & 2 != 0, modifiers & 4 != 0);
                if let Some(fixed) = command_for(key, ctrl, shift, alt) {
                    assert_eq!(command_for_gated(key, ctrl, shift, alt, all), Some(fixed));
                }
            }
        }
    }

    #[test]
    fn productivity_chords_do_not_collide_with_existing_commands() {
        assert_eq!(
            command_for(0x4B, true, true, false),
            Some(Command::ShowCommandPalette)
        );
        assert_eq!(
            command_for(VK_N, true, true, false),
            Some(Command::ToggleNotes)
        );
        assert_eq!(
            command_for(VK_D, true, false, false),
            Some(Command::AddBookmark)
        );
        assert_eq!(
            command_for(VK_E, true, true, false),
            Some(Command::ShowExtensionsMenu)
        );
    }
}
