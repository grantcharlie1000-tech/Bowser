# Root-associated Menu and structural MenuBar witnesses in strict native tests

Both strict Chromium Views lanes reuse `probe-default-context-cancellation.ps1`.
The added category is only a non-root, exact browser-PID `ControlType.MenuBar`
whose no-default HWND value is explicitly `AutomationElement.NotSupported`,
whose supported bounds are finite, positive and contained in the exact native
popup, and whose ItemContainer and VirtualizedItem patterns are positively absent.
Its HWND remains tagged `not-supported`; it is not a supported zero, Menu,
fixture MenuItem, noninteractive element, invisible element or childless leaf.
Unknown MenuItem geometry remains refused. The separately gated Menu category below
does not broaden MenuBar or fixture-row eligibility.

Every admitted unknown-HWND MenuBar gets its own fresh bounded raw-parent proof,
even when it is not on the fixture item's path. The same chain implementation
also proves fixture ancestry. It resolves every node and edge to the already
counted immutable browser inventory, rechecks typed properties and outer/default
consistency, enforces exact parent/depth progression and finishes at the captured
`AutomationElement.FromHandle(popup)` root. Missing, stale, foreign, duplicate,
cyclic, skipped, reparented or unexpected supported nonzero native nodes refuse.
The chain never reads above that root or treats runtime-ID prefixes as ownership.

## Separate unknown-HWND Menu eligibility

A non-root exact-browser `ControlType.Menu` may have the same explicitly unsupported
HWND, supported finite positive exact-popup-contained bounds and positively absent
ItemContainer/VirtualizedItem patterns. Typed construction retains a distinct
pending-root signature. It alone is never eligible for the unique-Menu count.
`IsBrowserRow` and `FixtureBounds` remain unchanged and reject unknown-HWND fixture
MenuItems. A Menu cannot stand in for the exact enabled, onscreen, raw leaf row.

Each `ReadMenu` creates a new invocation-local, non-reusable reference identity.
Only after the shared `CountedBrowserAncestry` succeeds from the caller's original
immutable `expectedStart` can that invocation register rooted Menu eligibility.
The registered witness binds the exact Menu signature, expected start, root runtime,
popup and the full counted chain. A bare boolean, constructed node, unregistered
witness or another invocation's token cannot count. Every unknown Menu without
such eligibility throws for the whole inventory, including beside a valid sibling.

The shared walk now returns immutable path witnesses as well as its Menu count.
Fixture ancestry must identify exactly one Menu and match the same eligible Menu
selected for `MenuId`, its root/popup and its exact Menu-to-root chain. Merely finding
one raw `ControlType.Menu` somewhere on the fixture path is insufficient.

The inventory serializes only stable semantic fields: exact popup/root/start/Menu
signature plus SHA-256 of the length-framed immutable Menu-to-root signature
sequence. The local reference identity is never serialized, stored on `MenuProof`
or reused by the fresh full inventory. The fresh `ReadMenu` independently issues
its own eligibility; equal complete observations produce equal inventory/digest.
Rooted-proof, ancestor availability, parent, runtime, PID, type, HWND or geometry
changes either refuse outright or change the complete inventory/digest.

Each fresh chain-node/edge callback is fenced before and after by the retained
browser/root/process checks and the original absolute readiness and ownership
deadlines. No deadline is reset, widened or retried. These are cooperative checks:
a blocked native/provider call cannot be hard-preempted.

All descendants remain in the complete inventory. Existing limits remain 128 raw
nodes, 32 ancestry nodes and 65,536 inventory characters. The exact supported
fixture geometry, one Menu/one fixture count, fresh complete-inventory comparison,
SHA-256 digest binding and final native focus/capture/source/process/deadline
checks before the sole Escape remain required.

This is bounded provider-observation evidence, not an atomic lifetime guarantee,
malicious-provider resistance or proof that every hidden native boundary is
absent. The existing supported-HWND-zero path has the same limitation. There is
no MSAA or reflection fallback.

## Validation scope

`test-native-menu-helpers.ps1` already invokes `CheckBrowserStructurePolicy`, which
now also invokes the synthetic MenuBar and rooted-Menu matrices. Invented typed inputs exercise the
actual witness, absent-pattern, shared ancestry, absolute-budget, unique-fixture,
complete-inventory, SameProof and digest predicates. Cases cover unavailable or
wrong typed fields, root/foreign nodes, malformed/outside bounds, tag changes,
virtualization/getter/fence failures, missing/skipped/cyclic/reparented chains,
stale roots/runtime/depth, the ancestry cap and inventory add/remove/order changes.
Rooted-Menu cases additionally cover construction without proof, foreign-invocation
reuse, same-process wrong root/popup, unproved Menu plus valid sibling, a fixture
path naming another eligible Menu, stale chain binding, independent fresh issuance,
the nine expected-start substitutions and every before/after/issuance fence.
These tests perform no provider queries, process/window reads or input and do not
simulate successful native acceptance or a provider race.

Portable source contracts also pin the unchanged final input boundary and both
lanes' use of the shared helper. Run them with:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest benchmarks/test-*.py -q
node benchmarks/scripts/validate-app-host-menu-fixture.mjs
```

Windows PowerShell managed compilation and the pure matrix require a separately
reviewed Windows run of `benchmarks/scripts/test-native-menu-helpers.ps1`. Portable
source checks are not a substitute for that compilation or either real native
observation/cancellation lane.
