# Background media suspension guard

## Code-level diagnosis

Switching tabs still hides the old WebView2 controller with `SetIsVisible(false)`.
That is normal tab visibility behavior and is not established here as the cause
of an immediate site-specific pause. No JavaScript visibility spoofing, autoplay
bypass, forced `play()`, or globally visible background controllers are added.

For immediate silence, also inspect the existing `tabs_mute_mode` preference:
`active` intentionally mutes background tabs and `prioritise` can mute them while
the active tab is audible. The default is `all`. Muting is not a paused media
timeline; this patch does not override the user's preference.

The automatic 10-second (efficiency) / 30-second suspension sweep previously
called `TrySuspend` without the media/unsaved-input probe used by manual
hibernation and discard. That bypass is a confirmed code defect: the sweep could
ask WebView2 to freeze page scheduling while a media element was buffering,
playing silently, or otherwise not protected by the runtime's audible heuristic.
The DOM probe also required `readyState > 2`, incorrectly excluding play intent
while loading or buffering.

The sweep now:

- requires a completed current document and at most one outstanding check;
- refuses native audible output (including muted output and Web Audio), and
  fails closed if that query is unsupported or fails;
- resumes and runs the existing read-only media, PiP, capture, frame and draft
  probe before asking to suspend;
- protects unpaused, unended media regardless of buffer readiness;
- binds the asynchronous verdict to the controller, document and hidden interval;
- checks the fence again after the final native audio call;
- records suspension only after a successful runtime result and resumes on a
  stale/failed completion rather than leaving a newly active document frozen.

Memory-target trimming, ordinary visibility, and explicitly requested manual
hibernation otherwise remain unchanged. The application's `suspended` marker is
still an optimistic scheduling hint, not evidence that closing a controller is
safe. The independent fresh probe before discard remains mandatory.

## Verification

The Node suite `ui/test/window-safety.mjs` evaluates the actual embedded sleep
script. It covers readiness 0–4, muted/zero-volume media, same-origin frames,
paused/ended media, live/ended capture, PiP, inaccessible frames and dirty drafts.
The source contracts ensure the automatic sweep cannot bypass the probe.

Rust unit tests cover stale hidden intervals, active tabs, split panes, document
navigation/completion, controller replacement, close, reentrant native audio
queries, and completion dispatched while the core is already borrowed. Run these on Windows:

```powershell
cargo test --locked automatic_suspend
cargo test --locked
cargo test --locked --features html-chrome-default
cargo clippy --locked --features html-chrome-default -- -D warnings
```

These tests are not proof of end-to-end playback on Windows. Before promotion,
use a Windows/WebView2 build with a disposable profile and verify:

1. Play audible audio/video, switch to another tab, and wait over 30 seconds.
2. Repeat with efficiency mode enabled and wait over 10 seconds.
3. Repeat with muted video, buffering media, a same-origin iframe player and PiP.
4. Switch back and forth repeatedly and navigate/close during a pending check.
5. Confirm playback/time progression without replaying or reloading the tab.
6. Pause/end media and verify idle pages can still suspend and resume normally.

Immediate pause on a particular third-party site, Windows audio-device output,
minimize/restore, cross-origin players, and long-duration live streaming still
require native runtime verification. No release, deployment or stable pin change
is part of this fix.

## Hosted muted-media regression

`Native muted background media probe` runs the exact ignored native test on a
fresh Windows runner, pinned to the PR head rather than the synthetic merge:

```powershell
cargo test --locked --lib --features html-chrome-default engine::webview2::background_media_live::windows_media_hidden_sweep_sleep_off_and_on -- --exact --ignored --nocapture --test-threads=1
```

It uses a hidden owned window, a new disposable profile, loopback-only WAV media
and no foreground input. For each actual `background_policy` sleep-enabled value,
it confirms the native WebView is muted before unmuting the HTML audio element,
starts playback with a real native audio signal, then switches to a lazy blank tab and waits 12
seconds, runs the real efficiency sweep, and requires a separate idle controller
to suspend. It checks native suspension before any observation script, then
requires the media timeline to keep progressing after the completed sweep and
remain unpaused/unended. CI rejects a zero-test run or a missing policy result.
The test logs exact source/runtime identity and timing, not user profile data.

This verifies native-WebView-muted local audio and policy-plus-engine scheduling, not audible
hardware output, Spotify/DRM, the App's saved Settings UI/timer, or third-party
visibility handlers. Native host muting keeps output silent while WebView2 still
reports the stream as playing audio; it does not validate the output device. Owned resource cleanup runs on assertion
failure; profile deletion is best effort if WebView2 temporarily retains locks.

A prior attempt to compile this test on the connected laptop was stopped before
host creation when available memory fell below its safety budget. That attempt
produced no playback result and left the user's existing browser untouched.

The original fixture loaded HTML audio with `muted` and never unmuted it. Its
first hosted run failed the no-suspension assertion. Chromium distinguishes such
never-unmuted media from an audible stream when deciding hidden-page playback;
the acceptance fixture therefore uses native WebView muting instead. This is a
fixture/visibility-behavior distinction, not a claim that Spotify was reproduced.
The original run and subsequent state diagnostics remain available in CI history.

References: [Chromium hidden-playback implementation](https://chromium.googlesource.com/chromium/src/+/HEAD/third_party/blink/renderer/platform/media/web_media_player_impl.cc),
[WebView2 audio signal and native muting](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_8).
