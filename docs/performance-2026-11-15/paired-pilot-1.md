# Ordinary StartupOnly collection pilot 1

This is a collection-path and native usability pilot, **not accepted task-time data**. No speedup, RAM reduction, or exhaustion follows.

Raw output: `C:/Users/charlie/.t3/artifacts/bowser-performance-20261115/paired-startup-timing-60b1ce40886d41ef94e74f06cb681314`.
The runner exited 0 with `completed_pilot_unaccepted`, both sample errors null, both application exits 0, and both owned process trees confirmed drained. `presentation` and `accepted_task_ms` remain null; `accepted` remains false.

## Inputs and integration

The hash-bound five-file paired-measurement package from `handoffs/paired-measurement-20261115T082556Z-52f8dcd6` was integrated after verifying all 254 frozen benchmark prerequisites. Parent reran all **34 pure PowerShell 5.1 contracts**, fixture Node syntax, and benchmark whitespace checks successfully. No product source changed during integration.

- Pair manifest SHA-256: `13884e5b55a66b52ee0d8733e28b9b2278e962c476c09e187e1072858350a59c`.
- Baseline executable: `F0203D9F8F928C82598D13E8EF279315B80C12EA46828C7F8ED0A88AC00C327E`.
- Candidate 3 executable: `423EE65B14AB9B8E4C9EBA9860BD326477C51B81E30841F0C7629A28DA60215D`.
- Ordinary flags only: isolated `--data-dir`, loopback `--open-url`, and `--diagnostic-log`. No interaction/detail/updater-off/security behavior flags.
- Each arm used a new empty profile and LOCALAPPDATA root. This does not establish cold OS caches.
- Shared build/native lane held throughout the serial pair; no Cargo/npm or other owned test application ran concurrently.

## Native observations

Both windows displayed the same first-run import prompt. The parent dismissed it with Computer Use Escape, verified full intended HTML toolbar/tabs/rail and the correct visible Ready nonce, typed a fresh challenge, verified the field in AX and actual captured pixels, then closed with Computer Use Alt+F4. Both exact windows disappeared from the inventory. The actual user browser was left untouched.

| Arm | Root PID / HWND | Nonce | Challenge |
| --- | --- | --- | --- |
| Baseline | 25568 / 593326 | `70c0ab9795f54597936e7637eb653ee2` | `probe_70c0ab97` |
| Candidate 3 | 26104 / 593368 | `c21d9778272647b9952ecffdf8c80ad4` | `probe_c21d9778` |

Both observed captures were 1000x720. The approved capture helper was adapted into `capture-owned-paired-window.ps1` in the artifact root. It accepts only a matching paired ready record, exact sample/token/HWND, and the two pinned executable paths/hashes; it retains held-process lifetime, foreground, bounds and before/after ownership checks. Original recovery capture helper is unchanged. Each PNG has adjacent QPC/UTC, bounds, process/executable and hash provenance.

Baseline captures:

- `p1-baseline-70c0ab9795f54597936e7637eb653ee2/baseline-presented-20261115T0841247381096Z.png`
- `p1-baseline-70c0ab9795f54597936e7637eb653ee2/baseline-input-20261115T0842160455807Z.png`

Candidate captures:

- `p1-candidate-c21d9778272647b9952ecffdf8c80ad4/candidate-presented-20261115T0843255700146Z.png`
- `p1-candidate-c21d9778272647b9952ecffdf8c80ad4/candidate-input-20261115T0844075973897Z.png`

Element-index clicking initially failed with `coordinate input geometry is unavailable`. Approved captured coordinates were used for the field. Immediate AX snapshots omitted the just-typed value; after the next Tab action, AX and pixels both contained the exact challenge in both builds. This establishes neither the cause nor latency of that observation lag.

## Problems exposed before accepted measurement

1. **Timing observer interference.** Although native RAM counters are resource-only, the timing loop still runs whole-system CIM inventories and host-memory/CPU queries. Median QPC-bracketed pass duration was 1,910.65 ms in the baseline arm and 1,943.13 ms in the candidate arm; maxima were 5,339.95 and 4,619.08 ms. These are observer costs, not application times. The nominal 500-ms sleep does not establish sampling cadence. Move heavy ownership/host collection outside the timed application interval without weakening lifetime/tree-drain proof.
2. **Baseline provenance.** Its historical manifest did not record a complete build-time source snapshot/dirty state. A later Git-derived committed-input manifest cannot prove those earlier bytes. Rebuild an immutable pinned baseline from recorded clean product inputs before accepted comparisons; keep the original binary and evidence intact.
3. **Presentation endpoint.** These are late snapshots after observation, not a calibrated burst bracketing first presentation. Native readiness receipts remain proxies. Complete capture invocation costs exceed its inner CopyFromScreen interval.
4. **Fresh first-run behavior.** Import prompts must remain explicit workload state; operator dismissal cannot be silently removed with benchmark-only behavior or counted as browser latency.
5. **Fixture response logging.** Existing request arrivals include guessed status codes. They do not establish actual finished/aborted response outcomes. This pilot used correct positive light-page pixels plus bound readiness; broader failure acceptance requires corrected outcome records.

No resource pilot ran. Do not collect more nominal timing pairs until observer interference and endpoint/provenance limits are addressed.
