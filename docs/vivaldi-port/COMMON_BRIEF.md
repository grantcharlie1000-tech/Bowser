COMMON BRIEF (every wave-1 subagent): you are one thread of the Bowser Vivaldi port.

RE-READ RULE: after every step of your own work, re-read C:\Users\charlie\.claude\vivaldi-port\MASTER_PROMPT.md and compare what you did against it; fix drift before continuing.

SETUP
- Repo: C:\Users\charlie\Documents\ChatGPT\Bowser (your worktree is a copy). First command: git fetch origin && git checkout -B <branch> origin/main. Never touch local main or benchmarks/results/sidebar-verify/run2-B/http-requests.jsonl.
- Windows long-path problem: benchmarks/ contains over-long filenames. If checkout fails, run `git config core.longpaths true` and use `git sparse-checkout set --no-cone '/*' '!/benchmarks/'` (but benchmarks/results/foundation/harness/render.mjs and benchmarks/results/surfaces/panels2/harness/render.mjs are needed for screenshots: add them back with explicit include lines).
- Read first: PRODUCT.md, ui/STYLEGUIDE.md, ui/README.md, docs/html-chrome-plan.md, docs/html-chrome-protocol.md, docs/sidebar-panel-host.md, and your section of C:\Users\charlie\.claude\vivaldi-port\vivaldi-features.txt.
- Audit of existing settings: C:\Users\charlie\AppData\Local\Temp\claude\C--Users-charlie-Documents-ChatGPT-Bowser\7e086296-3425-410a-96e3-4d6d5ca5775a\scratchpad\bw-audit-report.md (what already exists on origin/main).

RULES (verbatim from the master prompt)
- UI style does not change. Reuse existing tokens, components, icons (existing icon pipeline, no new icon families) and motion. New surfaces must look made by the same hand as the existing ones. Layout presets use preview thumbnails in the style Bowser's layout settings already have; add new thumbnails the same way.
- A setting must do something. No row is added unless it changes behaviour, and each new row needs a test that proves the effect (UI -> Rust save -> snapshot -> behaviour). If a Vivaldi option has no meaning on Windows/WebView2, say so and skip it rather than adding a dead toggle.
- All work branches from origin/main.
- Shared files: settings chain (settings.rs, chrome_host/settings.rs, ui/src/settings-page-logic.ts, settings-page.ts, ui/test/settings-page.mjs, win32/settings_panel.rs), chrome_host/protocol.rs and ui/src/protocol.ts, win32/window.rs, app.rs, chrome/layout.rs, theme.rs, ui/src/view.ts, shortcuts.rs. Other threads edit these too: keep your edits there minimal, additive and in their own clearly delimited blocks, to ease rebasing. Settings-touching PRs merge one at a time; before your PR rebase on origin/main and rebuild ui/dist and ui/dist.manifest (rebuild, don't hand-merge).
- Test data: redirect LOCALAPPDATA for every test launch; benchmark flags don't isolate app data.
- Never drive GUI tests on the user's desktop. Headless only: cd ui; npm run build / lint / typecheck / test; cargo test --locked; cargo clippy --locked -- -D warnings; headless screenshot harnesses. Real-app checks are one batched pass the user approves later: list the ones you would want.
- Every UI change gets headless before/after PNGs (light and dark, 100% and 150%). Put them under your own new directory in benchmarks/results/ with SHORT paths.
- Release builds use --features html-chrome-default. Do NOT merge, release, or bump versions. Commit to your branch and open one PR with the file-pr skill; the user merges.
- PRODUCT.md applies: nothing may delay input, focus or navigation or slow cold launch. Passwords stay native.
- Skills: verification-before-completion before any "done" claim; security-audit if your thread is marked (security).
- Never delete a setting or feature without asking (report back instead).
- Do not add network services or accounts.
- Keep the native GDI fallback consistent where the feature has a GDI surface.

DEFINITION OF DONE
New rows each act and are tested; cargo test --locked and cargo clippy --locked -- -D warnings pass; ui lint/typecheck/test pass; headless screenshots exist; verification-before-completion output included in your final report. Your FIRST step is a short plan (existing code you will reuse, which of your settings rows already exist) - include it in your final report; do not wait for approval. If something truly needs the user's decision, finish the rest and list it. Final report: what works, what was skipped and why (Windows/WebView2 meaningless), PR URL, test output summary, any real-app checks wanted.
