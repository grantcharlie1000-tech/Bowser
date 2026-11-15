# Candidate 1: controlled browser-process loss

This is a functional fault-injection probe, not a performance measurement, a visual acceptance result, or attribution of the user's ordinary-browsing crashes.

## Identity and isolation

- Base commit: `440575e0870fd35c257d4195ec990d3c02b837aa`.
- Shipping feature: `html-chrome-default`; locked offline release build, one Cargo job.
- Executable: `C:\Users\charlie\.t3\artifacts\bowser-performance-20261115\stability-candidate-1\Bowser.exe`.
- SHA-256: `A1924E932C03F41E3C7BB48631EFCEA5725789EAE436BB2F4689660DA345221C`; 8,599,040 bytes.
- Source identity: `candidate-1-manifest.json` beside the release log in the artifact root. The builder verified source identities before and after compilation.
- Probe directory: `stability-candidate-1\probe-24c9a9e99ff346c09ad1959f3e8bc00a` under the artifact root.
- Root PID 27128, started `2026-11-15T04:02:22.3484118Z`; fixture server PID 33764. Both have exited.
- Separate disposable profile and LOCALAPPDATA; inherited BOWSER/WEBVIEW2 overrides removed. Installed Bowser and its actual user profile were untouched.
- Runtime: WebView2 `157.0.4341.46`. Only the browser-role processes belonging to this root and exact disposable profile were terminated, after PID, start time, executable, parent, role and profile checks. No process-tree/global-name kill was used.
- Fixture: loopback HTTP `/demanding`, 4,000 synthetic rows, token `candidate_1_31c47b79c88f4a418915f3e9673990d4`. Fixture readiness is semantic evidence only.

## Observations

1. The initial GET and `/ready` acknowledgement succeeded. Accessibility showed the expected document URL/token and the HTML toolbar, tabs and rail. Ctrl+L and typed test text changed the HTML address field; Escape restored the displayed address without navigating.
2. The owned WebView2 browser PID 5568 was terminated. The same Bowser window survived. Accessibility exposed a native address edit (ID 100) and the native button “The browser process stopped. Reload this address. Submitted forms will not be sent again.” The window inventory contained no modal error dialog.
3. Ctrl+R explicitly initiated recovery. A new owned WebView2 browser PID 13004 appeared. The fixture received a second GET and `/ready` acknowledgement; accessibility showed the expected recovered document. HTML toolbar/tabs/rail remained absent, consistent with the known lifetime-native-fallback limitation that the separate restoration package is addressing.
4. The native address field eventually contained `native-recovery-control-check`. Some input/accessibility calls and `setValue` timed out or briefly returned unavailable state, so the sequence does not establish input latency or which individual command produced the final value.
5. Terminating only the recovered owned browser PID 13004 again produced the native recovery button and address field, with no modal error in the window inventory. The root remained alive.
6. Alt+F4 removed the candidate window. The owned launch helper recorded exit code 0 at `2026-11-15T04:05:12.2306446Z`, cleaned up its fixture server, and released the shared build/probe mutex. Process metadata confirmed both owned PIDs absent. The installed Bowser window remained present throughout.

## Evidence and limits

Raw records in the probe directory: `launch.json`, `fault.json`, `fault-2.json`, `exit.json`, `requests.jsonl`, and `profile\diagnostic.csv`. Native UI observations are in the root tool transcript. The diagnostic log records typed engine failures, browser loss, new controller creation/navigation/fixture readiness and orderly shutdown.

Screenshot capture failed with both “FrameArrived timed out: timed out waiting on channel” and “window capture timed out: timed out waiting on channel”. No screenshot was obtained. Accessibility does not establish opaque pixels, presentation, caption hit testing, click geometry, accessibility parity or visual quality. Alt+F4 was verified; caption-button clicking was not.

No spontaneous host crash, natural trigger, allocation exhaustion, renderer-only failure, GPU/utility failure, POST, private/split tab, glass/solid/DPI/fullscreen matrix or long browsing session was exercised here. Full HTML restoration, other recovery cases, resource effects and user-visible task timing remain outstanding. No 50% or RAM reduction claim follows from this probe.

The combined shipping-feature Rust library suite passed 3,017 tests, with 42 ignored and zero failures. The full settings/navigation Node checks passed 93 tests; typecheck, lint and the matching UI asset build exited 0. These checks establish their scoped behavior/build properties, not the end-to-end performance target.
