# Initialized Copy known-format candidate: local-only, not native acceptance

Status: prepared locally for review. This implementation phase has not included
external publication, CI dispatch, Windows/native clipboard execution, production
changes, merge, release, deployment, locale/settings changes, or access-permission
changes. Local commits used to freeze review snapshots are not external publication.

## Evidence and hypothesis boundary

The isolated candidate starts at commit
`f0ffd48949d882f3dfd243161ca53ff8284e2e42`, tree
`1679225c7409244490bc6e1641a52b59aef72d1c`. This is the supplied tree equivalent
of the actual tested/public snapshots; this note does not independently attest to
commit objects absent from the local repository.

The inspected receipt from artifact `12305305281` of Store run `39508962545`,
`store-initialized-empty-copy/copy/app-host-context-menu.json`, records a final
pre-Close epoch of 3, confirmed Close, and a later epoch of 6. Both independent
snapshots report the exact live helper. Actual RequireOwned refused before either
physical input, and cleanup was disarmed with zero clear attempts. These separate
observations identify neither a cause nor a writer, and prove no continuous
custody. This candidate must still refuse that exact sequence mismatch.

Option A is a bounded fixture-construction hypothesis: explicitly install the
known text-conversion family instead of relying on conversion synthesis. It keeps
the generated text sentinel and text/Paste availability semantics. It does not
claim that four formats guarantee post-Close epoch stability. Option B (registered
opaque marker) is not implemented, registered, or used as a fallback.

## Construction and authority

Only the existing opt-in initialized-empty Copy lane reaches this installer. The
contract/ready/one-shot guard claim is shared with synthetic checks. The ordinary
ClipboardGuard class, including its separate cleanup gap and zero refusal, remains
byte-identical. Its frozen SHA-256 is
`fa9f96064e086c6f741a4da920137e067e57ac189c22bcd7ad7a8896c2322a59`.

The bounded token grammar is `\Asentinel-[0-9]+-[0-9]+\z`, capped at 128 ASCII
bytes before terminators. A final newline, embedded NUL, non-ASCII character,
wrong prefix, or overlong token is refused before any clipboard operation.

The fixed ordered payloads are:

- CF_UNICODETEXT (13): exact generated ASCII token encoded UTF-16LE plus two zero
  bytes. High bytes and terminal bytes are constructed explicitly, not by a lossy
  or locale-dependent encoder.
- CF_LOCALE (16): exactly `09 04 00 00`, the 32-bit en-US LCID 0x00000409.
- CF_TEXT (1) and CF_OEMTEXT (7): equal ASCII-plus-NUL payloads in separate buffers.

Four distinct `GMEM_MOVEABLE | GMEM_ZEROINIT` allocations are filled and fully
unlocked before Open. Each newly allocated buffer has exactly one owned lock;
GlobalUnlock must return FALSE with immediate last-error zero. Allocation padding
stays zero-initialized. The existing production Copy-reader unlock policy is not
changed by this stricter fixture-preparation rule.

There is one independent ordinary admission, one Empty attempt, four eager
non-NULL setter attempts in the success case, and one Close attempt per successful
Open. The live lexical helper is proved before Empty, and exact live helper owner
checks follow Empty, precede every setter, and surround the final nonzero sequence
observation while open.
The sole final nonzero candidate epoch is captured only after all four setters.
Existing before/after-Close diagnostics remain read-only and cannot update last,
OwnedSequences, or cleanup authority. Cleanup may arm only with that retained
pre-Close epoch after confirmed Close.

Every non-NULL setter return marks its buffer transferred immediately, before
receipt bookkeeping. Transferred buffers are never written or freed. A throwing
setter has uncertain ownership and is not treated as caller-owned. Other created,
untransferred buffers are freed at most once, continuing through another free's
failure. GlobalFree NULL means success. A partial install, unknown owner/helper,
zero final epoch, or unconfirmed Close disarms cleanup: no clipboard-clear repair,
setter retry, fallback, changed-epoch adoption, or input.

Both existing physical-input sites still call RequireOwned independently. The live
guard and synthetic checks now use the same shared open/sequence/exact-owner/Close
control flow. Any sequence mismatch (including 3 to 6 with the same live helper),
zero, owner mismatch/error, helper PID/TID/class/token/liveness failure, lock, or
Close failure refuses and disarms cleanup before that site's input. A successful
right-click admission cannot license later selection.

Four fixture setters are reported separately from production Copy. Matching
production completion still requires exactly one successful production setter;
ObserveNativeCopy, completion validation, bounded native content verification,
initializer behavior and latest-exact-owner cleanup control flow are byte-frozen.
No new clipboard content getter or new-format enumeration was introduced.

## Validation scope

Portable checks completed locally:

- `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest benchmarks/test-*.py -q`: 148 pass.
- `node benchmarks/scripts/validate-app-host-menu-fixture.mjs`: pass.
- `git diff --check`: pass.

The existing managed CheckInitializedClipboardPolicy entrypoint now calls the
actual shared known-format installer/RequireOwned matrix before any live probe.
The matrix covers design cases 1-14: gate and one-shot failures; malformed token,
exact locale/encoding/terminator and shared-handle failures; each of four
allocation/lock/fill/unlock/setter failure positions; admission/owner/helper/epoch
failures; one-close accounting; transfer-before-bookkeeping exception; uncertain
setter ownership; failed free continuing other frees; zero allocation padding;
helper loss before each setter; the exact pre3/Close/post6 refusal; arbitrary and
zero changed epochs; and each negative independently at initial and second input.
The existing native Copy/content/process/completion and cleanup negatives remain
unchanged. Design case 15 concerns option B and is inapplicable because B is absent.

PowerShell 5.1 / C# 5 compilation and managed synthetic execution: NOT RUN here.
No PowerShell or C# compiler is installed, and none was installed for this task.
Windows/native clipboard, input, Copy completion, real content and cleanup: NOT RUN.
Portable source contracts are not substitutes for those executions or native
acceptance. Exact-source review is required before any separately authorized
Windows/CI run or publication.
