# Guarded cloud Windows release: v0.0.19

This procedure needs only standard GitHub-hosted runners, not a connected laptop,
paid runner, persistent release credential, or repository setting change.
It replaces the local build portion of the historical release record in
`benchmarks/results/final/release-procedure.md`, not the separate stable rollout.

## Order of operations

1. Merge the repaired UI, committed reproducible `ui/dist`, native smoke helper,
   and `.github/workflows/release-v0.0.19.yml` to `main` while Cargo is still
   `0.0.18`. The new workflow remains disarmed at that version.
2. Open a separate release-preparation PR that changes Cargo.toml and the bowser
   package in Cargo.lock to `0.0.19`, and `src/updater.rs` PIN_REVISION to `20`.
   Do not update `stable/release.json` yet. Merge only after ordinary Windows
   verification passes. The merge's actual main commit SHA is the build identity.
3. The version-specific workflow runs automatically on that main push. It checks
   exact version/revision, locked dependencies, UI typecheck/lint/tests (including
   mandatory Edge-backed tests), shipped Shield tests, byte-reproducible UI,
   Rust tests, Clippy, the release build, and isolated real-executable native
   smoke. A missing desktop, Edge, WebView2, or failed check blocks publication.
   Smoke evidence explicitly describes its bounded coverage and unverified areas;
   it is not a substitute for full human UI QA or a performance audit.
4. The read-only build job uploads an immutable candidate artifact with the exact
   source SHA, executable SHA-256 and size, compiler identity, run identity, and
   hash-bound smoke report. A dependent job receives only an ephemeral
   `GITHUB_TOKEN` with `contents: write`. It executes no checkout scripts or
   candidate binary, validates provenance, creates a draft targeting the exact
   build SHA, uploads `Bowser.exe` and `build-metadata.json`, downloads both for
   verification, and only then publishes. The release is not marked latest.
5. Independently download the public release asset, compare hash/size and confirm
   that `v0.0.19` resolves to the recorded build SHA. Review the native evidence.
   Only then open a separate stable-pin PR with version `0.0.19`, revision `20`,
   the full source SHA, repository `grantcharlie1000-tech/Bowser`, tag `v0.0.19`,
   asset `Bowser.exe`, and the independently checked hash/byte count. Run updater
   tests before merging. This pin merge is what offers the build to installed
   stable users. The workflow never edits or merges that pin.

## Fail-closed recovery

- Do not retarget tags, overwrite assets, delete releases, or force-republish.
- Concurrency is serialized for this version. An existing tag pointing elsewhere
  is an error. An existing published release is only verified, not overwritten;
  its tag, metadata, asset hash and size must match the candidate.
- An existing draft, partial upload, or ambiguous creation/publication result
  stops for inspection. Check the remote release and workflow logs before any
  retry. A full rebuild may have different bytes; mismatch is a blocker, not
  permission to replace the published executable.
- Ordinary PR/test jobs remain read-only. There is no `pull_request_target`,
  branch-dispatched publication, account setting change, or stored secret.
- GitHub's release API may refuse an exact target that changes workflow files
  relative to the current default branch under `GITHUB_TOKEN`. Keep the workflow
  installation and version-preparation merges separate, and do not expand token
  permissions or create a persistent credential to work around a refusal.

## References

- [GitHub CLI release creation](https://cli.github.com/manual/gh_release_create)
- [GitHub CLI release download](https://cli.github.com/manual/gh_release_download)
- [GitHub release API](https://docs.github.com/en/rest/releases/releases)
- [Pinned official download-artifact v4.3.0](https://github.com/actions/download-artifact/commit/d3f86a106a0bac45b974a628896c90dbdf5c8093)
