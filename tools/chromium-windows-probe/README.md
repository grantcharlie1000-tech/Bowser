# Chromium on Windows: feasibility experiment

This is an isolated Windows-only CEF/Chromium test. It does **not** modify Bowser's WebView2 engine, Rust dependencies, installer or Linux builds.

## Purpose

The upstream Rust CEF bindings (cef-rs) explicitly support Windows. This experiment uses their maintained example before attempting to integrate CEF into Bowser. It pins the cef-rs release tag cef-v156.3.0+156.0.15 to exact commit 224a785c7b41758b51821cbb7395ea57ba493a63, builds its cefsimple example with the official bundle-cef-app tool, checks the required CEF runtime files and records the bundle size.

## Run

Install Git, stable Rust/Cargo and the Visual Studio C++ build tools and Windows SDK. Allow ample disk space for the CEF distribution and compilation. From the Bowser repo root, run:

~~~powershell
pwsh -File .\tools\chromium-windows-probe\validate.ps1
~~~

For the interactive browser rendering test on a real Windows desktop:

~~~powershell
pwsh -File .\tools\chromium-windows-probe\validate.ps1 -InteractiveSmoke
~~~

The probe caches its pinned CEF checkout and Rust build under %TEMP%\BowserChromiumCefProbe by default and writes validation.json into that folder. It refuses to replace a mismatched checkout.

CI runs **only** the packaging test: hosted Windows runners do not reliably demonstrate a usable interactive GUI. No CEF binaries are committed or uploaded.

## Acceptance gates

1. **Windows compile/package:** Build Rust CEF bindings with a pinned release, bundle the executable and required runtime files, and capture bundle size.
2. **Interactive Windows smoke:** Run the HTML test page, verify canvas/JS, typing, clicks, scrolling, GPU/WebGL, window resizing, clean exit and process cleanup.
3. **Bowser integration (later):** Add a CEF-backed browser surface behind an opt-in build/launch switch; validate tabs, browser events, HTML chrome bridge, separate profile, and downloads without changing WebView2 behavior.
4. **Shipping decision (later):** Compare startup, RAM/CPU, media, extensions, sandboxing, installer size, updates and protected playback against the standard build.

Passing gate 1 does **not** mean Bowser supports standalone Chromium, nor that extension parity is proven. Do not change the default Windows engine or merge an engine switch based solely on this result.

Upstream references: https://github.com/tauri-apps/cef-rs and https://github.com/chromiumembedded/cef.
