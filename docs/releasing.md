# Releasing Bowser and moving the stable pin

One release takes three short steps and two human merges. Every release uses
the same workflow, `.github/workflows/release.yml`; the version, pin revision
and tag are read from the source, so there is no per-release workflow to write.
Production changes only when a human merges the `stable/release.json` PR.

Use `$R = 'grantcharlie1000-tech/Bowser'` and `X.Y.Z` for the new version below.

| Step | What runs | Expected time (estimate) |
| --- | --- | --- |
| 1. Version-bump PR | `windows-verify` (Rust, UI and build jobs in parallel) | ~10 min |
| 2. Release dispatch | `release.yml` gate, three parallel Windows jobs, publish | ~12 min |
| 3. Stable pin PR | `windows-verify` pin-only check, `stable-package` | ~1 min |
| After merge | `stable-package` push verification and `mark-latest` | ~1 min |

Before this, v0.0.31 took about 88 minutes of serial CI: a full Windows run on
the pin PR, a separate `windows-verify` dispatch on main, a mandatory dry run,
and a 17-minute single-job release run.

## 0. Preconditions

- The features going into the release are merged to main and their Windows checks passed.
- Merge freeze from the version-bump merge until the pin PR is merged.
- No `vX.Y.Z` release, draft or tag exists:
  `gh api repos/$R/releases --paginate --jq '.[]|select(.tag_name=="vX.Y.Z")|{id,draft}'`
  and `gh api repos/$R/git/ref/tags/vX.Y.Z` (expected 404).

## 1. Version-bump PR

```powershell
python tools/release.py bump X.Y.Z
cargo test --locked --lib updater::
```

The script changes exactly `Cargo.toml`, the bowser entry in `Cargo.lock`,
`PIN_REVISION` in `src/updater.rs` (+1), and adds the one exact
`{tag, version, revision}` entry to the `mark-latest` allow-list in
`.github/workflows/stable-package.yml`. Do not change the stable pin. Open the
PR, wait for `windows-verify`, and merge with a merge commit. That main commit
is `<BUILD_SHA>`.

## 2. Release

```powershell
gh workflow run release.yml --repo $R --ref main -f commit=<BUILD_SHA> -f dry_run=false
gh run watch <RUN_ID> --repo $R --exit-status
```

The gate (about 10 s) refuses to publish unless: the commit is main's head and
clean; Cargo.toml and Cargo.lock agree; the version and `PIN_REVISION` are both
above the live pin; the allow-list entry exists; and no tag points elsewhere.
Three Windows jobs then run in parallel on that exact commit:

- `rust`: native theme check, both feature sets of `cargo test`, and Clippy.
- `ui`: typecheck, lint, the CJK baseline, the full UI suite, and Shield.
- `build`: reproducible UI rebuild, release build and native smoke, then it records the build metadata.

The `verdict` job requires all three to pass and the same `rustc` in tests and
build. Only then does `publish` run. It is the only job with write permission.
It creates a draft, uploads the files, downloads them back by asset id and
verifies them, then publishes with `make_latest=false`. The run summary prints
the pin values.

You do not need a separate `windows-verify` dispatch on main or a dry run first:
the release jobs are a superset of `windows-verify` on the exact commit. Use
`-f dry_run=true` only when you want a check without publishing.

A failed run is dispatched again, never partially re-run: the publisher
requires the candidate's run id and attempt to be its own. If the run fails
after the publish job has started, inspect the remote state first. Existing
drafts stop publication. Never delete, replace, retag or blindly retry them.

## 3. Stable pin PR: the production switch

On a branch from main:

```powershell
python tools/release.py pin vX.Y.Z
```

This downloads the published `Bowser.exe` and `build-metadata.json` itself and
hashes the executable. It requires the metadata to agree with the hash, the
byte length, the tag, the version, `PIN_REVISION` and the build commit. Then
it rewrites only `stable/release.json`, keeping the field order and four-space
indent. Commit that one file and open the PR.

Because only `stable/release.json` changed, `windows-verify` skips the Windows
build. Its `changes` job runs `tools/release.py check-pin` instead, which applies
the rules of the Rust test `the_shown_version_follows_the_pinned_release` plus
revision == `PIN_REVISION` and the allow-list entry. A PR that touches any other
file gets the full Windows run. `stable-package` installs the public package in
isolation. Merge when both pass.

After the merge, the push run of `stable-package` verifies the package again.
Its `mark-latest` job then promotes only the exact allow-listed release, after
rechecking the public bytes and the pin, and verifies the public
latest-download route. Publishing the files alone is not a stable rollout.

## Rollback

The updater will not downgrade a user who is already running vX.Y.Z. To roll
back, release a fix-forward build with a higher version and pin revision. The
metadata contract requires the release metadata revision to equal the pin
revision, so you cannot re-point the pin at an older artifact with a new
revision. Any emergency rollback needs a separately designed and reviewed
procedure. Never delete or retag a published release.

The older per-version `release-v0.0.*.yml` workflows and the recovery workflows
are historical. Leave them unchanged and do not dispatch them again. See
`docs/release-cloud-v0.0.19.md` for historical recovery.
