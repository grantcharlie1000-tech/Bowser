# New baseline and observation repair

The original F0203D9F baseline remains intact. Its historical build-time dirty state is still unproven; a new source manifest cannot repair that omission retroactively.

A new ordinary `html-chrome-default` baseline was built from clean commit `440575e0870fd35c257d4195ec990d3c02b837aa`. Contemporaneous pre/post records agree on all 824 product inputs and 3542 tracked-file hashes, with clean status and no extra ignored/untracked product inputs. The parent reverified the executable and manifest hashes, both observation-record pins, all 824 product-map entries, and clean pre/post state. The observation timestamps appropriately differ. This establishes the new artifact's recorded build inputs, not hermetic dependency/toolchain provenance or absence of transient restored edits.

- Executable: `C:/Users/charlie/.t3/artifacts/bowser-performance-20261115/baseline-proven-20261115T085052Z-91688e9f/release/bowser.exe`
- SHA-256: `4EB54DE3C9DDBDC185DA8A826BE59B59D31419A07B562801C4C5814DBC91A7B0`; 8540160 bytes.
- Build-manifest SHA-256: `97107490d4a8e9f8015390f7780dd2805077802e0626c8c5d4ecb23211cbefc3`.
- Full package/handoff: `handoffs/proven-baseline-cleanup-20261115T085008Z-20dd3f76` in the artifact root.

The first paired pilot exposed expensive process/host observations during startup. The reviewed five-file incremental measurement repair is now integrated. Timing observation performs only held-root metadata and receipt polling. After Computer Use confirms usable content and a fresh input challenge, the parent leaves the window alive and writes `begin-cleanup.json`. Only then does the runner collect process ownership, publish `ready-for-close.json`, and permit parent Computer Use close. The final receipt binds the first receipt's bytes. Cleanup requires two successful inventories with no remaining owned/profile processes. Failure remains retained and blocks further launches.

Fixture logs now distinguish request arrivals from actual response finishes and aborts. An arrival is never labelled successful HTTP 200; 204/302/404/500 and unfinished sockets remain distinct. Server finish is not browser consumption or presentation.

Parent validation: all **56** pure PowerShell 5.1 contracts passed against the newly pinned baseline/candidate-3 pair; both Node syntax checks and all **8** HTTP-outcome cases passed. These checks ran no Bowser, native UI, CIM, input or capture work. The identity manifest still binds candidate 3; candidate 4 requires a fresh pair/source manifest after its release build. Lightweight observation and capture overhead, native tree coverage, first-presentation calibration and separate settled RAM collection remain to be measured. No task-time gate is accepted and no speedup or RAM saving is claimed.
