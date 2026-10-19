# Single native focus topology control

This is the final bounded topology investigation of the unresolved fast UIA input control. No focus behavior fix, delay, repair, input method change, product rebuild or primary performance claim is included.

## Frozen inputs and stopping rule

Reuse the exact c6f93 controller-focus artifact12136361999, ZIP SHA2568ed7287f9fcdb71087fd61ee9bc256ea06bb4345c14beb37e5c3dce7864bdcfe. Candidate SHA25640c68ab5ad01cfa30b1632f58047e5293b2d7c0f305c05e9be51b4d8a3aff905; reference SHA256ba569e18a2b6bf220805aad6de67ce3adc03d94ce48cbffd01ec2ca65b83147a, reference composite tree50708b8a05cd6f203503b1a83c6bee4f3dad4fc4. Actual binary identities remain c6f93/reference-tree; harness HEAD is recorded separately.

One four-arm pair (reference/candidate, DOM observation OFF/ON), each fresh and reused: exactly8 planned rows, all failures retained, no within-row retry. Same ordinary updater-OFF new profiles, exact input sequence, before/after pinned hashes, native HTML/navigation validity, independent drains and controller coverage. Any identity/isolation loss stops subsequent launches. All-pass means not reproduced, not fixed. Success transients can still discriminate topology. If ancestry classification remains unknown, stop this investigation branch; no automatic larger cohort or additional tracing.

## Falsifiable question

At existing Ctrl+L/SetValue/assertion/Enter boundaries only, retain the native focus HWND/owner returned by Ctrl+L as the original anchor. Observe bounded parent chains of both anchor and current focus, including screen rectangles, visibility and style/ex-style bits, fixed class labels and PID/TID. No new UIA queries, window titles/text, document identifiers, focus APIs, hooks, sleeps or polling are added.

H1a: original/current share a verified ancestor below the owned native root (same_native_subtree). H1b: their verified direct-child or owned-popup containers differ and both have recognized fixed native class labels (different_known_container). Otherwise unknown. Neither classification establishes a WebView controller identity or causal responsibility. Sharing only the native root is insufficient. The anchor is the post-Ctrl+L HWND, not an assertion that its class is an input/controller.

Each chain has a16-node depth bound and visited-handle guard. Owner PID/TID is checked before/after each node and original anchor ownership must match the retained observation. Root PID/TID is rechecked after both chains. Null, destroyed, changed-owner, failed geometry/style or incomplete parent chains remain explicit unknown; absence is never zero/success. Same-PID/TID handle recycling cannot be excluded by these observations and is an explicit limit. No arbitrary class string is logged. Existing fixed labels plus other/unknown are retained.

The entire lookup+snapshot interval includes the extra topology work and anchor bookkeeping. Inner observation_ms includes topology capture, not just GetGUIThreadInfo. Caller dispatch/record append remain excluded. Observations are non-atomic and may perturb timing; no overhead subtraction, primary timing or typing-latency claim. DOM receipt QPC is not DOM execution time; SetValue is bracketed by snapshots, not instrumented at exact API entry/exit.

The previous16-row c6f93 run remains14 pass/two failures, not rescued. Its failing ON row had toolbar LostFocus inside the SetValue snapshot bracket, no restoration or receiving GotFocus, and no intervening observed COM MoveFocus. That does not rule out other native activation/visibility/accessibility effects or prove UIA-only fault. The separate setText overwrite hypothesis remains untested.
