# Bowser performance execution contract

Baseline: `origin/main` at `440575e0870fd35c257d4195ec990d3c02b837aa`, verified against the remote on 2026-11-15.

## Acceptance

- Every task area must take at most 50% of its baseline time. RAM is reported separately and never substitutes.
- Priority tasks: retained/suspended-site wake, discarded-site restoration, new-tab opening, and ChatGPT.com responsiveness.
- ChatGPT workloads are separate: typing, scrolling, opening conversations, switching conversations, and responsiveness during reply streaming.
- Page loading, searching, settings, video/audio startup and stalls, and download initiation/completion remain in scope.
- Preserve features, correctness, security, privacy, visual quality and accessibility. Report resource, energy, durability and background-work changes separately.
- No spending or iteration cap. Each thread must remain below 2,000,000 context tokens; use fresh phase threads and bounded handoffs far below that ceiling.
- Use OpenAI models for audit and implementation threads; the user now prefers GPT-6.1 Sol with xhigh reasoning for future OpenAI threads. The user's subsequent clarification expands the council to the top available model from every connected provider, with OpenCode explicitly excluded. Retain independent OpenAI/Anthropic findings and act on verified findings now. Scrutinize Gemini claims against source/evidence and explicitly identify unsupported or incorrect claims; do not delay confirmed fixes for a pending review.
- Every independent council member must score at least 9.2/10, with unresolved blockers preventing acceptance. Actual per-area measurements must meet the target independently of scores.
- A shortfall may end the program only when the council unanimously substantiates that no further viable savings remain under these constraints. Missing evidence is not proof of exhausted savings.

## Execution

1. Parallel read-only subsystem audits.
2. Freeze production-equivalent release baseline, fixtures and primary endpoints before evaluating gains.
3. Fresh isolated implementation threads with explicit file ownership and exact skill paths.
4. Parent review and serial integration. No shared hot-file editing.
5. Serial paired performance measurement, separate profiling, meaningful correctness/security verification.
6. Fresh independent council reviews of the same final source and evidence. Resolve findings and repeat as required.

The user has not authorized commits, pushes, publishing, deployment or changes to their actual browser data. Preserve all existing work and use isolated app-data and WebView2 profiles.

## Measurement rules

Use release builds with HTML chrome and normal protections. Keep binary/source/asset/runtime identities, cache/profile conditions, power and window conditions, machine load, error/work counts and raw samples. Use at least five alternating pairs for an exploratory comparison. Acceptance requires at least ten valid alternating matched pairs per condition, expanding when variability requires it. Choose the sample plan before evaluating candidate gains; retain failed attempts and reasons in the report.

A readiness event, page title, double animation-frame acknowledgement, microbenchmark or successful compilation cannot establish visible/usable task completion. Record the practical limit of each endpoint. Confirm operation success, correct output and actual work in the timed region. Distinguish server/transport, renderer, extension and Bowser-host costs.

The parent owns live GUI and performance runs. Audit workers must not execute builds, GUI probes or benchmarks. This machine currently has four logical processors and limited free RAM, so builds and later checks must be scheduled conservatively; build load is never a performance sample.

## Skill assignment

The parent selects the smallest relevant installed/project skill set for every delegation, supplies exact SKILL.md paths and purposes, and requires a brief applied-skills handoff. Current assignments cover benchmark design, root-cause tracing, TypeScript contracts, boundary validation and lifecycle modeling. The shared context guard applies to bounded reads and reports. Generic skill defaults never override the user's provider scope, read-only, no-tradeoff, context or orchestration constraints.

Future phases use isolated-state guidance for parallel fixes, the interrogate rubric for independent council review, and scoped UI-verification guidance for actual runtime probes. External skill installation is unnecessary unless a concrete capability gap appears.

## Status

All five initial audits are preserved in this directory: navigation/search, settings/UI, streaming/downloads, memory/measurement coverage, and stability. They are source evidence and hypotheses, not measured gains or council verdicts.

The frozen baseline release build completed successfully with `html-chrome-default`, locked offline dependencies, and one build job. Its manifest is at `C:\Users\charlie\.t3\artifacts\bowser-performance-20261115\baseline-manifest.json`. Executable SHA-256: `F0203D9F8F928C82598D13E8EF279315B80C12EA46828C7F8ED0A88AC00C327E`.

The controlled stable-reference browser-process loss reproduced a modal error blocking recovery; see `repro-stable-process-loss.md`. It does not establish the cause of the user's spontaneous crashes or pixel transparency. Separate isolated implementation threads own process recovery, native surface allocation safety, and redundant warm new-tab model rebuilding. The reviewed native-surface package is integrated locally; parent window recovery hooks, full-suite/release checks and live acceptance remain pending. Stability and usable recovery gate later performance acceptance.

The process-recovery package and parent document/controller preservation corrections are also integrated. Their 16 focused tests pass; the combined pre-start-cache default-feature library suite passed 2972 tests, with 42 ignored. A fresh isolated thread is implementing remaining bounded drawing/window recovery and safe chrome restoration. The start-cache package and required image-delivery settlement hook are integrated locally; 98 combined targeted tests passed, with two ignored. Its accidental embedded timing run is excluded from performance evidence. The request-filter copy reduction is also integrated; all 25 focused reconciliation/protection checks passed. Its allocation result is not a task-time or RAM result. No per-area 50% result or council verdict exists yet.

The settings snapshot package and parent protection hook are integrated locally. The worker's separate package passed 47 scoped tests; after an initial unavailable-lane attempt, the parent combined integration passed 51 scoped tests. See `settings-integration.md`. The bounded native-window recovery package is now also integrated. Safe full HTML-chrome restoration still requires the document-state acknowledgement protocol; no current native or full restoration acceptance is claimed. Council reviews have not been convened; the completed audits and implementation checks are not council verdicts.

The combined shipping-feature Rust library suite passed 3017 tests, with 42 ignored and no failures. The settings-search source package is integrated; parent corrections to its live-child-list fake DOM adapters resolve its reported full-file stall. All 93 full settings/navigation tests, typechecking, lint and the standard matching UI asset build passed. See `settings-search-integration.md` for red/green evidence and limits. The first isolated release candidate built successfully. In a disposable-profile probe, two forced losses of its own WebView2 browser process exposed native recovery controls without a modal error; explicit reload recreated content, and Alt+F4 closed normally with exit 0. See `candidate-1-native-recovery.md` for raw evidence, screenshot-capture failures and limits. Full HTML restoration and complete live acceptance remain outstanding; no task-time/RAM target has been established. The first independent council round is being prepared against this exact candidate, including all missing evidence as acceptance blockers.

