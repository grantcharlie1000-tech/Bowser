# Background media suspension guard

## Code-level diagnosis

Switching tabs still hides the old WebView2 controller with `SetIsVisible(false)`.
That is normal tab visibility behavior and is not established here as the cause
of an immediate site-specific pause. No JavaScript visibility spoofing, autoplay
bypass, forced `play()`, or globally visible background controllers are added.

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
navigation/completion, controller replacement, close, and reentrant native audio
queries. Run these on Windows:

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
