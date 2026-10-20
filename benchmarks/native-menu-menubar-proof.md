# Structural MenuBar witness in strict native menu tests

Both strict Chromium Views lanes reuse `probe-default-context-cancellation.ps1`.
The added category is only a non-root, exact browser-PID `ControlType.MenuBar`
whose no-default HWND value is explicitly `AutomationElement.NotSupported`,
whose supported bounds are finite, positive and contained in the exact native
popup, and whose ItemContainer and VirtualizedItem patterns are positively absent.
Its HWND remains tagged `not-supported`; it is not a supported zero, Menu,
fixture MenuItem, noninteractive element, invisible element or childless leaf.
Unknown Menu and MenuItem geometry remains refused.

Every admitted unknown-HWND MenuBar gets its own fresh bounded raw-parent proof,
even when it is not on the fixture item's path. The same chain implementation
also proves fixture ancestry. It resolves every node and edge to the already
counted immutable browser inventory, rechecks typed properties and outer/default
consistency, enforces exact parent/depth progression and finishes at the captured
`AutomationElement.FromHandle(popup)` root. Missing, stale, foreign, duplicate,
cyclic, skipped, reparented or unexpected supported nonzero native nodes refuse.
The chain never reads above that root or treats runtime-ID prefixes as ownership.

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
now also invokes the synthetic MenuBar matrix. Invented typed inputs exercise the
actual witness, absent-pattern, shared ancestry, absolute-budget, unique-fixture,
complete-inventory, SameProof and digest predicates. Cases cover unavailable or
wrong typed fields, root/foreign nodes, malformed/outside bounds, tag changes,
virtualization/getter/fence failures, missing/skipped/cyclic/reparented chains,
stale roots/runtime/depth, the ancestry cap and inventory add/remove/order changes.
These tests perform no provider queries, process/window reads or input and do not
simulate successful native acceptance or a provider race.

Portable source contracts also pin the unchanged final input boundary and both
lanes' use of the shared helper. Run them with:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest benchmarks/test-native-safety-source.py -q
```

Windows PowerShell managed compilation and the pure matrix require a separately
reviewed Windows run of `benchmarks/scripts/test-native-menu-helpers.ps1`. Portable
source checks are not a substitute for that compilation or either real native
observation/cancellation lane.
