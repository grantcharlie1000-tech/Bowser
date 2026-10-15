# Tab drag: lifting a tab off the strip

Status: implemented (HTML strip, vertical sidebar, native ghost). Owner: tabs lane.
Code: `ui/src/tab-drag-logic.ts` (pure), `ui/src/tab-drag-protocol.ts` (wire), `ui/src/tab-drag.ts`
(page session), `ui/src/tabs.ts` and `ui/src/sidebar.ts` (drag code), `src/chrome_host/tab_drag.rs`
(wire types, geometry), `src/win32/tab_ghost.rs` (the native ghost window).
Tests: `ui/test/tab-drag.mjs`, `cargo test --lib tab_drag tab_ghost`.

## What the reference does (Chromium / Helium)

Helium is a Chromium fork, and none of its patches touch tab dragging. It behaves exactly like
Chromium's `TabDragController`
(`chrome/browser/ui/views/tabs/dragging/tab_drag_controller.cc`, `dragging_tabs_session.cc`).

- **Start.** A press becomes a drag once the pointer has moved more than `kMinimumDragDistance = 10`
  DIP (Euclidean distance).
- **Attached.** While attached, the dragged tab moves only along the strip (`GetAttachedDragPoint`
  clamps x and pins y to 0). It is reparented above the other tabs. The neighbours animate to their
  new slots through the container's bounds animator, which runs 200 ms through
  `RichAnimationDuration` so it respects reduced motion. Reordering has hysteresis: the model is
  only reordered after the pointer moves `kHorizontalMoveThreshold = 16` DIP (scaled by tab width).
- **Detach.** The tab detaches when the pointer leaves the strip's drag area. That area is the strip
  rect grown vertically by `kVerticalDetachMagnetism = 15` DIP (50 for touch), and not grown
  horizontally. On Windows a detached tab becomes a real new browser window, moved by a nested move
  loop. The window re-attaches when the pointer reaches any strip's drag area again.
- **States.** `kNotStarted`, `kDraggingTabs`, `kDraggingWindow`, `kWaitingToDragTabs`,
  `kWaitingToExitRunLoop`, `kStopped` and others.
- **Cancel.** Escape ends the drag as `kCanceled`, and `RevertDrag()` puts every tab back at its
  source index, group and pin state. Any other key completes the drag.
- **Release.** On release the dragged views animate to their ideal bounds (`StoppedDragging`).
- **Pins and groups.** Pinned tabs stay among the pinned tabs. Group membership follows the
  neighbours, with a quarter-tab "lean" buffer. Collapsed groups are not joined.

The "physically picked up" feeling comes from the tab leaving the row and following the pointer
freely once detached, while the strip closes up behind it. We copy that model. The window it would
become is drawn as a lightweight native ghost instead. Dragging out into a new window is a future
step (see the end of this note).

## The constraint

The HTML strip is a WebView2 exactly one row tall, and the sidebar is one column wide. Pointer
capture keeps events arriving when the pointer is outside, but nothing the page paints outside its
own bounds is visible. A tab that "comes off the strip" therefore has to be drawn by something else.

| Option | Verdict |
| --- | --- |
| A. Native top-level layered ghost window (`WS_EX_LAYERED \| TRANSPARENT \| NOACTIVATE \| TOOLWINDOW \| TOPMOST`, per-pixel alpha via `UpdateLayeredWindow`) | **Chosen.** Self-contained (one new module, three small hooks). Moving it costs one `UpdateLayeredWindow` with no pixels. Works in screen coordinates, so it is right over the toolbar, the page, DevTools and side panels, and across monitors. Click-through, so capture and hit-testing are untouched. Pattern already proven in `corner_fringe.rs`. |
| B. Paint it in the chrome overlay | Rejected. Every frame would go through the whole chrome paint (`window.rs` / `paint.rs`, which other lanes are editing). The overlay also ends at the window edge. |
| C. Grow the strip's WebView2 during a drag | Rejected. Airspace and z-order fights with the page WebView2, a resize per drag, and a visible re-layout. |

**The ghost carries the whole drag.** The page's own copy is hidden from the first frame after Rust
says the ghost is up, so the tab separates from the strip at once. Where the copy was is the gap.
Rust paints the ghost itself (a pure rasteriser plus GDI text), mirroring the CSS geometry. It does
not reuse `paint.rs`'s tab painter, which needs a whole `ChromeView`.

## States

```
             press                          move >= 4 px (either axis)
  Idle ───────────────▶ Pressed ─────────────────────────────▶ Attached ◀──────┐
                          │ release: click                        │  pointer > 24 px outside   │ back within 12 px
                          ▼                                       ▼  the band                  │ (hysteresis)
                        select                                 Floating ───────────────────────┘
                                                                  │ (over the page: split zone preview)
  Attached + release ─▶ Drop (reorder or settle) ─▶ ghost glides to the slot ─▶ land ─▶ Idle
  Floating + release over the page ─▶ split drop ─▶ ghost fades ─▶ land ─▶ Idle
  Floating + release elsewhere, Escape, pointercancel, blur ─▶ Cancel ─▶ ghost glides home ─▶ land
```

- **Band.** The horizontal strip detaches only through its top and bottom edges. Beyond its ends
  is outside the window, so it stays attached and the slot clamps. The sidebar detaches only through
  its right edge (towards the page). Above and below the sidebar it stays attached and auto-scrolls.
- **Thresholds.** Lift at 4 px. Detach at 24 CSS px outside the band, re-attach within 12 px.
  Chromium uses 10 and 15. Ours are larger because the toolbar sits between the strip and the page.
  Re-attach at 12 or more keeps "floating below the strip" identical to split-logic's `overPage`
  (`DRAG_OUT_PX = 12`), so split `over`/`drop`/`end` messages are unchanged: they are simply sent
  while floating.
- **Attached.** The slot comes from the pointer (`dropIndex` over home centres), and the existing
  pin, group and collapsed-group rules and edge auto-scroll still apply. Neighbours open the gap
  with exact offsets (`gapShifts`: the held tab's width plus the 4 px gap). The hidden copy sits in
  the gap, so the release target is known.
- **Floating.** The gap closes: the tabs after it shift left by its width plus the gap, and sidebar
  rows and tiles reflow up. Over the page, the split drop zone runs as before.
- **Drop.** `tabmove` as before. The ghost glides into the slot. Once the host's reorder has laid
  the slot out, the page re-aims the glide with a second `end`/`settle`.
- **Split.** `split drop` as before. If a zone was showing, the ghost fades where it is. If none
  was (Rust asks `split_ui::drop_zone_showing` before the queued drop runs), the ghost glides home
  instead, because `end`/`drop` carries the home slot.
- **When the page offers a split** (`src/drop_intent.rs`). A zone shows only within 25% of its edge,
  and the middle of the page is neutral. The edge the tab came in through (top from the strip, left
  from the sidebar) is offered only after the pointer has gone past that edge's band once. Corners
  are 15% squares, and only over a shown split. A shown zone holds 4% further. Before this, every
  point of the page had a zone, so pulling a tab off the strip showed "Open at the top" (or a
  quarter) from its first pixel over the page.
- **Cancel.** Shifts go back to 0 (the gap re-opens at home), and the ghost glides back.

## Visuals

| | Value |
| --- | --- |
| Ghost body | The active island made opaque: dark is text 24% over the window, light is white 92%. Hairline is text 22%. Private uses a violet 75% edge plus a 2 px violet bar (under a tab, left of a row). |
| Content | 16 px favicon at the CSS inset (or the globe glyph), title in the system UI font (ellipsised), close X when the tab showed one. Pinned tabs and tiles show the favicon only. |
| Lift | Scale 1.03 about the grab point, so the grabbed pixel stays under the cursor. |
| Shadow | Lifted: 0 6px 16px black 26% plus a 1 px ambient edge at 16% (stronger than `--bw-sh-flyout`, because it is seen over arbitrary pages). Rest: `--bw-sh-tip`. |
| Opacity | 255 attached, 235 while floating. |
| Gap | The copy is `opacity: 0` (`.lifted`) and keeps its box. Neighbours slide on the strip's own 167 ms ease-out transform transition. |

## Motion

| Moment | Duration | Easing | Who |
| --- | --- | --- | --- |
| Neighbours open or close the gap | 167 ms | `--bw-ease-out` cubic-bezier(0,0,0,1) | CSS transform |
| Ghost glide to slot or home | 200 ms (`GLIDE_MS`) | same curve; scale 1.03 to 1 and shadow lifted to rest along it | Rust, 16 ms `WM_TIMER` steps |
| Ghost fade on split drop | 120 ms | linear | Rust |
| Hand-off at landing | Rust sends `land`, the page shows its copy, the ghost stays 34 ms over it, then hides | none | both |

Hand-off overlap avoids a frame with neither the ghost nor the copy on screen.

**Reduced motion.** This covers `prefers-reduced-motion` and the Windows "show animations" setting
(`SPI_GETCLIENTAREAANIMATION`). The page sends `end`/`none` and Rust skips the glide and fade: the
ghost lands at once. The lift scale stays (it is static) and `--bw-dur-base` is 0, so neighbours
jump.

## Robustness

Every exit path ends the drag: `pointerup`, `pointercancel`, `lostpointercapture`, window `blur`
(Alt+Tab) and Escape. Each path either glides home or lands the ghost.

Rust's watchdog (`tab_drag::watchdog`, on the ghost's 16 ms timer) takes a held ghost down and
sends `gone` in three cases:

- no message for 2 s (the page sends a keepalive every 500 ms);
- the primary mouse button has been up for 300 ms with no `end` (a lost release);
- the window is no longer the foreground, visible, non-minimised one.

On `gone` mid-drag the page shows its copy and keeps dragging it in place (the old behaviour).

The page never waits forever either. After a release its copy reappears after
`GHOST_REVEAL_FALLBACK_MS` (600 ms) even if `land` never comes. A new press abandons any landing at
once (the existing `snapHome` path). The ghost is only for **mouse** drags: Rust follows the system
cursor, which touch and pen do not reliably move, so they keep the in-place drag.

**Private tabs.** The ghost is a live window only. It is never written to a thumbnail or a file.
For a private tab it is marked `WDA_EXCLUDEFROMCAPTURE` (Windows 10 2004 and later; it is ignored
where unsupported).

**DPI.** Everything is in physical pixels at the source window's DPI (96 to 192 and above, tested at
100 / 150 / 200%). Moved to a monitor with another DPI, the ghost keeps the source size, as Chromium
does.

## Accessibility

A polite live region (outside the reconciled strip DOM) announces each moment:

- "Picked up X. Move to reorder, drag onto the page to split, Escape to cancel."
- leaving and re-joining the strip
- "X moved to position N of M."
- "Move cancelled."

Keyboard reordering (Ctrl+Shift+Left/Right) is unchanged.

## Protocol

See docs/html-chrome-protocol.md, "Tab drag ghost". Page to Rust is `tabdrag`
(`start` / `move` / `end`). Rust to page is `tabghost` (`shown` / `land` / `gone`). Messages stay
within the six-field cap by packing `r` = "w,h,gx,gy,c" and `p` = "x,y".

## Future: drag out into a new window

Not built. The pieces are ready for it:

- the ghost is a top-level window in screen coordinates;
- the page already reports "floating";
- `end` could gain an `m:"window"` mode that asks Rust to open a window at the cursor with that tab
  (Chromium's `DetachIntoNewBrowserAndRunMoveLoop`).

Re-attaching to another window's strip would need a window finder under the cursor, as Chromium's
`GetDragTargetForPoint` has.
