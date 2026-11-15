# Paired ordinary-release StartupOnly pilot

Use `production-baseline.ps1 -ExternalUiDriver -StartupOnly -PositiveOnly -Fixtures light`
with explicit `BaselineExe`, `CandidateExe`, `PairManifest`, `PairManifestSha256`
and a new `Out` directory. The ordinary HTML-default releases keep first-run UI,
updater and security defaults. Only `--data-dir`, `--open-url` and `--diagnostic-log`
are passed. Inherited BOWSER_/WEBVIEW2_ overrides are rejected. The runner performs
no UI/input/capture/close/kill work on the app. Parent Computer Use owns those actions.

Each pair uses fresh empty data/LOCALAPPDATA roots: AB, BA, AB, BA, and so on.
Timing and resource require separate invocations/output roots. Fresh profiles do
not mean cold OS caches. One pair exercises collection; acceptance still requires
calibrated presentation/input endpoints, at least ten valid matched alternating
pairs per condition and every original per-area gate. This path remains unaccepted.

The pinned pair binds each executable path/hash/size, its full build manifest and
source manifest. Source maps match by path and hash, with no candidate file-count
constant. A `recorded-clean-build-snapshot` also binds the successful build's complete
pre/post source observations, clean status and discovered product inputs. These
observations must have been recorded around that build. The old F0203D9F baseline
supports historical pilots with an explicit unrecorded build-time dirty-state limit;
committed Git bytes cannot retroactively prove its original dirty state. A new
recorded baseline establishes provenance for that new executable only.

Every sample is persisted before launch and after the external Process.Start QPC
brackets. That interval measures launch-call overhead, not startup completion.
The held process handle, PID, start ticks and executable path bind the root; read
locks and revalidated hashes bind executable bytes. A nonzero HWND remains only
a discovery hint. Parent Computer Use must independently verify the bound window.

Timing uses two operator stages. Before `begin-cleanup.json`, the runner polls only
the held root's metadata and the receipt file. It performs no process inventory,
counter or host queries in that observation interval. Lightweight metadata polling,
receipt I/O and the ordinary fixture/diagnostic logs remain observer effects; this
repair does not claim zero overhead or presentation calibration.

1. Read `ready-for-operator.json`. Verify the exact disposable window through
   Computer Use, observe/capture the full usable page and visible nonce, inspect
   ordinary first-run UI, and exercise a fresh typed input challenge. Record failed
   observations as `failed`. Leave the app alive, then atomically write the bound
   begin-cleanup receipt below.
2. Wait for `ready-for-close.json`. The runner has now ended observation and
   collected heavy ownership inventory with the bound root still alive. Verify
   this second record against the first and the window. Close only that window
   through Computer Use, then atomically write the bound close receipt below.

```powershell
$r=Get-Content -LiteralPath $readyPath -Raw|ConvertFrom-Json
$begin=[ordered]@{
    sample_id=$r.sample_id;token=$r.token;pid=$r.pid;start_ticks=$r.start_ticks
    exe=$r.exe;exe_sha256=$r.exe_sha256;driver='ComputerUse'
    action='begin_cleanup';closed=$false;outcome='observed' # or 'failed'
    notes='Record usable chrome/content, visible nonce, first-run UI, fresh input, DPI/bounds and conditions'
    evidence=@('Absolute paths to parent captures and provenance')
}
$begin|ConvertTo-Json -Depth 8|Set-Content -LiteralPath ($r.begin_cleanup+'.tmp') -Encoding UTF8
Move-Item -LiteralPath ($r.begin_cleanup+'.tmp') -Destination $r.begin_cleanup
# Wait until the runner publishes $r.ready_for_close. Do not close before it exists.
$c=Get-Content -LiteralPath $r.ready_for_close -Raw|ConvertFrom-Json
# Verify $c's sample/token/PID/start/exe/hash against $r and the exact window.
# Close via Computer Use, then:
[ordered]@{
    sample_id=$c.sample_id;token=$c.token;pid=$c.pid;start_ticks=$c.start_ticks
    exe=$c.exe;exe_sha256=$c.exe_sha256;driver='ComputerUse'
    begin_cleanup_sha256=$c.begin_cleanup_sha256;closed=$true;outcome='observed' # or 'failed'
    notes='Computer Use close result';evidence=@('Absolute paths to parent evidence')
}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath ($c.operator_result+'.tmp') -Encoding UTF8
Move-Item -LiteralPath ($c.operator_result+'.tmp') -Destination $c.operator_result
```

The begin receipt is read once and its exact bytes are hashed into the sample and
ready-for-close record. The close receipt must bind that digest and the same process
identity. Stale receipts, wrong paths, early exits and failed outcomes reject the
attempt. A `failed` begin receipt still permits controlled cleanup; it cannot pass
the attempt. Parent must review all receipts/evidence rather than treating a file
as independent proof of usability.

Observation and close each have an `OperatorTimeoutSeconds` deadline (default 180,
range 30..600). Drain has a 20-second polling deadline; the close receipt has up to
30 seconds after drain. OS/CIM calls may exceed polling deadlines. Timeouts leave
the app untouched, persist `paired-isolation-blocked.json` and stop subsequent
launches. Parent Computer Use cleanup and whole-tree proof are required before
removing that block. No automatic retries or overwritten attempt directories.

Ownership inventory requires a nonempty successful Win32_Process query and binds
CIM root creation ticks once to the held root within at most 1 ms of timestamp
precision loss. Exact identities are remembered thereafter. Cleanup follows live
root descendants, profile-matching processes, remembered/reparented helpers and
their later descendants; recycled PIDs do not seed ownership. The root must be alive
before and after the initial cleanup inventory before ready-for-close is published.
After root exit, two consecutive nonempty inventories must show no remaining owned
or profile tree. Query failure, missing creation identity or surviving helpers
cannot establish drain. All cleanup observations/errors are retained outside timing.
This remains sampled process/profile ownership, not an ETW-complete history: helpers
born/exited or reparented without a discoverable profile before cleanup can escape
sampling. The two-stage lifecycle deliberately retains a live root at ownership
collection; the parent must validate real WebView2 tree coverage before acceptance.

`observation_end_received_qpc` labels receipt processing, not first presentation or
input latency. Operator/capture/cleanup/close/drain delays are never task latency.
Diagnostic readiness is a nonce/navigation-bound native receipt proxy. DOM/title/
double RAF markers are semantic proxies. `presentation`, `accepted_task_ms` remain
null and `accepted` false. No ratios/aggregation hide failures. Parent must calibrate
the entire capture invocation, bracket observed frames, retain negative frames and
report timestamp uncertainty; late screenshots cannot locate first presentation.

Resource mode separately samples native full-tree counters and host load during
observation. Rows include process details/errors; incomplete native coverage leaves
native totals unavailable. WMI proxy values are labelled. Shared working pages can
be counted more than once; transient processes/CPU may be missed. Rows include
operator work and are not standardized settled/idle RAM results. Each heavy pass
has QPC brackets excluding JSON serialization and sleep; polling is not cadence.

Fixture request rows retain `path`, `token` and arrival `at_ns`, with
`event=request_arrival` and `status=null`. Outcomes use the separate sentinel
`path=/__response_outcome__`, original `request_path`, request ID, finish/abort time
and `event=response_finish` or `response_abort`. Only `response_finish` carries the
actual server status; finish means Node handed the response to the OS, not that
the browser consumed/presented it. Aborts have null status; a hung live response
has no outcome. Count completed 200s only from finish rows with status 200, never
from arrivals, `/ready` 204, redirects, errors or aborted/unfinished sockets.

Allowed pure checks are `test-paired-startup.ps1` under Windows PowerShell 5.1 and
`node --check` plus `test-production-fixture-outcomes.mjs`. Fake process interfaces
and supplied inventories test observation/identity/tree/counter behavior without
native Bowser/CIM/UI/input/capture work. The focused Node test launches only its
own loopback fixture server. Native lifecycle/coverage/calibration and acceptance
remain entirely with the parent.
