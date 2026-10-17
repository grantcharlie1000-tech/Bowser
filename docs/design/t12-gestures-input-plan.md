# T12 Mouse gestures: input-capture plan (security, for approval before building)

Status: plan only, nothing built. Baseline `origin/vivaldi/wave1-review`. T12 runs in wave 4, after T4
Commands and T6 Layout.

## The problem

Pages are windowed WebView2 controllers. Their HWNDs live in the browser process, so a right-button
drag over a page never reaches Bowser's window procedure. A gesture also has to stop the right button's
context menu, must not let a page start or read gestures, and must not delay any click.

## Options

| Option | Sees the drag | Page can spoof or read it | Context menu | Iframes, PDF, error pages | Cost | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| A. Injected JS in every page (`AddScriptToExecuteOnDocumentCreated` + `postMessage`) | Yes, where it runs | **Yes.** The script runs in the page's own world, so any page can call `chrome.webview.postMessage` with a gesture message and run commands; a nonce in that script is readable. Pages can also `stopPropagation` it away. | Needs `preventDefault` in the page | Script must reach every frame; does not run in the PDF viewer or error pages; breaks on strict-CSP or event-hijacking sites | Turns on the page message channel, which is off today except for the password login message | Rejected |
| B. Composition-controller hosting (`SendMouseInput`) | Yes, first | No | Host decides | All | A rewrite of tab hosting (DirectComposition, cursor, IME, drag and drop, accessibility) and risk to every input path and to cold launch | Rejected |
| **C. Native low-level mouse hook (`WH_MOUSE_LL`) on its own thread, watching only** | Yes, from hardware, before any app | **No.** Pages cannot make OS mouse input, and synthetic input is ignored | Suppressed by WebView2's own `ContextMenuRequested` (`SetHandled(true)`) | All, since it works on screen coordinates | A few integer comparisons per mouse event on a dedicated thread | **Recommended** |

## Recommendation: C. Watch with a low-level hook, decide on the window thread, suppress the menu via WebView2.

**Hook thread.** One `std::thread` per process, started the first time a Bowser window becomes active
while "Allow gestures" is on (never on the cold-launch path). It installs `WH_MOUSE_LL` and runs only a
message loop. The hook procedure does no allocation, locking, logging or I/O. It reads a few atomics
(the foreground Bowser window, the content rect in screen pixels, the trigger mode and the threshold),
updates a small fixed-size path buffer, and `PostMessage`s results to the window. Because the thread
only ever runs the hook, the global hook adds microseconds to system mouse input and never stalls it.
It returns immediately unless all of these hold:

- the foreground window is a Bowser window (`GetForegroundWindow` compared with an atomic set on
  `WM_ACTIVATE`)
- the point is inside that window's content rect (not the chrome)
- the event is not injected (`LLMHF_INJECTED` / `LLMHF_LOWER_IL_INJECTED` are dropped, so neither
  other programs nor test tools can draw gestures)
- the cursor is not clipped (`GetClipCursor` smaller than the virtual screen means the page has pointer
  lock, for example a game)

The hook is removed when gestures are turned off or the last window closes. **No keyboard hook.** The
trigger modifier is read with `GetAsyncKeyState(VK_MENU)` once, when a button goes down. No keystroke is
seen, kept or logged.

**What the page still gets.** The hook watches; it does not swallow the triggering button, so the page
gets its normal `mousedown`, `mousemove` and `mouseup`, as it always has. Pages learn nothing new and
cannot block, start or see a gesture. The one exception is rocker gestures (below).

**The right-click menu.**
- Right press: the hook records the start point.
- Right release after movement under the threshold: no gesture. Nothing is touched, and the page or
  Bowser context menu opens as normal.
- Right release after movement over the threshold: the hook stores a short-lived "suppress the next
  context menu" deadline, in an atomic, *before* Windows delivers the release. A third
  `ContextMenuRequested` handler in `webview2.rs`, beside the split-view and search ones, checks that
  deadline (200 ms) and calls `SetHandled(true)`, so no menu appears. A page's own `contextmenu`
  handler still runs, as it does today for any right-click.
- Shift+F10 and the Menu key never set the deadline, so the keyboard menu is never affected.

**Rocker gestures.**
- Hold right, click left: Back.
- Hold left, click right: Forward.
- For these only, the hook swallows (returns 1) the *second* button's down and up, so the page never
  sees a stray click on a link. The first button is still delivered; its menu is suppressed as above.

**Alt+drag mode** (instead of the right button).
- The hook swallows the left button's down/move/up while Alt is held over the page, so the page does not
  start a selection or download a link. A press with no movement is *not* replayed to the page as an
  Alt+click (`SendInput` replay is fragile), so Alt+click on pages does nothing in this mode. The
  setting's description says so.
- The Alt release that ends the gesture is marked so the "Alt opens the menu" path
  (`AcceleratorKeyPressed`) ignores it.

**Recognition** (window thread, pure Rust, unit-tested). The path is split into strokes whenever the
direction changes for more than the threshold. Each stroke is one of 4 directions, plus the 4 diagonals
if the threshold allows. Strokes are merged, and at most 8 are kept. The threshold is in DIPs (the
sensitivity slider, 10-60 DIP, default 25) multiplied by `GetDpiForWindow` of the window, so a gesture
feels the same at 100 % and 150 % and on either monitor. Drags that take longer than 2 s are dropped.

**Trail.** One owned, click-through popup (`WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE`)
over the content, created on the first gesture, then hidden and reused. It shows a 2 DIP accent-colour
line, updated at most once per frame with `UpdateLayeredWindow`, and the recognised command's label
near the pointer. With `prefers-reduced-motion` it shows no fade. It never takes focus.

**Running the command.** Recognition only posts `WM_GESTURE_RUN` with the gesture's id, and
`run_registered(hwnd, app, id)` runs it on the window thread. A gesture is just another binding: it can
name any command shown in the keyboard editor, or a chain (`chain:<n>`), which then runs through
`commands::chains::schedule` as shortcuts do. A command id that no longer exists is skipped, as for
chains. Nothing runs inside the hook or inside a WebView2 callback.

## Mapping and defaults

Gestures are stored per profile in `gestures.txt` (plain text, like `keybindings.txt`), one line per
gesture: `gesture <strokes> <command-id>`. A guest profile keeps them in memory only.

Defaults, using existing registry ids:

| Gesture | Command |
| --- | --- |
| ← | `back` |
| → | `forward` |
| ↑↓ | `reload` |
| ↓→ | `close-tab` |
| ↓ | `new-tab` |
| ← ↑ | `reopen-tab` |
| ↑ → / ↑ ← | `next-tab` / `prev-tab` |
| ↓ ↑ | `stop` |
| Rocker left / right | `back` / `forward` (built in, not editable rows) |

## Settings (four rows, plus a sub-page)

1. **Allow mouse gestures**: new key `gestures`, default off, so nothing is hooked until the user opts
   in.
2. **Draw with**: right button / Alt + left button.
3. **Sensitivity**: a slider using the existing range control (or buttons, if T9's option is on).
4. **Rocker gestures**: on/off.
5. **Edit gestures…** opens a sub-page:
   - each row shows a stroke icon (from the existing icon pipeline), the command and Edit/Remove
   - a New button records a gesture by drawing it in a pad, or by entering arrows
   - a command picker, the same one the chain editor uses, lists commands and chains
   - a gesture two rows share is reported and can be taken, as in the keyboard editor
   - Restore defaults

Each row is tested UI -> Rust save -> snapshot -> behaviour: the recogniser on recorded point fixtures
at two DPIs, the filter (foreground, rect, injected, clipped cursor), the context-menu deadline, rocker
swallow rules, store round-trip and a missing id, a chain gesture scheduling its steps.

## Security notes (security-audit at the end of T12)

- Pages cannot start, see or block a gesture. There is no page script and no new message channel.
- No keystrokes are captured. Gestures are not logged or kept beyond the current drag, and the path
  buffer is cleared on release.
- Synthetic input is ignored. The hook is off when the setting is off, and only acts while a Bowser
  window is in front.
- Private windows behave the same, and nothing is persisted from them.
