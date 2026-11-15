# Supplied performance evidence checker

This independent Python standard-library tool checks the supplied measurement
bundle against the 2026-11-15 parent contract. It executes no application, reads
no browser profile, starts no server, and collects no performance measurements.
It does not inspect the pixels, sound, input behavior, or contents at an evidence
reference. A path and SHA-256 preserve provenance; they do not establish visual
proof. Parent reviews must establish the truth and completeness of those claims.
Global performance acceptance remains a parent/council decision.

Run with Python 3.10 or later, from the repository root:

```powershell
python -B benchmarks/acceptance/acceptance.py supplied-evidence.json --json report.json --html report.html
python -B -m unittest discover -s benchmarks/acceptance -p test_acceptance.py -v
```

The CLI prints a short JSON summary. Exit 0 means complete supplied timing and
review evidence supports parent review; it never grants global acceptance.
Exit 1 means invalid, exploratory, insufficient, missed, synthetic, or outstanding
evidence; reports are still written. Exit 2 means an unreadable/malformed JSON
input, duplicate JSON key, unsafe output alias, or output I/O failure. Existing
output files can be overwritten; input/output paths must be distinct. HTML is
self contained, escaped, and has no scripts or external resources. The full JSON
and HTML preserve all supplied attempts, failures, nonfinite-value diagnostics,
evidence references, reviews and provenance. Legacy production logs without a
version-1 plan remain `insufficient`; malformed version-1 bundles are `invalid`.
There is deliberately no converter or automatic promotion to proof. Unknown
synthetic provenance is reported as null rather than invented.

## Inventory and condition scope

`catalog.json` freezes the parent workload document's SHA-256 and the baseline
source commit. Workload keys are `area/variant/site`. The catalog is checked
independently of the input plan: omitting an area creates explicit missing rows.
Conversation variants each require both `synthetic` and `chatgpt.com` coverage.
Synthetic-site coverage alone cannot establish real-site responsiveness.

| Area | Required distinct timed variants |
| --- | --- |
| Startup | Fresh profile; reused profile with all owned processes exited |
| New tab | First use; reused host; configured web page |
| Retained wake | Confirmed successful suspension, same retained document |
| Discarded wake | New controller/document at the expected URL |
| Page load | First visit; cached visit; redirect; history navigation |
| Search | Local suggestions; remote suggestions; Enter to usable results; full new-tab-to-results |
| Settings | Open section; filter; reversible setting with persistence |
| Conversation typing | After idle; after wake; while chunks arrive |
| Conversation scrolling | Presented movement; fixed transcript target; stream anchoring |
| Open conversation | Correct transcript and usable composer |
| Switch conversation | Correct transcript, preserved independent drafts and usable composer |
| Reply streaming | Chunk to visible text; final text and completion |
| Media | First video frame; first audio sound; video seek; audio seek |
| Downloads | Initiation and durable completion, separately for ordinary, redirect, collision and resume cases |

The table expands the parent prose into explicit endpoints; it does not assign a
50% task-time threshold to resource counters, stall counts or correctness checks.
Stalls, dropped frames, delivery cadence, transfer throughput, disk completion,
and server/renderer/extension/host cost attribution have separate diagnostic
reviews. Cancellation/resume, exact download hash/size, drafts, per-character and
caret correctness, stream anchoring and preservation protections have separate
parity reviews. All listed failure/recovery scenarios are stability gates.
The final-source/evidence council gate is an attestation that the parent checked
every required independent score (at least 9.2/10) and unresolved blocker; this
tool does not convene or independently validate council members.

Each plan condition chooses one workload key and explicit matching conditions.
Add separate condition IDs for additional sections, sites, cache states, viewports,
and other matrix cells before collection. At least one condition for every key
is necessary. Fresh profile, reused/exited profile, and warm browser must all
appear; the tool does not invent a full Cartesian product of all settings.
Startup has two applicable process-launch regimes. The parent must review the
prespecified matrix's sufficiency for every task. Unknown OS cache is explicit:
fresh profile never implies cold OS cache.

## Schema and evidence boundary

`schema.json` is JSON Schema 2020-12 plus the semantic rules enforced by the CLI.
The bundled validator implements only the schema keywords used here, with an
additional finite-number rule. No third-party validator is needed. Unknown fields
are rejected, keeping typos and old/proxy fields from silently passing. Evidence
and source hashes are lowercase hexadecimal; references are opaque strings and
are never dereferenced by the analyzer.

The required top-level fields are `schema_version`, `synthetic`, `plan`,
`plan_commitment`, `builds`, `attempt_ledger` and `attempts`. `reviews` and its
categories may be absent, leaving those gates outstanding. The plan
fixes purpose, primary decision rule, pair count per condition, first arm and
matching conditions. `plan_commitment.sha256` is SHA-256 of UTF-8 JSON of `plan`
using sorted keys, separators `(',', ':')`, `ensure_ascii=True`, no NaN, no newline.
It must have an independently retained reference, reviewer and UTC recording time
at or before `plan.declared_at`, the declared collection boundary. All acquisition
times must follow that boundary. These are supplied attestations, not a trusted
timestamp service: parent review must rule out retrospective plan selection.

`attempt_ledger` attests that all attempts are retained and counts them, including
failures, cancellations and timeouts. Its hash identifies the externally retained
ledger; the tool checks count/completeness, not external bytes. It cannot detect
a dishonest ledger, omitted real attempts or fabricated measurements.

Baseline source must match the frozen commit. Both arms record executable,
source-tree, asset and lockfile hashes, features, flags, release mode, toolchain,
WebView2/runtime, instrumentation and manifest reference. Features, flags,
toolchain, runtime and instrumentation must match; source/assets can change.
Every attempt must match its arm's executable and the condition's full matching
object, including machine/OS, fixture, seeded profile, extensions, protections,
power, DPI, viewport, window, load policy, random seed and cache states. Ordinary
release mode with HTML chrome and isolated app data is mandatory. Per-attempt CPU
load, available RAM and policy outcome are retained. Parent review must verify
the externally prespecified load bounds, normal protections, no hidden benchmark
behavior flags, and actual build/source/asset provenance. No arbitrary CPU/RAM or
other resource threshold is built into this tool.

Every attempt binds process creation identity, window, controller, document,
task nonce, action nonce, executable, acquisition order and endpoint evidence.
Action, task and evidence nonces must be globally unique, including across kinds.
An evidence record must bind the exact attempt identity and clock. A parent
review must attest foreground ownership, current document/task, correct content,
actual work, usable input, build identity and the full task start endpoint.
Retained versus discarded document semantics require the separate parity review.

Acceptance needs `task_start`, `native_presentation`, `native_interaction` and
`work` evidence, each with reference/hash and finite nonnegative timestamp on the
same monotonic `ms` clock. `proxy` records remain visible but cannot satisfy these
requirements. Server ready, callbacks, title, DOM timestamps, double RAF and
operation counts belong in proxy/semantic records, never native presentation.
Native audio observation is included in the presentation claim for audio tasks.
Capture failure leaves a missing-proof gap. All required evidence must fall in
the timed region; start equals the unique task-start observation and end equals
the last presentation/interaction/work observation. Evidence collection and clock
alignment must be reviewed independently; no overhead is subtracted. Startup
starts at external launch, before internal `process_start`; full new-tab search
starts at the physical new-tab action, rather than at Enter or navigation ready.
One monotonic clock with serial, nonoverlapping intervals is required per bundle.

## Sampling, calculations and decisions

Pairs are numbered 1 through the fixed plan count. Within each condition the
declared first arm alternates by pair: AB, BA, AB, BA (or BA, AB, BA, AB).
This balances within-pair order, as in the parent's paired production plan.
`sequence` is the complete contiguous acquisition ledger order. Conditions may
be interleaved serially; overlapping timed work is invalid. Every pair has exactly
one attempt per arm. Duplicates, retries outside reserved slots, order changes,
mismatches, failed reviews and unsuccessful attempts prevent that condition from
passing, even when ten other pairs are valid. Missing slots are not zeroes.
Reserve a larger fixed plan up front when variability warrants it; do not stop
on a favorable result or rewrite a plan after collection. An expanded later plan
requires fresh predeclaration and parent review of all earlier attempts/reports.

The primary estimate is the **median of paired candidate/baseline task-time
ratios**, target <=0.5. This choice makes the prose contract operational; it is
not a claim that every individual run or tail met the target. Baseline must be
positive to define a ratio; zero candidate duration is permitted as supplied
evidence but still needs independently credible presentation/clock resolution.
Report per-condition baseline and candidate absolute ms, paired absolute deltas,
every paired ratio, median, min/max and nearest-rank p95. Paired summaries use
valid matched pairs only, clearly accompanied by all observed attempt elapsed
distributions (including invalid attempts), outcome counts by arm, invalid rates
and unsuccessful rates. Incomplete/failed evidence blocks acceptance; its timings
remain in raw attempts and are never relabeled as successful task durations.

Uncertainty uses a distribution-free sign/order-statistic confidence interval
for the population median paired ratio. Choose the largest k with
`1 - 2*sum(comb(n,j) for j in range(k))/2**n >= 0.95`, then bounds are sorted
ratios `[k-1]` and `[n-k]`. With no finite such bounds, report `[0, unbounded]`.
Coverage is discrete and reported (ten pairs give at least 97.8515625%). The
interval assumes independent matched pairs and a stable sampling regime; serial
drift, autocorrelation and selection can invalidate its interpretation. Ties make
coverage conservative. Small-sample p95 is descriptive, not precise tail proof.

There is no hidden confidence requirement in the parent contract. Plans must
explicitly select `median_ratio` (the point target) or `upper_95_bound` (a stricter
optional rule requiring the interval upper bound <=0.5). The report exposes both
tests and the entire supplied plan. A point pass with an interval crossing the
target remains uncertain and needs parent review/possibly expanded collection.
The fixture generator chooses `median_ratio`; no sampling runs occur here.

Condition status is `invalid` for invalid/unsuccessful evidence or manipulated
order; `exploratory` for a complete exploratory plan of at least five pairs;
`insufficient` for missing slots, fewer than ten acceptance pairs or an unresolved
selected confidence bound; `measured_target_missed` for an eligible point estimate
above 0.5; or `supported_measured_acceptance` for eligible supplied timing evidence.
These statuses apply per condition; no aggregate ratio can hide a missing/missed
condition. Global timing status is invalid, then missed, then insufficient, then
exploratory, or supported, in that precedence; all condition rows remain visible.

Resource, parity/stability/council and diagnostic gates are separate. Missing
reviews remain outstanding. A passing gate needs a parent reviewer/reference,
evidence hashes/references, exact baseline/candidate commits, source-tree hashes and executable identities,
plan/ledger hashes and coverage
of every **condition ID** in the plan. Stale/unsupported passing reviews are invalid
review evidence and block overall support without changing valid timing results.
Commit identity alone is insufficient when uncommitted source changes share a base commit.
The parent must supply actual RAM/CPU/GPU/page-fault/network/energy values and
coverage in those referenced reviews; a timing pass cannot fill their absence.
No resource improvement offsets a missed task-time condition or parity failure.

## Synthetic fixtures

`fixtures/synthetic.py` creates fabricated data, marked `synthetic: true`, only
for arithmetic/rejection tests. It does not read a host clock or run Bowser.
Its references explicitly say `SYNTHETIC` and do not point to real native proof.
Every synthetic report says **SYNTHETIC FIXTURE — NOT PERFORMANCE PROOF**, always
has global status `synthetic_not_performance_proof` (or invalid input), and exits 1.
Do not attach fixture reports as measured performance evidence. Tests generate
and remove a tiny self-contained report in a temporary directory, verify escaping
and provenance, and cover the CLI as well as the public analysis function.
