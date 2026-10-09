# Project Bowser — Phase 0: From Nothing

**Milestone:** Bowser 0.0.1  
**Source:** User-provided Phase 0 brief, 2026-10-08

## Objective

Build the smallest genuinely usable Windows browser around one goal: run quickly on weak hardware while using as little RAM, CPU, disk, and battery as practical. Do not make performance claims without measurements.

## Product scope

The full interface is one native Windows window, one text input at the top, and one embedded webpage viewport below. The input accepts a URL or search text; search uses a configurable provider. Search-result links stay inside Bowser. Alt+Left and Alt+Right move through the current page's history.

Bowser starts as an empty window. It does not load a homepage or search engine until the user submits input. It has no tabs, bookmarks, accounts, extensions, settings panel, welcome page, animation, telemetry, polling, or preloading.

## Architecture and constraints

- Native application: Rust and direct Win32 controls.
- Rendering: Microsoft's installed Evergreen WebView2 runtime, behind a replaceable engine adapter.
- State: a dedicated WebView2 user data folder under the current user's local application data; never share Edge's profile.
- Dependencies: use direct, minimal Windows bindings. No Electron, React, browser-based application UI, or heavyweight UI framework.
- Security: retain WebView2 sandboxing, site isolation, and security updates. Do not add flags that disable browser security for benchmark gains.
- Lifetime: close the WebView2 controller cleanly and allow its app-owned process group to exit.
- Runtime failure: detect a missing WebView2 runtime and show a helpful message with installation guidance.
- Safe defaults: deny website permissions; cancel downloads; route user-initiated web popups into the existing viewport when safe, and block other popups; block external protocol launches.

## Performance evidence

Provide a reproducible local benchmark process for:

- Time from launch to a visible and responsive window.
- Time until the first search or navigation can be submitted.
- Time from submission to a usable first page.
- Idle CPU.
- RAM for every Bowser and WebView2 process, not just Bowser's parent.
- RAM with one lightweight and one demanding page.
- Bowser's own disk footprint, separate from the shared WebView2 runtime.

Compare Bowser, Edge, and Chrome on the same computer with matching workloads and fresh-process and reused-profile launches. Record the host, browser/runtime versions, test conditions, and actual results. Clearly label any metric that cannot be measured; do not invent battery figures or performance improvements.

## Delivery and acceptance

The 0.0.1 deliverable is a Windows release executable, Start-Bowser.cmd, and a concise README with build/run steps and runtime requirements. Run the requested tests and available benchmarks, record working behavior, failures, the largest measured bottleneck, and three evidence-backed or explicitly hypothetical next performance improvements. Stop at 0.0.1.

## Implementation assumptions to review with the plan

- Target x64 Windows first; keep the window native and empty on launch.
- Check WebView2 availability without starting its browser process, then create the WebView2 controller only when the user submits the first navigation. This follows the no-preload requirement and keeps the empty window light.
- Use DuckDuckGo's HTML search endpoint as the initial provider. A local text configuration file under Bowser's LocalAppData directory overrides its URL template; it must use HTTPS and contain one query placeholder.
- Permit HTTP and HTTPS destinations from the address input. Unknown or unsafe schemes are rejected rather than passed to WebView2. A benchmark-only command-line option may load fixture files from the repository's benchmark directory; normal launches cannot use that option.
- Browser state remains local. The app itself has no remote service; browsing and search requests use the network when the user asks for them.
- Benchmark fixture pages are local files and never load third-party resources. Bowser opens them only when explicitly launched with benchmark logging and a benchmark URL under the fixture directory. Search-provider behavior is separately smoke-tested against the configured provider.

## Host observed during planning

The workspace was empty apart from its Git metadata. The host is an Acer Predator PH317-51 with an Intel Core i5-7300HQ (4 cores), 15.9 GiB RAM, and Windows 10 Home 64-bit. Rust 1.99.0, Visual Studio Build Tools 18 with MSVC, Windows SDK 10.0.26100.0, Edge/WebView2 154.0.4258.62, and Chrome 154.0.8037.98 are installed. These are environment facts, not benchmark results.

## Technical references reviewed

- [Microsoft WebView2 user data folders](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/user-data-folder)
- [Microsoft WebView2 process model and safe shutdown](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model)
- [webview2-com Rust bindings](https://docs.rs/crate/webview2-com/0.39.1)
- [webview2-com WebView2Loader linkage](https://github.com/wravery/webview2-rs#cross-compilation)
- [Legacy high-level webview2 crate](https://docs.rs/crate/webview2/0.1.4)
