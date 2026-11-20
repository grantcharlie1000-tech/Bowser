# Reconcile Settings controls with native snapshots

## Context

The native host owns Settings persistence. Some controls edit a page-wide draft, while other controls have dedicated commands and snapshots. Results also arrive after browser activity outside Settings and as separate feature snapshots.

## Decision

- Apply native snapshots through setters on the existing controls so updating them preserves focus. Hold the draft while Settings changes await an answer. The native host drains the current command batch and returns a snapshot with the saved state and the batch acknowledgement. Request a fresh snapshot when the window comes back into focus because another window may have changed settings.
- Keep the protection panel's blocking choice in step with the draft's legacy `blockerEnabled` field. The protection panel owns the level command, and its None choice maps to the old off value. The first-run welcome uses the same command as the Settings row.
- Debounce successive tab-number spinner steps because each option update performs a read, merge, and write in Rust. Flush the last queued value on blur, page hide, or a transition to hidden visibility. Keep the value in the field while it is queued, even if a snapshot arrives.
- Apply control availability after the global readability checks. Dependent settings stay disabled when their prerequisites are unavailable, including a centred address field when its toolbar lacks room or the compact address view when Full address is selected. Keep settings that tune an off feature available to search as dimmed rows. For an unreadable settings file, the banner explains why the controls are locked. Flag settings that require a restart so the page communicates when a protection change will take effect.
- Clear a custom accent if the viewer picks a palette. Native rendering gives a custom accent priority over both palettes, so leave the palette choices unselected while one is active. Use the theme's focus colour for an unset accent once the host supplies it.
- Start the Media probe on the first visit to its card. Keep backup file selection in the native dialog, clear password fields immediately after sending, and route outcome messages to the first-run welcome while the page's toast is hidden.

## Consequences

Native persistence and the state reported by the host remain authoritative. The UI can show an in-progress edit while a write settles. A control's effective availability can depend on both the global snapshot and its own setting context.

See [Settings UI](../../ui/src/settings-page.ts), [native Settings host](../../src/win32/settings_page.rs), and [Settings wire format](../../src/chrome_host/settings.rs).
