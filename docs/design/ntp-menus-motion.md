# Tab drop motion and anchored composers

Owner: icon and animation design. Status: design note, not implemented. Scope: the tab drag drop
animation (`ui/src/tabs.ts`, `ui/src/styles/tabs.css`), and the three composers that replace the
centred name dialog for New workspace, Add web app and Edit web app.

Motion follows the micro-interaction rules: 100–250ms, transform and opacity only, ease-out on
enter and ease-in on exit, and no travel under `prefers-reduced-motion`. The composers follow
rule/inline-before-modal (the menu is the control) and rule/keyboard-complete-flow (the keyboard
can finish every task).

---

## 1. Tab drop: what goes wrong today

Code read: `endDrag` (tabs.ts:491), `onPointerMove` (tabs.ts:569), `render` FLIP block
(tabs.ts:361–381 "first", 440–454 "last, invert, play"), `setShift` (tabs.ts:235), `flush`
(tabs.ts:292), `.strip .tab` transition (tabs.css:42), `.strip.settle .tab` (tabs.css:97),
`.strip .tab.dragging` (tabs.css:100), the reduced-motion rule in base.css:44.

The suspected cause in the ruling is right in outline but misses the mechanism that matters most.
**Two separate faults combine, and the second one fires on every reorder, even with no stale
render.**

**Fault A: the dragged tab's transform is not tracked by `shift`.** `onPointerMove` writes
`translateX(dx) scale(1.02)` straight to `style.transform` (tabs.ts:581). It never updates
`d.node.shift`, which stays 0. `render` resets positions with `setShift(n, 0)` (tabs.ts:372, 375).
`setShift` returns early when `n.shift === 0` (tabs.ts:236), so the dragged tab keeps
`translateX(dx)` through the whole FLIP. When the host reorder arrives:

1. "First" reads the dragged tab at `oldHome + dx`. That reading is correct.
2. `place()` moves the node. The leftover transform is still on it, so "last" reads
   `newHome + dx`.
3. The invert is `(oldHome + dx) − (newHome + dx) = oldHome − newHome`. Setting
   `translateX(oldHome − newHome)` with transitions off puts the tab **back at its origin slot**
   for one frame. Clearing the transform then slides it from the origin to the drop slot.

This is the user's "jumps back to where the drag started, then moves to the drop slot". It happens
even when nothing else renders between the drop and the reorder.

**Fault B: a stale render runs during settling.** `flush` renders whenever `dirty` is set, and
`dirty` was set by any `tabs`, `fav` or `pair` message during the drag (tabs.ts:786–800; a loading
tab sends these often). Before the reorder arrives, the next frame renders the *old* order.
`settling` forces `animate` true (tabs.ts:364), and the cleanup at tabs.ts:453 clears **every**
transform, including the dragged tab's `translateX(dx)`. With the transition on (tabs.css:42),
the tab eases back toward its origin while the neighbours slide home. `settling = false` and
`clearTimeout(settleTimer)` (tabs.ts:379–380) also cancel the refusal fallback. The reorder then
lands and makes a second FLIP to the destination. That is two motions, and the first goes the
wrong way.

**The "zoom".** `endDrag` replaces `translateX(dx) scale(1.02)` with `translateX(dx)`
(tabs.ts:521) while transitions are on, so the scale drops 1.02→1 over 167ms as its own motion.
That motion starts before, and separately from, the travel. `.dragging` is also removed at once
(tabs.ts:499), so `z-index: 5` drops. The returning tab can then pass *under* its neighbours.

## 2. Tab drop: the corrected animation

### State flags

| Flag | Change |
| --- | --- |
| `settling: boolean` (tabs.ts:271) | **Remove.** Replace with `landing`. |
| `settleTimer` (tabs.ts:272) | **Remove.** Move it into `landing.timer`. |
| `landing: { node: TabNode; signature: string; timer: number } \| null` | **Add.** Set on a committed drop. `signature` is the layout signature at the moment of the drop. |
| `TabNode.shift` | **Keep, and make it honest.** `onPointerMove` sets `d.node.shift = dx` whenever it writes the transform, so `setShift(n, 0)` clears the dragged tab like any other. |
| `.landing` class (tabs.css) | **Add.** `.strip .tab.landing { z-index: 5; }`. It keeps the tab above its neighbours until it lands. Its shadow returns to the tab's own (`--bw-sh-tip` on the active tab) through the existing `box-shadow` transition. |
| `.dragging` | Keep it for the live drag only. On release, swap it for `.landing`. Never drop both in the same frame. |

### Timing

Use one motion, one curve and one duration for everything: transform `--bw-dur-base` (167ms) with
`--bw-ease-out` (`cubic-bezier(0, 0, 0, 1)`), which is the transition `.strip .tab` already has. The
shadow uses the same 167ms ease-out transition (tabs.css:103). Scale 1.02 is **not** a separate
step. It rides inside the landing transform and reaches 1 at the same instant the tab reaches its
slot.

### Steps

**A. Pointer-up with a committed move** (`endDrag(true)`, `dest !== from`, not split-out):

1. Send `tabmove` (unchanged).
2. Replace `.dragging` with `.landing` on the dragged node.
3. **Do not touch any transform.** The tab stays at `translateX(dx) scale(1.02)` and the neighbours
   keep their shifts. Nothing moves in this frame.
4. `landing = { node, signature, timer: setTimeout(refuse, 400) }`.
5. `schedule()` may stay: deferred title or favicon updates can render, under rule B.

**B. `render` while `landing` is set and `nextSignature === landing.signature`** (the stale render):
update only the content (titles, favicons, active state). Skip both FLIP branches, do not call
`setShift`, and do not touch `landing`. Nothing moves. This removes fault B.

**C. `render` while `landing` is set and the signature changed** (the host reorder, or any other
change to the layout):

1. Add `.settle` (transitions off).
2. For every visible node, capture `prev = n.shift`. Then call `setShift(n, 0)` on all of them in
   one pass. This now clears the dragged tab too, which removes fault A.
3. Read `home = getBoundingClientRect().left` for each node (one forced layout). Then
   `first = home + prev`. Use `prev` arithmetic rather than reading a transformed rect, because the
   1.02 scale moves the measured left edge by about 1% of half the tab width.
4. Run `place()` and the ghost handling as today.
5. For each node, read `last = getBoundingClientRect().left` and compute `invert = first − last`.
   Write `translateX(invert)`. For `landing.node`, write `translateX(invert) scale(1.02)`, so the
   lift continues unbroken from the drop.
6. Run `void strip.offsetWidth`, remove `.settle`, and clear every transform (tabs.ts:452–453 as
   today). CSS plays one 167ms ease-out: the dropped tab travels from the pointer's release point
   to its slot while it scales 1.02→1. The neighbours close up in the same frames.
7. `clearTimeout(landing.timer)`. Remove `.landing` on the node's `transitionend` for `transform`,
   with a 250ms fallback timer. Then set `landing = null`.

**D. The host never reorders** (the `refuse` timer fires at 400ms with `landing` still set): call
`setShift(n, 0)` on every node, including the dropped tab. This is **one** 167ms ease-out back to
the origin, with scale 1.02→1 inside it. Remove `.landing` after the transition as in C7, then set
`landing = null`. If a reorder does arrive after the timeout, it is an ordinary layout change and
gets one ordinary FLIP. Nothing replays the drop.

**E. A cancelled drag** (Escape via `onDragKey`, `pointercancel`, `lostpointercapture`, `blur`, or
`dest === from`): do the same as D immediately, with no timer. `.dragging` → `.landing`, and one
167ms ease-out home.

**F. Split-out drop** (tabs.ts:504–516): unchanged, except that it also zeroes `d.node.shift`.

### Reduced motion, and strips with more than `FLIP_MAX_TABS`

- During the drag, write `translateX(dx)` only, with **no `scale(1.02)`**. The lift reads through
  the smaller `--bw-sh-tip` shadow that tabs.css:375 already sets. The tab following the pointer is
  direct manipulation, so it stays.
- In A, nothing changes: the tab stays where it was released.
- In C, add `.settle`, clear all shifts, run `place()`, and remove `.settle`. There is no invert:
  the tab **appears** in its slot. That is the existing `else if (settling)` branch
  (tabs.ts:373–377), now keyed on `landing`.
- In D and E, snap back under `.settle`. base.css:44 already limits `transition-property` to
  opacity, so the transforms snap regardless. `.settle` makes the result deterministic.

### Verification (headless only)

Record the dropped tab's `getBoundingClientRect().left` every frame from pointer-up until
`landing === null`, in four cases:

- (a) reorder within 1 frame;
- (b) a `tabs` message before the reorder;
- (c) refused move;
- (d) reduced motion.

In (a) and (b), the left edge must move monotonically from the release point to the slot, and it
must never pass the origin. In (c), the movement is monotonic back to the origin. In (d), there are
no intermediate values. Scale must be 1 at the frame where travel ends.

---

## 3. Composers: New workspace, Add web app, Edit web app

### What goes away

- `RailKey::AddWorkspace` in `activate_rail` (window.rs:3801) calls `App::workspace_create`
  **before** prompting, then renames through `profile_rename::prompt` (window.rs:3804–3806). Today,
  Cancel leaves a "Workspace N" behind. With the composer, the workspace is created **only** when
  the user confirms, with the confirmed name. Escape or click-away creates nothing.
- `profile_rename`'s HTML dialog (`HTML_WIDTH` 400 × `HTML_HEIGHT` 200, profile_rename.rs:53–54;
  `rename.ts` with Cancel/Save in `.bw-dialog-actions`; `.rename-dialog` in dialog.css) is no
  longer used for these three flows.
- `RailKey::AddApp` (window.rs:3799) opens the Customize panel, and `edit_rail_app`
  (window.rs:3848) opens Customize focused on the app. Both open the composer instead.

### Host and anchoring

- The composer is a popup page on the **menu popup host**, not a dialog window. It sits on the same
  window, surface and size reporting (`sendPopupSize`, so Rust caps, flips and clamps to the work
  area), with a new payload kind next to `menu`: `composer:workspace-new`, `composer:app-add`,
  `composer:app-edit:<id>`.
- It is anchored exactly like `show_rail_context_menu` (window.rs:3997):
  `screen_point(hwnd, rail_menu_anchor(pointer, layout::left_rail_inset(dpi)))` with
  `menu::Placement::BelowLeft`. `rail_menu_anchor` (window.rs:3985) already keeps the composer to
  the right of the rail.
  - Rail **Add workspace** and **Add web app**, clicked: the anchor is the click point.
  - The same buttons activated **from the keyboard**: the anchor is `(rail_right, button top)`.
  - The rail app menu's **Edit…** row (window.rs:4049): the anchor is the `point` the menu already
    used. The composer opens where the menu was.
  - **Customize panel**: the panel's New workspace and Add web app buttons, each app's `edit` icon
    button (customize.ts:354), and a preset pick (customize.ts:75) send their own screen
    rectangle. The composer opens below-left of that button, and a preset opens it prefilled. The
    inline `createWorkspace` form (customize.ts:51), `appForm` (customize.ts:64) and the
    `cust-editor` form (customize.ts:360) are removed. There is no second design.

### Anatomy

The composer uses the menu surface as it is: `#app.html-menu` with `data-kind="composer"`. That
gives the 8px `--bw-r-overlay`, the 1px `--bw-hairline` border, the opaque overlay fill and
`--bw-sh-flyout` from menu.css. Root padding is `--bw-space-1` (4px).

```
┌──────────────────────────────────────┐  width 280px fixed (inside menu min 200 / max 360)
│ New workspace                        │  .html-menu-section heading (Caption strong, --bw-text-2)
│ [ws+] [Workspace 3______________]    │  field row 28px
│       Use at most 40 characters.     │  error line (only when invalid)
│──────────────────────────────────────│  .html-menu-separator (1px, 4px margins)
│ [ + ] Create workspace        Enter  │  .html-menu-row 28px, shortcut column
└──────────────────────────────────────┘
```

Add web app has two field rows (Name, then Address) and the confirm row **Add web app**. Edit web
app has the same two rows prefilled, and the confirm row **Save web app**.

**Heading.** `.html-menu-section` showing "New workspace", "Add web app" or "Edit web app". Its
`id` is the dialog's `aria-labelledby`.

**Field row.** It is 28px tall (`--menu-item-h`) and uses the row geometry: `0 var(--bw-space-2)`
padding, a 16px `.html-menu-lead` glyph column, then a 12px gap (`--menu-icon-gap`). The input
fills the rest of the row and keeps the `.bw-input` look (1px `--bw-ctl-edge`, 4px
`--bw-r-control`, `--bw-fill`, its own focus ring), but its height is 28px, not 32px. Field rows
are 4px apart (`--bw-space-1`). The input's edge lines up with the label column of ordinary menu
rows.

**Confirm row.** A real `.html-menu-row` button: 28px, 4px radius, the menu hover and selection
tint, and the keyboard focus ring from menu.css. Its label is a verb ("Create workspace", "Add web
app", "Save web app"). The `.html-menu-shortcut` column shows "Enter". There is **no Cancel row
and no button pair**: Escape and click-away cancel, as they do for a menu.

**Glyphs.** All are existing 16px, currentColor, single-grid icons from `icons/menu` and
`icons/extra`, already in `ICON_NAMES`. **No new SVG is added**, because every row already has a
glyph that fits.

| Row | Glyph | Already used for |
| --- | --- | --- |
| Workspace name | `workspace-add` | the rail's add-workspace mark |
| Web app name | `app-tile` | rail app menu "Open" |
| Web app address | `link` | tab menu "Copy URL" |
| Create workspace / Add web app | `plus` | "New workspace", "New tab" |
| Save web app | `edit` | rail app menu "Edit…" (same action family) |

`check` is not used as a lead glyph: menu-icons.md reserves checks for the trailing column.

**Values.**

- New workspace prefills `Workspace {n+1}` (the name window.rs:3803 computes today), fully
  selected. Enter alone creates it, and typing replaces it.
- Add web app opens empty, with the placeholders `Name` and `https://example.com/` (the same
  placeholders as customize.ts:65–66).
- Edit opens with the saved values. The first field is focused and selected.

**Accessible names.** The root is `role="dialog"`, `aria-modal="true"`, with `aria-labelledby` the
heading. A menu cannot contain text fields. The inputs have `aria-label` "Workspace name", "Web
app name" and "Web app address", plus `aria-describedby` pointing to their error line. The glyphs
are `aria-hidden`.

**Error text.**

- Placement: directly under the field it concerns, starting where the input's text starts. It uses
  Caption size and `--bw-text` with `--bw-weight-strong` (the same treatment as
  `.rename-message.is-error`), and the field gets `aria-invalid="true"`. The line is
  `aria-live="polite"`.
- Content: local checks run first (`localRenameError` rules for names, an http(s) URL for
  addresses). Rust validates again, as `profile_rename::html_submit` does today, and returns the
  error to the page. The popup stays open, focus returns to the failing field, and its text is
  selected.
- Size: the line exists only while there is an error. The window resizes once, with no animation,
  through `sendPopupSize`. Only the text fades in, opacity 0→1 over `--bw-dur-fast` (83ms).

### Keyboard

- The composer opens with focus in the first field.
- Tab and Shift+Tab cycle through the fields and the confirm row, and stay inside the composer.
- **Enter in any field submits**, except while an IME is composing (`isComposing`). Enter or Space
  on the confirm row also submits.
- **Escape dismisses**, with no create and no change.
- Clicking away (popup deactivation) dismisses the same way.
- The confirm row is never disabled. An invalid submit shows the error and focuses the field, so
  the user learns why it failed instead of facing a dead button.

### Motion

| Phase | What | Duration and curve |
| --- | --- | --- |
| Enter | The menu's own `is-entering`: root opacity 0→1, and content `translateY(-4px)`→0 | 83ms `--bw-ease-fade`; 167ms `--bw-ease-out` |
| Exit on Escape, or after Rust accepts | Root opacity 1→0, then the page sends dismiss or confirm | 100ms, ease-in `cubic-bezier(0.4, 0, 1, 1)`; add it as `--bw-ease-in` in tokens.css (no ease-in token exists yet) |
| Exit on click-away | Instant hide, as a menu does | none |
| Error line | Opacity 0→1 | 83ms linear |
| Reduced motion | Enter is the fade only, with no `translateY` (menu.css already sets `animation: none` under reduce). Exit is instant. | ≤83ms |

Enter starts when the size is reported (`measure()` in menu-view.ts), so the motion begins when the
window is shown. Input is never blocked during either animation. The exit on confirm waits for
Rust's verdict, so an error never flashes on a composer that is already fading out.

---

## 4. Do not

- Do not open a window-centred dialog, a Cancel/Create button pair, or `profile_rename::prompt` for
  these three flows.
- Do not create the workspace before the user confirms.
- Do not use a scale bounce on drop: no overshoot and no separate scale step. Scale 1.02→1 moves
  only inside the single landing transform.
- Do not make a second trip through the origin. A render with an unchanged signature never touches
  transforms while `landing` is set.
- Do not animate layout: no transition on `width`, `height`, `left` or `top`, for the tab strip or
  for the composer window. Only `transform` and `opacity` animate.
- Do not add a new glyph, a second colour, or a filled icon. Every composer row uses the existing
  16px currentColor set.
- Do not keep a second add or edit form inside Customize.
