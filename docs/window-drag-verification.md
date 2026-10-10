# Window dragging verification - 2026-10-10

Changes remain uncommitted. Luna 6 implemented the targeted Win32 changes;
Haiku 5.5 investigated and reviewed the overlay/capture changes and ran targeted
tests. The lead reviewed the final implementation and exercised running windows.
Existing glass-smoothing changes were preserved.

## Diagnosis

Host movement already uses native `WM_NCHITTEST`/`HTCAPTION` and `DefWindowProcW`.
The manual drag state is for tab reordering, not window movement. No position
animation or interpolation was involved.

Three concrete problems contributed to the symptoms:

- `WM_MOVE` invalidated the whole host. The separate owned layered chrome popup
  was repositioned only during its next paint/upload. Rapid baseline runs showed
  overlay skew up to 49 px, making the chrome appear to lag behind the window.
- `WM_CANCELMODE` was consumed without calling `DefWindowProcW`. In the baseline
  cancellation probe, native capture and the move/size loop were still active
  50 ms after cancellation, before the real mouse-up. The capture-change handler
  also attempted to release capture after it had already changed owners.
- Legacy accent-policy Acrylic adds drag latency on this Windows 10 host.
  Exploratory controls on the first overlay fix measured p95 tracking error of
  47 px with Acrylic versus 25 px with native blur. These are bounded samples,
  not a universal compositor benchmark.

## Exact changes

`src/win32/window.rs` now synchronizes the overlay to the client origin on
`WM_MOVE` using `SetWindowPos(SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE)`, without
invalidating or uploading its pixels. The chrome state borrow is released before
calling Win32, and minimized windows are skipped. Size/DPI changes retain their
existing rendering paths.

Capture loss clears tab-drag/pressed state without releasing the new owner's
capture. Cancellation clears that state, releases capture only when Bowser owns
it, restores the normal backdrop, and delegates native cancellation to
`DefWindowProcW`.

`src/win32/glass.rs` selects native blur during the legacy move/size loop and
restores Acrylic at exit or cancellation. The switch is idempotent and happens
only at the loop boundaries, never on individual moves. Theme refreshes respect
the temporary state. Windows 11 system-backdrop behavior, solid fallback,
transparency/high-contrast gates and the debug blur override are preserved.
A small policy regression test checks blur selection and normal restoration.

No new rendering pipeline, capture, allocation per move, position animation or
continuous repaint loop was added. Dirty regions, 150 ms chrome transitions and
reduced-motion behavior are unchanged. WebView2 is not resized on pure moves;
its existing synchronous resize path remains in place for actual size changes.

## Live evidence

Artifacts and the native input probe are in ignored `.context/drag/`. The baseline
is `.context/glass-smooth/bin/after.exe`, which includes the glass work but none
of these drag fixes. The final candidate is `.context/drag/final.exe`, SHA256
`1A89A9FD26ED8EDEE9FDCED41F8FF2976EDD34B608256B17B76C99B053F78366`.

The probe moves the cursor rapidly along a sinusoidal path with the real left
button held, samples native rectangles/capture/move-loop flags, then releases
the button and continues moving the cursor for roughly a second. It checks
foreground activation and native capture before accepting a run, and identifies
the overlay by its host owner. Some starts require posting the native
`WM_NCLBUTTONDOWN` message while the physical button remains down; the movement
itself is entirely handled by Windows. Earlier runs with no active native loop
are excluded. Statistics use nearest-rank p95 and the mean of the two central
values for the median.

- `final-rapid`, `final-repeat1/2/3`: median tracking error 3/1/0/0 px,
  p95 25/25/25/24 px. Capture was zero in the first sampled release frame,
  the native loop had ended, and every released position was identical.
- `final-cancel`: capture was zero and the loop ended before physical release,
  50 ms after `WM_CANCELMODE`. `final-escape` also ended the loop and stayed still.
- `final-resize`: native right-edge hit test returned `HTRIGHT`, width changed
  while held and stayed constant after release; p95 edge error 24 px.
- `final-actions-verified`: real Win+Left/Right produced 960 x 1080 windows at
  x=0/x=960; physical title-bar double-click maximized the window. Subsequent
  rapid dragging and release passed (p95 19 px).
- `final-page-verified`: `navigation_completed` was logged before dragging a
  live WebView2 page. Median error 0 px, p95 25 px; release was stable.
- `final-loaded-resize`: the loaded WebView2 page also resized through the native
  edge loop (median 0 px, p95 25 px), with constant width and no capture after
  release.
- `final-solid`: the forced-solid fallback also ended capture and movement on
  release, without enabling glass.

The final results contain an outlier: `final-paired` p95 was 70 px, although its
median was 6 px and release was stable. The accompanying baseline was 12 px
median / 59 px p95 and moved another 72 px after its first post-release sample,
with capture remaining held until about 51 ms after the last held sample.
The forced-solid run also had a 72 px p95 spike. Therefore the changes remove
the demonstrated application defects, but do not prove universally stall-free
OS/compositor/input scheduling. Repeated runs generally improved tracking;
the worst-run p95 did not improve. Runs overlapping the release test compilation
(`before-repeat1/2`, `final-repeat1/2`) are not clean paired performance evidence.

Overlay skew p95 fell from 25 px in `fast-acrylic-v2` to 0 px in most final
empty-window runs. The loaded-page run had 13 px p95 skew. Sequential reads of
host and popup rectangles can straddle a native move, so isolated nonzero values
cannot establish persistent visual skew. No presentation-frame trace was taken.
The undocumented accent-query API returned unsupported here; backdrop transition
verification rests on implementation review, the policy test and the blur control,
not a successful native query of the compositor state.

## Quality gates and limitations

- `cargo fmt -- --check`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo test --locked --quiet`: 241 passed, 3 artifact/probe tests ignored.
- `cargo test --locked --release --quiet --target-dir target/glass-smooth`:
  241 passed, same 3 ignored.
- Debug and release builds passed; release used the separate glass-smooth output
  directory to avoid replacing the user's running executable.
- Haiku's overlay, glass rim, memory-policy and resize tests passed, as did
  `benchmarks/test-measurements.ps1`.
- `git diff --check`: passed.

The existing glass report records the raster performance/memory/idle measurements;
this change does not alter its pixel renderer. No new idle timer was introduced.
Drag sampling measures cursor-to-native-rectangle error, not full-frame paint
duration or display latency. Intermittent stalls remain unresolved.

Live testing used one Windows 10 monitor at 1920 x 1080 / 96 DPI. Moving between
monitors and Windows 11 compositor behavior could not be exercised. The existing
benchmark startup-navigation panic was avoided by submitting through the EDIT
after startup; it was not refactored.
