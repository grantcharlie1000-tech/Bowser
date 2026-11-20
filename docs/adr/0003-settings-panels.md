# Settings profiles, start page, and theme panels

## Status

Accepted. Records behavior already implemented in Bowser.

## Profile commands and keyboard navigation

The Settings page sends profile commands to the Windows host and renders the returned snapshot. Create, rename, change appearance, and delete update the profile data shown on the page. The host acknowledges their success with a Saved reply. Open, guest, and shortcut actions can succeed without an acknowledgment. The page tracks only commands that acknowledge success as pending, so an action without an acknowledgment cannot leave the settings draft waiting indefinitely. Failures still produce an error reply. The native command classification and the UI's profile sender must agree. [Native bridge](../../src/chrome_host/settings.rs) · [Profile sender](../../ui/src/settings-profiles.ts)

The active rename or delete confirmation is tracked by profile ID when a snapshot redraws the list. A delete confirmation focuses **Keep** first, so pressing Enter again preserves the profile. This gives keyboard users a safe default during an irreversible operation.

## Availability and validation

Start page and theme controls that depend on another setting are disabled and hidden during ordinary browsing when they cannot affect the current configuration. Search still finds the matching disabled rows so users can discover them. When the settings file is unreadable, the page keeps the rows visible and disables their controls. The page banner explains the difference.

The new-tab **Blank** option disables the Dials and Widgets controls while leaving Homepage and Startup available. The Settings predicate mirrors the native predicate so availability agrees with the surface Bowser actually shows. Each disabled switch explains why. When that reason is hidden, its ID is also removed from the switch's accessible description because a directly referenced hidden element can still be announced. [Settings predicate](../../ui/src/settings-start.ts) · [Native predicate](../../src/tab_options.rs)

The **If no image** label explains the effect of the stored `auto` title mode. It keeps the title below tiles without a chosen image and shows it on hover for the rest. The Startup choices are labeled **Your tabs**, **Tabs + homepage**, and **Tabs + pages** because a normal startup restores the saved tabs regardless of the selection. A launch that already carries a target skips opening the configured homepage or startup pages. [Start page design](../design/start-page.md)

## Theme commands and files

Theme controls send one `themeopt` assignment per change. The theme settings panel restores a time field to its latest accepted value if the input is malformed or matches the other boundary. Native theme validation also rejects equal boundaries so a schedule has distinct light and dark transitions. [Theme settings panel](../../ui/src/settings-themes.ts) · [Native theme validation](../../src/theme_custom.rs)

The browser page sends named import and export actions while the Windows host opens the file dialogs and operates on the chosen paths. File paths stay within the native host. [Windows host](../../src/win32/settings_page.rs)
