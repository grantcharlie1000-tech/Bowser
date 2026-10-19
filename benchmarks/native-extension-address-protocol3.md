# Native extension resource address probe, protocol 3

This is a test-harness correction only. It changes neither Bowser production
code nor the measurement acceptance gates. It does not establish a cause for
an earlier missing address observation, and it cannot rehabilitate old samples.

## Evidence boundary

Run [39243519349](https://github.com/grantcharlie1000-tech/Bowser/actions/runs/39243519349),
resource/download job [118029761323](https://github.com/grantcharlie1000-tech/Bowser/actions/runs/39243519349/job/118029761323),
artifact 12182378265, was produced from source
`5d85e64c960fbfe39c0c32a46daa3a0e83b8d8ed`.
The downloaded ZIP SHA-256 is
`495afd8b9289ad10e575a66fbfbf44f7730516f69ca7fe5beefa4ffe884d9eb5`.
Its report retains two attempted startup/resource rows, one primary valid pair,
zero navigation valid pairs, and `series_complete=false` / `status=failed`.

The zero-extension trial completed its 66-key navigation. The several-extension
trial failed before any literal key with `observed_exact_edit_count=0` from the
combined whole-window Name+Edit query. Only subsequent failure observations
found one exact-name element, one Edit element, and four Document elements.
Those later observations did not record source URLs, ancestry, runtime IDs or
query timing. They are not proof of duplication, a permanently missing control,
or an identified timing/provider cause. They did not authorize input.

There is still no valid complete resource series. No relative resource or
performance-improvement claim follows from this correction or the old report.

## New authority and input contract

All new source/identity conditioning remains after the primary startup/resource
snapshot. Protocol 3 activates the exact owned native window, then requires one
outermost UIA Document whose provider-reported source is exactly the embedded
`bowser-ui://app/index.html?view=toolbar` URL from
`src/chrome_host/assets.rs`. Document title, accessible name and provider PID
are never source authority.

The native read-only COM adapter matches complete managed/native root and
Document runtime IDs. It reads LegacyIAccessible.Value with property 30093 and
`ignoreDefaultValue=true`. Unsupported, default, empty, non-string or conflicting
source values cannot establish toolbar authority. All COM results, including
non-string/unsupported values, are released in finally blocks. The owned HWND,
PID, thread, class, visibility, enabled state and foreground are checked on both
sides of native source retrieval.

Only inside that attributed Document does one exact-name query collect address
candidates. Exact current Edit type and nearest-Document ancestry are required;
zero or multiple matches fail immediately. Hidden/disabled matches are not
filtered away to manufacture uniqueness. The original native window/thread and
root/Document/Edit runtime identities remain bound throughout the lane. A
replacement identity fails rather than being reacquired as a new authority.

Each literal key and the single Enter require fresh source, identity, focus,
neutral-modifier and native-foreground guards. The exact text is read last from
the same ValuePattern after those guards and before Enter. No SetValue, typing
repair, global/first-match fallback or retry of a missing/ambiguous observation
is added. Existing finite focus/final-value waits do not catch selector errors.
A key-attempt record is appended before guard/dispatch/post-check work; if any
of that fails, its phase, dispatch-attempt/return flags and error remain in the
failed sample. Returned dispatch is not proof of key delivery.

The existing executable-hash latch, resource capture, runtime-version checks,
primary/navigation validity, pair/series accounting and cleanup/reporting code
from `Assert-FrozenExecutable` onward are unchanged. Protocol 3 labels its added
source checks as a changed UIA-conditioned correctness workload, not comparable
browser-only latency or a correction to old protocol-2 rows.

## Bounds and verification limits

Document inventory is capped at 64, name inventory at 10,000, ancestry at 64,
runtime IDs at 128 integers and document sources at 4,096 characters. Those are
inventory/output acceptance limits, not hard limits on synchronous UIA provider
call time or allocation. Sequential identity/focus/source checks cannot make
OS keyboard delivery atomic. A provider hang or UI change between checks may
still fail this lane; a fail-closed refusal is not a failed metric to erase.

`python benchmarks/test-native-extension-address-source.py` checks eight static
source/wiring contracts, including the unchanged measurement/accounting fence.
It is not PowerShell parsing or UIA execution.

`benchmarks/scripts/test-native-extension-address-guards.ps1` parses the real
harness, compiles its native declarations, and tests extracted source/runtime
and input guard functions on generated data with stubbed I/O. It sends no keys,
opens no browser/profile, and is not provider/runtime evidence. The Windows
workflow runs both contracts before the resource job's existing build and live
measurement. A real Windows run is still required to validate the COM binding,
source exposure, control structure and new protocol. Local Linux validation has
no PowerShell, Windows desktop, Rust or .NET toolchain, so none of those native
stages are claimed as run.

## Primary references

- [Microsoft: UI Automation property retrieval and unsupported defaults](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-propertiesforclients).
- [Microsoft: native runtime IDs are opaque, desktop-local and reusable over time](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationelement-getruntimeid).
- [Microsoft: LegacyIAccessible pattern Value](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nn-uiautomationclient-iuiautomationlegacyiaccessiblepattern).
- [Chromium: platform-document accessible Value uses tree-data URL](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/ui/accessibility/platform/ax_platform_node_win.cc).
