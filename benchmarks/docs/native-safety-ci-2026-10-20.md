# Native safety CI repair checkpoint, 20 October 2026

This is a branch-only investigation and repair checkpoint based on main
`2499596b22234e6c0b91fc4c060b7112599bdc00`. It does not claim all native safety
checks pass, that an extension is compatible, or that the experimental engine
menu feature is safe to enable in shipping defaults. No shipping feature,
permission, release, deployment, or merge behavior changes here.

## Retained failures and evidence

The source run is [Store native safety run 39379895266](https://github.com/grantcharlie1000-tech/Bowser/actions/runs/39379895266).
The relevant artifacts are `12245526894` and `12245601418`.

1. Extension menu observation sent its one physical right-click, then timed out
   waiting for a Bowser-process Win32 `#32768` popup/GUI popup mode. Its retained
   native topology instead shows an actual browser-process `Chrome_WidgetWin_2`
   popup, exact host parent/owner/root-owner, and matching native source focus
   and popup capture. No Escape was sent. This is a backend mismatch in the
   helper, not evidence that no genuine menu existed.
2. The separate default Chromium cancellation gate refused a depth-one UIA
   structural Pane whose provider PID differs from the retained browser PID.
   The existing evidence did not capture that Pane HWND's native ownership or
   establish a valid fixture-row ancestry. No Escape was sent. This refusal
   must remain until new evidence supports a safe correction.
3. Actual App Reload refused before input because the extension receipt window,
   not the ordinary App owner, was foreground when its helper started. The
   earlier Rust activation had returned success. No Reload call was made in
   this run; it does not diagnose the later in-modal cancellation path.
4. The Copy lane opened an empty clipboard successfully but refused sequence
   zero before mutation/input. A successful open/format enumeration is not
   sequence authority. Zero does not by itself uniquely prove a missing access
   right. The Copy/Paste behavior remains unproved and the zero refusal remains.

## Changes

- The extension-observation entry point now delegates to an explicit unhandled
  WebView2 Chromium Views backend, sharing the separate default lane's exact
  retained process/native focus/capture/foreground, unique UIA fixture-row,
  ancestry, identity, and disappearance proofs. It keeps its own report name,
  scope, Rust callback/inventory assertions, fresh profile and no-grant checks.
  It never substitutes a native-inventory-only pass for a visible-row proof.
- A foreign UIA provider still unconditionally throws before Escape. Its
  failure-only report adds bounded metadata for only the exact surfaced HWND:
  PID/thread/class, native ownership/ancestry, visibility, geometry, and child
  relationships. It reads no foreign title or UIA content. Root identity is
  fenced before and after the diagnostic read. Metadata is non-atomic and
  cannot authorize input or allow skipping a provider.
- Reload hides only its exact live fixture receipt window once, before its
  one original-App activation. The retained COM view remains alive for worker
  and permission receipts. A test-only lifetime/PID/STA/class-guarded seam is
  clone-only under engine borrows; no borrow survives native calls. Hidden-state
  rechecks after queued messages and immediately before helper launch are
  observation-only. Reappearance or lost foreground fails, without re-hide,
  reactivation or input rescue. Existing document, native Reload, completion,
  spontaneous disappearance and no-grant gates remain unchanged.
- Only after clipboard admission has failed and the clipboard was confirmed
  closed, a bounded read-only diagnostic records station/desktop metadata,
  input-desktop status, session IDs, and one non-inheritable attempt to open
  the exact current station with `WINSTA_ACCESSCLIPBOARD`. A newly opened handle
  is closed; borrowed handles are not. This does not change station attachment,
  permissions, desktop, privileges, clipboard data or the failed outcome. Even
  a successful access probe is not authority to use sequence zero.

The native readiness and disappearance deadlines are not increased. No foreign
provider filtering, clipboard initialization/seeding, unsafe flags, fabricated
permission, ignored required failure, or native selection fallback is added.

## Verification and next evidence

`python benchmarks/test-native-safety-source.py` is a portable source-contract
check. `powershell.exe -NoProfile -File benchmarks/scripts/test-native-menu-helpers.ps1`
parses every affected PowerShell entry point and compiles its managed helper
without executing any probe body; it also tests pure diagnostic JSON escaping.
Neither is Windows native behavior acceptance.

The existing `native-safety` job retains each genuine isolated test and its
failure artifacts. The extension/default gates may now both refuse the same
unresolved UIA foreign provider; the clipboard gate may still refuse zero.
These outcomes must remain red, not be treated as waived or substituted by the
independent Note/Split pass. Use the resulting exact HWND/access evidence to
identify the next correction. The earlier separate reader CJK timing failure
in a release run is not conflated with these native gates.

Primary API references:

- [GetClipboardSequenceNumber](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getclipboardsequencenumber)
- [OpenWindowStationW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-openwindowstationw)
- [GetUserObjectInformationW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getuserobjectinformationw)
- [Window station security and access rights](https://learn.microsoft.com/en-us/windows/win32/winstation/window-station-security-and-access-rights)

## Follow-up evidence from PR #68, head 06f4f75

[Native-safety run 39388446363](https://github.com/grantcharlie1000-tech/Bowser/actions/runs/39388446363)
compiled the four managed helper classes, passed their pure serializer/layout
checks and five source contracts at that head, and
built/executed the native lane. The strict native-safety result remained red.
Both menu refusals now identify an `Intermediate D3D Window` that is an actual
disabled child of the exact Chromium popup, not a proved unrelated UIA branch.
The current code collects no-default control/content/focus property kinds and
values, raw child absence, error-qualified first-native-child observations,
and a bounded actual WebView2 process-group snapshot with matching
disposable UDF and browser entry. This is diagnostic only: foreign providers
still unconditionally throw. No class-only or subtree-skipping exception exists.

The clipboard failure remained sequence zero before input/mutation on
WinSta0/Default, with an input desktop, matching process sessions, and a
successful non-mutating right-4 access-open probe. This does not establish a
missing-rights cause or authorize zero. The precise result is sequence authority
unavailable on this runner; the required gate stays red.

Reload reached a stronger target-focus check and refused before native input.
Its initial preparation is now ordered as one owner activation, bounded message
turns observing that request, one MoveFocus, a positive native/document focus
pair separated by another message turn, and one target focus request requiring
separate document/target booleans. All phases share one monotonic three-second
acceptance deadline; late native/COM returns refuse. No nested ten-second script
pump, readiness-first bypass, repeated focus, sleep extension, or input rescue
is used in this preparation. Source focus/geometry/document and post-click
Reload/completion/disappearance/permission gates remain required.
