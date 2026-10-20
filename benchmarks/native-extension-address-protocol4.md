# Native extension address readiness, protocol 4

Protocol 4 adds a distinct before-binding observation phase after primary
startup/resource capture, inside the existing conditioned navigation clock.
It does not diagnose or rehabilitate prior failed protocol-2/3 series. Its
additional UIA conditioning is not comparable to those older workloads and is
not browser-only navigation latency. There is still no valid complete resource
series established by this source change.

## Evidence and boundary

The protocol-3 artifact ZIP with SHA-256
`cbc112dcaa799028631c4551c7347a7a4f819071e55a9f8ce710e4a7113d91f2`
reported source `3742c516afd923fec114d68019f8b433e45cca0b`. The initial
several-extension observation found three outermost owned Documents, with
complete agreeing managed/native sources classified only as `other`. Later
failure-only Name/Edit/Document queries found one exact Edit and four Documents.
There were no per-query timestamps or later source/runtime-ID observations to
identify that fourth Document. This supports an exposure hypothesis only.

## Finite before-binding state machine

The monotonic clock starts before the native owner/root latch. Four observation
schedule points are fixed at 0, 1,000, 2,000 and 3,000 ms. An overrun may consume
an already-due schedule point immediately, but grants no extra observations or
time. Any elapsed value at or above 10,000 ms rejects, including a provider
return or the final strict binding that arrives late. Synchronous UIA calls
cannot be interrupted by this budget; they may block beyond it, but cannot
produce an accepted late result.

The original PID, HWND, thread and root runtime ID are fixed before observation
and rechecked on both sides of every complete observation. Each source must be
present, bounded, exact managed/native agreement within the original owned root.
All observed Documents must be outermost; missing data, excluded scope, getter
failure, duplicate IDs, ambiguity and truncation fail without retry. Exact
source strings remain only in private state. Every previously seen Document ID
must remain present with its identical source; disappearance or source change
fails. Zero exact toolbar candidates can be `PendingComplete` only here. The
initial snapshot may already be `Ready`; after a pending snapshot only a newly
appearing ID with the exact toolbar source can become `Ready`.

Every started observation is retained on success or failure, including the
first zero-candidate snapshot, scheduled/start/completion times, completion and
failure flags, classes and bounded opaque IDs. No raw URLs, names or errors are
added to these receipts. Four complete zero-candidate snapshots exhaust the
schedule and remain failed, rather than restarting the lane.

After `Ready`, the existing strict address/Edit validation runs exactly once.
Its returned native/root/Document identities must still equal the readiness
latch, and its original Edit identity becomes the unchanged input binding.
Getter, source, focus, geometry or identity failure cannot trigger another
binding, fallback, key repair or retry. The strict reader, post-binding keyboard
lane, native declarations and the complete measurement/hash/accounting/cleanup
tail are byte-identical to the base checkpoint.

## Verification limits

The Python checks are static source/wiring contracts only. The PowerShell
contract extracts the real transition and final binding functions and executes
initial-ready, pending-to-new-ready, source/ID change, disappearance, duplicates,
missing/conflicting source, ambiguity, expiration, exhaustion and one-shot
binding cases using generated values and stubbed I/O. These are not native UIA
evidence. New Windows parsing/generated contracts and real provider/runtime
behavior remain unrun locally; a source audit and Windows run are still required.
