# Performance pass — 10 October 2026

Baseline exe = clean HEAD `093194b`. Changes are uncommitted and not merged. Windows 10 Home 19045, i5-7300HQ, 15.9 GiB, one 1920×1080 96-DPI monitor.

Every number below comes from single runs or small samples (n = 1 to 15) on one machine. Treat differences under about 10% as noise.

## Summary

| Target | Result | Status |
|---|---|---|
| Startup 20% faster | First chrome frame −28%; window_visible −10% | Met for first chrome frame; not met for window_visible |
| Lower idle memory 20% | −53% to −81% with simulated memory pressure and 5–25 tabs; no change otherwise | Met only under memory pressure with several background tabs |
| Near-zero idle CPU | 0 ms over 20 s (already met before this pass) | Met |
| Fewer background wakeups | Caret blink and idle timer no longer run in the background | Fixed; final numbers before 530–706 context switches per 30 s with another app in front; after the foreground fix 8–18 (measured by the investigation agent, see the caret notes below). Live resume of the caret after returning to Bowser is still being verified |

## Host process (Bowser-controlled, excluding WebView2)

| State | Working set | Private |
|---|---:|---:|
| Empty window | 11.4 MB | 2.2 MB |
| 1–25 tabs | 17–19 MB | 3.2–4.0 MB |

Idle CPU was 0 ms over 20 s for the empty window with the omnibox focused, and for a loaded page. Idle CPU was already near zero before this pass.

## Startup

Interleaved runs, n = 15. Window shown and first chrome frame presented.

| Measure (median, p25–p75) | Baseline | This pass | Change |
|---|---:|---:|---:|
| First chrome frame | 68.9 ms (64.1–79.7) | 49.7 ms (47.4–66.4) | −28% |
| window_visible | 51.5 ms | 46.4 ms | −10% |

Second run by the verification agent, n = 9, fresh / warm:

| Measure | Baseline | This pass |
|---|---:|---:|
| Outer launch to visible | 100.1 / 94.1 ms | 96.0 / 83.9 ms |
| input_ready | 49.5 / 47.6 ms | 47.1 / 45.6 ms |
| chrome_painted | 50.9 / 48.8 ms | n/a on baseline (no such event) |

Baseline phase breakdown before first paint (medians):

| Phase | Time | Action |
|---|---:|---|
| Process start to WM_CREATE | 7.9 ms | None |
| EDIT, font and subclass | ~2 ms | None |
| Overlay popup window creation | ~21 ms | Kept; hiding the window only moves the cost |
| CreateWindow to window_visible | ~11 ms | None |
| First paint | ~17 ms | Font realisation moved to a background thread (first GDI text draw was 5–15 ms) |

The fix realises the font on a background thread while the window is created. It adds a `chrome_painted` event, logged after the first present.

## Per-paint cost

The layered chrome overlay used to upload the whole surface on every WM_PAINT, including 3 × 15 px caret blinks. It now uses `UpdateLayeredWindowIndirect` with the dirty rectangle. A full upload happens only on the first present or on a size, DPI or origin change. The per-paint `GetDC` is gone.

| Paint | CPU time |
|---|---:|
| 1000 × 62 full paint, cold first frame (includes font realisation) | ~11–15 ms |
| Caret repaint | ~0.1–0.7 ms |

Visual check: nine scenarios produced byte-identical screenshots between baseline and this build. The scenarios were fresh launch plus three caret phases, four tabs, window move, tab hover, plus hover, resize to 1024×700 and 1440×900, and minimise / restore.

## Memory by tab count

Distinct-site loopback tabs. WebView2 private MB, about 50 s after the last tab opened. Single run per cell. Processes in brackets.

| Tabs | Baseline | This pass, no pressure | This pass, simulated pressure | Change under pressure |
|---:|---:|---:|---:|---:|
| 1 | 113.6 (7) | 118.4 | n/a | n/a |
| 5 | 249.3 (11) | 226.9 | 118.1 (7) | −52.6% |
| 10 | 389.6 (16) | 387.0 | 134.2 (7) | −65.6% |
| 25 | 847.2 (33) | 843.2 | 159.4 (7) | −81.2% |

Without pressure the new build matches baseline within noise. One 25-tab run without simulated pressure recorded 9 discards from real system pressure (596 MB). Under simulated pressure, 4 / 9 / 24 tabs were discarded at 5 / 10 / 25 tabs.

Hidden tabs already used `MemoryUsageTargetLevel::Low` and `TrySuspend` after 30 s. The surviving cost was about 29 MB committed per hidden tab. Only discarding (closing the controller) removes that.

### Discard rules

Discard runs only under memory pressure: `GlobalMemoryStatusEx` load ≥ 85%, or available memory < 512 MiB.

- Longest-hidden tab first, at most one per timer tick.
- Only tabs that are inactive, not pinned, suspended (hidden ≥ 30 s), not loading, with a committed http(s) URL, and not playing audio.

Consequences for the user:

| Behaviour | Effect |
|---|---|
| Selecting a discarded tab | Recreates the controller and reloads the last committed URL |
| Back / forward history | Lost |
| Form-POST pages | Reload as GET |

### Why the idle target is only met under pressure

With one tab or no pressure there is no change, because 95%+ of Bowser's memory is the WebView2 runtime.

## Background wakeups

UI-thread context switches per 30 s, with another app in front and one loaded page.

| Build | Switches per 30 s | Notes |
|---|---:|---|
| Baseline, first sample | 706 | |
| Baseline, later samples | ~530–564 | |
| This pass | before 530–706 context switches per 30 s with another app in front; after the foreground fix 8–18 (measured by the investigation agent, see the caret notes below). Live resume of the caret after returning to Bowser is still being verified | |

Baseline cause: the caret blink timer (about 57 WM_TIMER and 57 WM_PAINT per 30 s). The omnibox kept keyboard focus while another app was foreground. The 1 Hz idle-memory timer also contributed.

Fix: the idle-memory timer polls only while Bowser is foreground. In the background, a loaded page is trimmed once and polling stops.

## Investigated and rejected

| Idea | Result |
|---|---|
| Hide the overlay window at creation | Moves the cost; no net benefit |
| Reuse DIBs across resizes | No measurable benefit |
| Faster omnibox suggestions | Not needed; history is capped at 500 entries and lookups take microseconds |
| Cut WebView2 background CPU after tabs sleep | About 3% over 20 s with 10 awake tabs; drops once tabs are suspended, so nothing to fix here |

## Verification

| Check | Scope | Status |
|---|---|---|
| Timing runs (startup, tabs, idle) | Startup, memory, wakeups | Measured, single runs or small samples |
| Screenshot comparison, nine scenarios | Paint change | Manual visual check |
| `cargo fmt --check` | Formatting | Passed |
| `cargo clippy -- -D warnings` | Lints | Passed |
| `cargo test` | Unit tests | 311 passed, 3 ignored (artefact probes) |
| `cargo build --release` | Release build | Passed |

Not verified:

- Caret blink while hovering, within a single sequence.
- DPI changes and multi-monitor setups.
- Windows 11.
- Window-drag latency. See `docs/window-drag-verification.md`.
- Frame rate and tab-switch latency. No measurements were taken.
- Shutdown: a 25-tab run took 1–27 s to exit. Baseline showed this too. The cause is unexplained.

## Known limits and next opportunities

- While minimised, the 30 s hidden-tab suspension does not run because its timer is killed. Only tabs that are already suspended can be discarded while minimised.
- A discarded tab looks identical to a sleeping tab.
- A tab whose navigation failed stays flagged as loading, so it is never discarded.

## How to reproduce

Run `benchmarks/perf-probe.ps1` against the built executable:

```powershell
.\benchmarks\perf-probe.ps1 -BowserExecutable <path-to-bowser.exe> -Label baseline -Mode startup
.\benchmarks\perf-probe.ps1 -BowserExecutable <path-to-bowser.exe> -Label current -Mode tabs
.\benchmarks\perf-probe.ps1 -BowserExecutable <path-to-bowser.exe> -Label current -Mode idle
.\benchmarks\perf-probe.ps1 -BowserExecutable <path-to-bowser.exe> -Label current -Mode tabs -MemoryPressure
```

- `-Mode` is `startup`, `tabs` or `idle`. Default is `startup`.
- `-Runs` sets the sample count (default 9). `-TabCounts` sets the tab counts for `tabs` mode (default 1, 5, 10, 25).
- `-MemoryPressure` adds `--benchmark-memory-pressure on`. This is a benchmark-only flag that enables the discard arm.
- Results are written under `benchmarks\results\perf-probe` by default. Use `-OutputDirectory` to change this.
- Baseline comparison: build `093194b` in a separate worktree and pass its executable with a different `-Label`.
