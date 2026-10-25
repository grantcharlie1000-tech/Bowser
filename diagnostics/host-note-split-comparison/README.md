# Prepared, not published: one host-only Note/Split comparison

## Review gate

Independent Astra review is required before any publication or execution. **Publishing the
workflow to `diagnostics/host-note-split-comparison-20261025` triggers the job.** Do not push
it merely to save a draft. Do not open a PR: that would activate unrelated PR workflows.
There is no dispatch yet and no comparison result. The preparation has used only GitHub
read operations and this isolated cloud directory, never the user's computer.

The intended diagnostics-only commit has parent
`70b2db35ebbf96ea41eb7a9cb22b5e96146eaab2` and adds only this directory plus
`.github/workflows/host-note-split-comparison.yml`. It does not alter #98, #97, any tested
source, test, fixture, guard, deadline, configuration, updater, release, or deployment file.

## What this will establish

One sequential matched pair on the same fresh standard GitHub Windows VM:

1. Baseline `74123993475e3db4a26686a1727dfc94ed103255`.
2. Candidate tested merge `70b2db35ebbf96ea41eb7a9cb22b5e96146eaab2` (icon head
   `4d34c863ec094fe5fe41e342693625715a62fa55`).

Each builds the exact same locked libtest feature set, then runs the independent exact
`engine::app_host_menu_probe::live::note_split::live_probe_real_app_host_note_split_context_menu`
once, with its original consent, argument checks, fixture, safety predicates, input guards,
and deadlines. Ordinary baseline failure does not prevent the candidate; policy, observer,
source-integrity, build, or supervisory blockers do. Both builds happen before either lane.

This can distinguish the pair's observed behavior on one VM. It cannot prove causality,
establish a test defect from one intermittent observation, satisfy the original Copy/Paste
gate, or claim Store compatibility or production readiness. Neither local Restricted-policy
failure nor a never-run candidate is evidence about the icon fix.

## Policy and security boundaries

- All workflow preparation/run steps use `cmd`. Source uses native public `git` fetches
  at exact SHAs, with no token, checkout action, archive fallback, or safe-directory write.
  The stock preinstalled stable Rust toolchain is verified and reused, never installed;
  `RUSTUP_AUTO_INSTALL=0` disables automatic installer fallback. C# diagnostics compile with the stock .NET
  Framework compiler directly. No execution-policy option, setting, environment override,
  registry write, or PowerShell preparation script is supplied.
- A first `cmd` step checks the default policy before any repository/tool preparation.
  The coordinator makes read-only `Get-ExecutionPolicy`/`-List` queries in the unchanged
  native helper host. Restricted/AllSigned or an inherited process-policy override is a
  blocker before testing, not an invitation to bypass. A later policy refusal from the
  unchanged helper aborts before the next lane. Observed scopes are compared after builds,
  after each completed lane, and finally; changes are reported, never restored automatically.
- The test itself still invokes its original `powershell.exe -NoProfile -File` helper.
  Note/Split never constructs `ClipboardGuard` or uses clipboard contents. **The unchanged
  helper's existing failure-only runner-state snapshot can query clipboard sequence metadata.**
  It is not modified; its general `runner_state`/clipboard metadata is not copied to public
  logs. No new diagnostic code calls any clipboard API.
- The separate observer retains only `PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE` for
  the exact libtest PID, full image path and creation FILETIME. It checks that identity
  repeatedly; it cannot kill, suspend, write, inject, activate, or send input/messages.
- Only same-process HWNDs, thread IDs, raw native class names, visibility and owner/root
  chains are logged. Other processes' foreground identity and owner-chain transition details
  are omitted; only an outside-target boolean/status is retained. No text, screenshots,
  accessibility, clipboard, or other application contents are inspected by the observer.
- HWND snapshots remain explicitly non-atomic and non-authorizing. Same PID/STA is not HWND
  lifetime proof. Observations do not relax any existing gate or permit focus rescue.
- The observer attaches immediately after normal test launch, without suspending or gating
  the test. There is therefore an unobserved startup interval before observer readiness;
  the bounded trace cannot prove what happened during that gap.
- The external 300-second supervision/observer cap is not test acceptance or a changed test
  deadline. A cap stops the pair without a new kill/rescue attempt; GitHub's finite job
  lifecycle tears down the VM. Logs are bounded, drained without blocking the child, and
  marked if capped. No retry-until-green exists; rerun attempts skip the job.
- The unchanged helper retains its original failure-only exact-process-handle termination
  path after native input, and its existing cleanup obligations. The diagnostic adds no
  termination behavior. No claim is made that the entire unchanged test has no kill API.
- Inherited `RC`, compiler-wrapper, `RUSTFLAGS`, or encoded Rust flags are blockers; they
  are not cleared or changed. Output EOF drains have a separate 10-second bound.
- This is not a system-wide Windows security-state audit. Execution-policy observations,
  source integrity and the diagnostic's absence of security-write APIs are the bounded
  evidence. The parent's separate local persistence verification is not duplicated here.

## Cost and trigger review

Checked 2026-10-25 around 01:51 UTC:

- Repository API reports public (`private=false`).
- [GitHub standard hosted-runner documentation](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
  lists `windows-2025-vs2026` as a standard public runner; standard public use is free and unlimited.
- [Official image inventory](https://github.com/actions/runner-images/blob/main/images/windows/Windows2025-VS2026-Readme.md)
  exists (image `20261007.264.1`). Actual allocation/tool availability is not yet tested.
- Repository Actions API reported 0 queued and 0 in-progress runs. This is not an account-wide
  billing or concurrency audit; no supported account billing query was available.
- All 11 existing workflows at the candidate were inspected. Push-trigger branches are
  `main` or `performance/post-ui`; no `workflow_run`/`create` trigger exists. A push solely to
  the dedicated diagnostics branch does not run them. No PR will be created.

Refresh public status, runner documentation/availability and current Actions activity
immediately before any approved publication. Use only this standard free runner. If a
cost/capacity condition is uncertain, stop. No larger/custom/self-hosted runner, cache or
artifact upload, secrets, publish token, release, merge, deployment or permission change is
included. Evidence goes only to bounded job logs and job summary; VM-local outputs expire
with the job. The workflow timeout is 25 minutes and contains one pair, no matrix or retries.
The compare step's 20-minute cap is below the sum of both builds' and both lanes' individual
maxima. A slow run may therefore stop as an incomplete-pair blocker; that never authorizes
a rerun, fallback, reduced gate, or increased deadline.

## Validation performed in the preparation container

The workflow parses as YAML. The initial pinned checkout-action proposal was rejected
  because its transitive REST ZIP fallback supplies an execution-policy override; it has
  been completely removed. The current workflow uses no actions. Static checks verify trigger isolation, least privilege, fixed
commits, unchanged safety blob identities, command shell, bounded observation, and excluded
native mutation APIs. The unchanged test/helper safety files have identical Git blobs in
baseline and candidate, including native helper `416d81358a21ea6db23c0b37c09ab9dc67b6be37`
and Note/Split test `59e7a7621d66d34ffdff82dc9a4fd26c32b7fbea`.

**Not yet verified:** Windows C# compilation, runtime behavior, actual runner allocation,
unchanged helper executability under that runner's default policy, and comparison outcomes.
This Linux preparation container has no C# compiler/.NET runtime or Windows desktop.
