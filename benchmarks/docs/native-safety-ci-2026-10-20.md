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


## Exercised in-modal Reload failure and correction

[Follow-up native-safety run 39391619788](https://github.com/grantcharlie1000-tech/Bowser/actions/runs/39391619788)
proved source conditioning, one genuine right-click, an actual App Reload return,
and one same-view/same-URL native NavigationStarting. Nevertheless, the exact
popup remained modal and no deferral completion arrived within the unchanged
five-second gate. The product coordinator excluded Showing from stale-proof
checks, and its cancellation path returned without ending TrackPopupMenu.
Stale selection was still refused after return, so this is a stuck cancellation
lifecycle, not evidence of a grant or dispatch.

The default-off coordinator now distinguishes healthy, explicitly stale, and
borrow-unavailable/unknown modal contexts. A healthy Showing menu is not expired
by the inventory deadline or a queued continuation. Only explicit staleness can
attempt one production EndMenu, after fresh same-STA owner lifetime, exact
registry Rc/claim/request, retained lexical HMENU, unique bounded native popup,
GetMenuBarInfo, GUI owner/menu mode and absent/exact-owner/exact-popup capture
proofs. App/Core/gate/registry guards end before native calls and are rechecked.
The attempted call does not finish, destroy or Complete in-modal; only the real
TrackPopupMenu return permits ordinary cleanup and the existing single Complete.
Unknown ownership or multiple visible popup windows (including unproved submenu
shapes) defer cancellation rather than use a generic thread-wide fallback.
No test Escape, helper EndMenu, input rescue or acceptance deadline changed.

The new GPU diagnostics also show that control/content/focusable properties are
explicitly NotSupported on the observed compositor node, despite matching GPU
process-group membership and leaf observations. No structural exception is made;
both foreign-provider gates remain required and may still refuse. Clipboard
sequence-zero refusal likewise remains unchanged.

- [EndMenu](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-endmenu)
- [GetMenuBarInfo](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getmenubarinfo)

Local portable checks are source contracts, not dynamic Windows proof. The new
pure Rust tests cover tri-state classification and lexical root clearing only;
wrong/reused native identities, borrowed-context reentry, the one native attempt,
healthy long-lived modal routing and actual cleanup/Complete still require
Windows negative/integration execution. No local native acceptance is claimed.


## Read-only existing station-handle diagnostic

The next failure-only clipboard diagnostic queries the existing borrowed current
station handle once with the documented generic NtQueryObject ObjectBasicInformation
contract. The public structure must be exactly 56 bytes, with GrantedAccess at
offset 4; status, returned size and the same nonzero current attached handle
before/after must all validate before reporting the observed mask/bit 4.
Resolution, layout, status or identity failure is explicitly unavailable.
No handle is duplicated, closed, switched or granted rights. The query runs only
after original clipboard admission has already refused and confirmed CloseClipboard;
it does not retry the sequence or admission, inspect clipboard data or change
anything about the zero refusal. A present bit does not explain zero or establish
a guaranteed nonzero initial generation. An absent bit would support the API's
documented missing-access cause, not permission to repair it automatically.

Microsoft documents window-station handles as kernel objects, and the generic
NtQueryObject structure exposes access granted to a handle, but warns that the
API may change or disappear. This is an optional discriminator, not a guaranteed
station-specific or runner clipboard-authority contract.

- [NtQueryObject](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntqueryobject)
- [Kernel objects](https://learn.microsoft.com/en-us/windows/win32/sysinfo/kernel-objects)
- [DuplicateHandle supported object table](https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-duplicatehandle)


## Counted owned-GPU-leaf model (requires native CI acceptance)

The raw popup inventory is genuinely multi-process on this runtime. The proposed
category is not a foreign-PID allowlist or a skipped subtree: it counts exactly
one direct raw Pane leaf, bound to a live retained GPU process in the actual
retained source view's disposable environment. Every Menu, MenuItem and fixture
row ancestor remains browser-owned. No foreign Name/value is read. Optional
control/content/focusable properties remain explicitly unknown when unsupported;
they are not called false or used as noncontrol/noncontent proof.

The Rust test retains browser/GPU process handles with creation/image/liveness
proof, the exact source COM/environment IUnknown identities and original
document generation. Actual Env8 membership and canonical UDF are checked again
after handle acquisition. An optional failure leaves only the unchanged strict
browser-only path, never invented GPU authority.

After the helper's complete first inventory, one create-new nonce-bound request
asks the original Rust STA to re-prove membership/source/process lifetimes. The
single response binds the popup/root runtime/full-inventory digest, original
source generation, environment/process identities and one immutable shared
GetTickCount64 deadline. All COM reads are fenced; missing/replayed/malformed,
unknown or late evidence refuses. The helper then repeats the entire bounded
raw inventory and all final native/process/foreground/capture proofs before the
one Escape. The existing 10-second readiness/5-second disappearance budgets are
not increased. A late publication cannot be made safe by deleting a receipt;
both processes enforce the shared deadline independently. This remains bounded
repeated observation, not atomic revocation or continuous document authority
through Escape: a change after response publication cannot be revoked across
processes. The source proof is fresh before publication; post-publication source
checks can fail the test but cannot retroactively stop a helper already reading
the response. Final native inventory and process/ownership proofs remain required.

The GPU node itself must have supported PID/HWND/Pane/bounds/disabled/unfocused
values, exact native parent/root/root-owner/class/process identity, successful
error-qualified zero-owner/zero-child observations, no raw child, and explicit
absence of ItemContainer/VirtualizedItem patterns. It is fully re-read and
retained in the complete ordered inventory proof; duplicates, any additional
foreign node, descendants, missing getters or changed facts refuse. No
Realize/Expand/Invoke or other tree mutation is permitted to obtain acceptance.

Synthetic Rust packet/membership/publication policy tests and pure managed
native/UIA-shape/receipt refusal tables are labeled synthetic. They cover wrong
identity/shape/type/bounds/parent/child/pattern fields, duplicate/unknown packets,
nonce/generation/root/digest/process changes, oversized/truncated/replayed
receipts and no-overwrite publication. The managed tests call the real SameProof
and digest predicates for changed ordered inventory/count/parent/runtime/PID/HWND/
type/bounds/GPU proof, plus native/root/fixture geometry fields. The Rust synthetic
step reuses the exact no-feature native-build executable, never creates a newer
feature variant for subsequent probes. They do not simulate genuine process or
UIA authority. Both real native observation/cancellation lanes still must
establish actual visible fixture rows, Escape/disappearance, same source/worker,
zero events and unchanged native permission denials. Broader dynamic reentry,
process crash and native identity-reuse races remain unclaimed until exercised.

The preceding attached-handle diagnostic ran successfully at head03d29a4:
NTSTATUS0, size56, same borrowed handle, mask983935 (0xF037F), bit4 present,
while sequence0 still refused before input/mutation. Missing bit4 on the actual
attachment is not supported by that run; undocumented initial-zero semantics
remain unresolved. The separate original App Cancel lane did not establish its
primary document before teardown; an unbounded message drain is a latent harness defect, not a
proven cause of that timeout. Reload still passed. No repeated unchanged
clipboard run or permission/clipboard workaround is introduced by this model.


## Original App startup pump correction

The retained head03d29a4 original-App failure does not establish a 60-second
controller failure: its unbounded message drain could bypass both readiness and
deadline until the existing 180-second timer posted quit. No concrete perpetual
producer was observed, so that is not claimed as the failure's established cause.
The test-only nested pump now processes at most32 messages per turn, checks its
unchanged monotonic timeout before/after readiness and dispatch, rejects late true,
and reposts WM_QUIT exactly once even after expiry. PeekMessage itself may dispatch
sent messages: an empty late return refuses; a removed non-quit message is completed
once before refusal, never discarded or arbitrarily reposted. No new readiness poll
or next message starts after that late return. It does not add time, input or a retry.
A blocking native or
COM dispatch cannot be preempted by this acceptance deadline.

A one-shot, caught failure-only startup log records normalized phase/exit cause,
elapsed/budget, polls, removed/dispatched message counts/batches and last message
ID, active readiness/PeekMessage/dispatch and their current/maximum durations, App borrow availability/engine presence/active-tab match, and the
existing successful-document handles/proof/fence availability. It runs at the
initial failure boundary before teardown, or before the existing180-second timer
closes a still-pending initial phase. No App/Core guard spans the closure calls,
no COM/native input is added, and no raw parameters/URLs/titles/pointers are logged.
The existing handles getter requires successful document evidence; absence must
not be described as a missing controller. A diagnostic failure cannot change the
original acceptance or assertion. Source/pure tests do not establish the actual
startup cause or exercise native message starvation. CI explicitly discovers and
runs the two pure Rust decisions in the already-used experimental feature set
before the live App lanes; the usual default/shipping suites alone cannot discover
tests behind this default-off feature.


## Nonfinite raw-inventory refusal diagnostic (no exception added)

At head9de75e5, both real menu lanes reached one right-click but refused the first
complete raw inventory with `Nonfinite raw inventory bounds`; neither sent Escape.
The typed GPU path would reject nonfinite bounds earlier, so the unwrapped error
points to the observed browser-PID inventory branch, not a proven GPU leaf. Its
final node/root fence had not run, so stable ownership is not established.

Microsoft documents both a default Empty and a supported Empty for a non-displaying
UIA element. System.Windows.Rect.Empty uses positive-infinite X/Y and negative-
infinite width/height. This does not identify our failed node. The next increment
only writes one bounded failure-only identity/bounds receipt: already observed
PID/runtime/type/HWND/depth/parent and box are separate from a later no-default
bounds query, with root fences and separately labeled node/parent identity readings.
The diagnostic explicitly distinguishes the canonical four-value Empty shape from
other NaN/infinity; output is create-new and capped at8192 characters. Unsupported, wrong-kind and getter-failure
states stay unknown. Nonfinite values are explicit strings, never numeric zero.
The original nonfinite throw, GPU/fixture finite checks, deadlines and no-Escape
outcome remain unchanged. No supported-empty policy exception is introduced.

- [UIA BoundingRectangle property](https://learn.microsoft.com/en-us/dotnet/api/system.windows.automation.automationelement.boundingrectangleproperty?view=windowsdesktop-10.0)
- [System.Windows.Rect.Empty](https://learn.microsoft.com/en-us/dotnet/api/system.windows.rect.empty?view=windowsdesktop-10.0)


## Host-lane reuse of the bounded test pump

On mergead33617/tree939e218, the independent note/Split phase receipt measured
240159ms inside its nominal60-second startup pump, ready=false, before the existing
240-second watchdog caused teardown. This establishes an unenforced acceptance
boundary, not an endless queued-message producer or a particular native callback.
The host live module now imports the already-reviewed, test-only bounded pump
instead of keeping a duplicate unbounded drain/late-ready fallback. All original
60/240-second budgets, phase receipts, native assertions and clipboard0 refusal
remain. The same two explicitly wired pure decisions cover the shared code;
real note/Split and Reload lanes remain required regression checks.


## Typed unsupported browser-structure representation (native CI pending)

The exact mergead33617/tree939e218 receipts identify a browser-PID Pane at depth4
in both lanes. Its defaulting outer HWND is0 and box is canonical Empty, while
no-default HWND and bounds are both NotSupported. Stable numeric runtime/parent,
PID/type and root readings do not turn those unavailable properties into values.

The narrow representation counts and traverses a non-root browser Pane with that
exact pair as explicitly unknown, after supported PID/type/runtime/parent proof
and read-only absence of ItemContainer/VirtualizedItem. It makes no leaf,
invisibility, noncontrol or no-HWND claim. A typed immutable witness binds outer
defaults and no-default tags; every browser node and fixture ancestor resolves to
its counted witness and is freshly checked. Known nonzero native-ancestor rules
remain. Root/Menu/MenuItem/GPU/fixture geometry never uses this unknown category;
supported Empty, arbitrary nonfinite, unsupported bounds with supported HWND,
wrong kind, missing getter or changed availability refuses. Full inventory/digest/SameProof retain
the actual kind/value tags so unknown-to-known changes cannot be hidden by0.

Pure matrices exercise the real witness, serializer, ancestry lookup and proof
comparison. They are not real UIA/provider race or permission acceptance. Both
real menu lanes still require the existing one-shot membership response, full
second inventory, shared deadline and exact native Escape/disappearance proof.


## Complete inventory evidence retention

For independent row-by-row audit, retain both already-captured complete inventory
strings, not only their hash. Reports label first/fresh phase, completeness,
count/GPU count, original nonce/source/popup/root identities, exact UTF-8 SHA256
and the issued membership-request digest when available. Missing, malformed or
incomplete captures remain unavailable, never empty success. Unknown tags remain
literal tags; the data contains no provider Name/content or process image path.

These strings are only attached/serialized after proven menu release or confirmed
exact test-process failure cleanup. There are no additional provider/native reads,
pre-Escape disk writes, retries, deadline changes or truncation. Pure PS5.1 checks
round-trip JSON including controls/Unicode and independently recompute UTF-8 SHA256.
A bounded menu-only artifact is saved before later independent App regressions so
failure evidence is not held back by unrelated startup timeouts. Neither artifact
creation nor a complete evidence record is an input-authority or acceptance verdict.

## Independent browser-Pane property availability

The head49fb450 integration run checked out merge21c2323e6cbc3420a75582e4a23d1d0d2fab02e0,
tree d9d7581fed00283a32d8b9f68bdc08820b15b591, not the branch-only tree. Both menu
receipts now stop at a distinct browser-owned non-root Pane: no-default bounds are
supported finite (181,81,423,400), while HWND is explicitly NotSupported and its
outer default is0. Runtime/PID/type/parent and both root fences were stable.
Neither helper sent Escape; this is not GPU-leaf or native cancellation acceptance.

The typed witness now tracks NativeUnknown and BoundsUnknown independently. In
addition to the prior double-unknown Pane, only a non-root browser Pane may have
unsupported HWND with supported finite non-empty bounds. Its outer HWND must
equal the documented default0 and its outer bounds the exact typed rectangle;
this consistency does not prove native absence, non-interactivity or invisibility.
Read-only pattern absence, complete child traversal, exact counted ancestry,
fresh immutable witness comparison and the original deadlines all remain.

Root, Menu, MenuItem, GPU and fixture geometry do not use this category. Supported
HWND with unavailable bounds, nonfinite/empty mixed bounds, null/wrong-kind/getter
failure, changed tags or outer mismatch still refuse. Both unknown categories
cannot be classified as command rows or supply fixture geometry. Pure matrices
exercise these rules plus mixed-to-double-unknown/supported-zero Signature,
SameProof and digest changes. Windows/native execution remains required.

- [UIA NativeWindowHandle property](https://learn.microsoft.com/en-us/dotnet/api/system.windows.automation.automationelement.nativewindowhandleproperty)
- [UIA BoundingRectangle property](https://learn.microsoft.com/en-us/dotnet/api/system.windows.automation.automationelement.boundingrectangleproperty)

## Bounded host fixture transport and script-stage diagnostics

In the head49fb450 integration tree21c2323e, the host Copy/Paste lane stopped at
an unlabeled COM script wait after240s, before helper/clipboard admission. Its
first source snapshot is strongly implicated by one ordinary controller and no
negative/per-mode evidence, but ExecuteScript, PeekMessage sent-message work and
one dispatched callback cannot be distinguished from that log. Fixed per-call
labels/ordinals now retain ExecuteScript-entered/returned, synchronous callback
status, pump-entered/exited and the existing primitive pump pulse. Failure and
the unchanged240s watchdog report at most four bounded lines, without new scripts,
source contents, pointers, input or App/Core borrows. A non-pumping blocked call
can prevent the watchdog; absent diagnostics do not establish where it blocked.

The same integration Note lane proved source readiness in2154ms and genuine native
Note selection/Complete/cleanup with the exact trusted DOM effect, but no exact
loopback acknowledgment. The previous server made one read and assumed a complete
GET, then ignored response-write failure. TCP does not promise a full request in
one read. This is a confirmed harness defect, not a diagnosed cause of that run.

The separate test-only server now collects complete CRLFCRLF headers across partial
reads within8192 bytes, strictly frames one fixture GET and rejects malformed,
duplicate-key, incomplete and oversized requests. Explicit read/write loops share
one original1s accepted-connection monotonic budget, recompute remaining time before
each syscall and refuse late final results. No hidden write_all/per-call budget reset,
request retry, clipboard operation, permission change or deadline increase exists.
Valid effects remain separate from timely full responses; both counters must match
the expected effects, and the original native/source/exact-body DOM acknowledgment
is still mandatory. Counter visibility races may fail closed, never repair a pass.

After sequence success/unwind, a create-new bounded receipt retains only classified
paths, boolean nonce/kind/value matches, transport outcomes and counters from up to32
connections; no raw request/value/body is logged. The fixture's existing snapshot
adds fixed fetch started/status/exact-body-match/error-name states without retries.
Four synthetic Rust fake-transport/clock tests exercise the actual bounded loops,
and two synthetic script-state tests preserve synchronous/old callback identity.
CI explicitly discovers/runs them with the existing experimental feature variant.
The local Node fixture checks include exact response, wrong body and rejection.
These checks do not prove real socket, COM or native-menu behavior; Windows CI and
the still-independent Reload/native safety gates remain required.

## Rejected popup property-shape diagnostic, not a new acceptance category

The next integration run (branch2c2c449, actual checkout63d8e43998565d42339cd22a326577553ade6e31,
tree8c49bd4feb78d7a306bf168249b5f6d72b7d0a44) passes the mixed Pane representation
and then refuses browser MenuBar50010 with supported finite bounds/unsupported HWND.
Both lanes still send no Escape. No MenuBar/Menu/MenuItem policy is inferred.

After the original browser-witness error is selected, a separate best-effort
collector buffers a non-atomic rejected-probe schema rooted only at the captured
popup. It always rethrows the original error. Numeric/runtime/type/property tags,
first/later readings and explicit coverage/stop reasons are separate from accepted
first/fresh inventories and membership hashes. No Name/text/value/image path,
pattern action, native input, reacquisition, acceptance retry or new permission
exists. The buffer is attached/written only after exact failed-test-process cleanup.

The collector has a750ms cooperative budget capped by both remaining original
readiness and ownership deadlines, with checks around each provider call. Blocking
UIA cannot be preempted. Existing128-node/queue/depth,32-runtime-unit and65536-character
caps remain; budget/cap/getter/identity failure makes output explicitly partial.
Only exact supported browser-PID nodes are descended into. Foreign/unknown-PID
boundaries are recorded but their subtrees are excluded; sibling advancement is
allowed only inside the exact re-proved browser parent's child list. Changed/missing
parent, runtime cycle or root/process mismatch stops observation. Root never has
an above-root parent query. A complete diagnostic traversal is never acceptance.

Pure managed tests exercise the actual caps/cycle/parent/getter/budget/serializer
predicates, including unavailable values and nonfinite coordinates with explicit
shape tags. Source checks preserve unconditional original refusal and post-cleanup
attachment. Actual PS5.1 compilation/provider behavior remains pending Windows CI.
