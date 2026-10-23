# Rooted browser MenuItem proof: local contract checks

This change extends only the browser-PID unknown-HWND MenuItem category in the
bounded raw UIA inventory. It does not authorize unknown-HWND Separators, generic
unknown controls, virtualization, command activation, or broader native input.
An unsupported HWND remains `not-supported`, not a supported integer zero.

## Ownership is not fixture authority

Every counted browser MenuItem needs typed, no-default Name availability and
Boolean enabled/offscreen facts. The only retained Name information is one of
two fixed classifications: exact ordinal fixture-name match or supported
nonmatch. Neither arbitrary labels nor their hashes belong in accepted rows.
Changing one supported nonmatching label to another is intentionally not claimed
as text stability; only its supported-nonmatch classification is retained.
The required Name string is capped at 4096 characters after the provider returns
it. That bound neither preempts a blocked provider call nor prevents the provider
from allocating a larger string before the probe can reject it.

Every browser MenuItem, including supported-HWND items, also needs its own
registered, invocation-local rooted item token. This deliberately adds stricter
typed-state and ancestry requirements without tightening supported nonfixture
geometry to the fixture's geometry rule. The token binds the original immutable start, exact browser lifetime,
popup/root, exact eligible Menu, and the successful full counted ancestry chain.
It is distinct from a rooted Menu token. Only the exact pending starting item
can provisionally enter its own proof; an unproved item is not an ancestor.

Nonmatching owned items can have children. Their ordered raw direct-child edges
must remain immutable and agree with every eventually counted descendant.
Ownership alone does not establish complete subtree coverage. Only the fixed
fixture match additionally needs enabled=true, offscreen=false, observed raw
leaf absence, and supported positive finite popup-contained geometry.

## Complete inventory is the serialization boundary

The bounded breadth-first walk buffers rows instead of publishing a prefix as
an accepted inventory. All queued descendants, parent/depth progressions, and
ordered child edges must close before serialization. The serializer first
validates every row's registry/category signature, then emits the complete rows;
a late missing item token cannot even produce a serialized local prefix. An
accepted MenuProof additionally requires the exact unique Menu/fixture counts
and final root/process/native fences. Rechecks happen around ownership and
before item sealing. The independent fresh invocation must issue new tokens and
yield equal semantic inventory data, not equal token object identities.

The existing final native/source/process/deadline checks and the sole Escape
decision remain separate prerequisites. A source-contract pass is not evidence
that any live MenuItem, UIA provider, popup, or native cancellation passed them.

## Microsoft facts and the policy boundary

- The [no-default property overload](https://learn.microsoft.com/en-us/dotnet/api/system.windows.automation.automationelement.getcurrentpropertyvalue?view=windowsdesktop-9.0)
  can retain NotSupported rather than a fallback value. A current property read
  can change after the call, so repeated observations are not an atomic snapshot.
- [NativeWindowHandle](https://learn.microsoft.com/en-us/dotnet/api/system.windows.automation.automationelement.automationelementinformation.nativewindowhandle?view=windowsdesktop-9.0)
  documents zero when there is no element window. This does not make an observed
  NotSupported value equivalent to supported zero.
- The [MenuItem control type](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-supportmenuitemcontroltype)
  can represent commands, options, or submenus. Its type alone establishes
  neither raw leafness nor authorization to invoke, select, or expand anything.
- The [property identifier reference](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-automation-element-propids)
  describes string Name and Boolean enabled/offscreen. Names are not unique;
  offscreen=false does not establish lack of occlusion.
- [Runtime IDs](https://learn.microsoft.com/en-us/dotnet/api/system.windows.automation.automationelement.getruntimeid?view=windowsdesktop-9.0)
  are opaque comparison values and can be reused over time, not ownership
  prefixes or evidence of a command's role.

The exact registered ancestry route is a Bowser safety-policy inference, not a
Microsoft guarantee of native ownership. It relies on the UIA provider's identity,
navigation, and typed property observations inside retained process/root fences;
it does not authenticate a malicious provider or make cross-process reads atomic.

## What the portable suite establishes

Run `python3 benchmarks/test-rooted-menuitem-source.py`. It reads the actual
embedded C# declarations and live call sites. Its balanced-body extractor masks
comments and literals for executable-order assertions, so explanatory text and
refusal messages cannot stand in for a required call or guard.

The existing CI source-test entrypoint, `benchmarks/test-native-safety-source.py`,
loads this suite explicitly through `load_tests`. No workflow or feature variant
changes are needed. Running both files separately executes these source checks
twice; duplicated executions are not additional distinct coverage or a CI result.

The source contracts check placement and binding of typed facts, exact pending
membership, distinct item-token registration, full-chain checks, immutable raw
child edges, deferred serialization, and wiring of the managed synthetic cases.
They do not execute a detached Boolean copy of the final Escape condition.

The managed item cases are wired through the existing structure-policy
entrypoint. Their invented state feeds the actual item constructor/issuer,
registry, raw child observer, immutable buffered rows, closure validator,
serializer, fixture predicate, and equality/digest path. The portable suite
checks that wiring and the stated case categories; it does not execute those
managed cases or establish dynamic provider-race coverage.

The existing `benchmarks/scripts/test-native-menu-helpers.ps1` can compile and
exercise managed synthetic policy entrypoints under Windows PowerShell. Its
presence or source wiring does not mean it ran here. Portable source tests,
managed synthetic execution, and real native execution are three different
evidence levels. Report each separately; managed compilation and native input
remain NOT_RUN unless an actual run establishes otherwise.

## Evidence still needed for native acceptance

A real accepted run must produce both complete first/fresh inventory receipts
and equal digests, exact fixed-match/leaf facts and all-node coverage, unchanged
final native/source/deadline fences, actual Escape/disappearance/release, and no
selection. Rejected-node diagnostics remain failure-only and cannot fill a
missing proof. Supported fixture and rooted-Menu behavior must retain their
existing synthetic and native coverage where available.
