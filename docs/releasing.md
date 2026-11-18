# Releasing Bowser and moving the stable pin

A release takes three clicks. Run the **Prepare Bowser release** workflow,
merge the version-bump PR it opens, then merge the stable pin PR that the
release opens. Every
release uses `.github/workflows/release.yml`. The version, pin revision and tag
are read from the source. Production changes only when a person merges the
`stable/release.json` PR.

Use `$R = 'grantcharlie1000-tech/Bowser'` and `X.Y.Z` for the new version below.

| Step | Who | What runs | Expected time (estimate) |
| --- | --- | --- | --- |
| 1. Version-bump PR | you: run Prepare, then merge | `prepare-release.yml`, then the `windows-verify` `bump` check | ~1.5 min |
| 2. Release | automatic on merge | `release.yml`: gate, five parallel Windows jobs, publish, pin | ~6.5 min |
| 3. Stable pin PR | opened automatically; you merge | `windows-verify` pin check, `stable-package` | ~1 min |
| After merge | automatic | `stable-package` push verification and `mark-latest` | ~1 min |

Before this, v0.0.31 took about 88 minutes of serial CI and about six manual steps.
The release run is now bounded by its fat-LTO release compile (about 4.5 min).
Going lower would mean changing the shipped binary's build profile.

## 0. Preconditions

- The features going into the release are merged to main and their Windows checks passed.
- Merge freeze from the version-bump merge until the pin PR is merged.
- No `vX.Y.Z` release, draft or tag exists:
  `gh api repos/$R/releases --paginate --jq '.[]|select(.tag_name=="vX.Y.Z")|{id,draft}'`
  and `gh api repos/$R/git/ref/tags/vX.Y.Z` (expected 404).

## 1. Version-bump PR

Actions > **Prepare Bowser release** > Run workflow on `main` (version `next`
or `X.Y.Z`), or:

```powershell
gh workflow run prepare-release.yml --repo $R --ref main -f version=next
```

It runs main's own `tools/release.py bump`. That changes exactly `Cargo.toml`,
the bowser entry in `Cargo.lock`, `PIN_REVISION` in `src/updater.rs` (+1), and
the exact `{tag, version, revision}` entry in `stable/allowlist.json`, the
`mark-latest` allow-list. It commits those four files as one commit on main's
head on `release/bump-vX.Y.Z`, starts `windows-verify` there and opens the PR.
If Actions may not open PRs, the run summary links to the branch. Locally,
`python tools/release.py bump X.Y.Z` makes the same change for a PR you open
yourself.

`windows-verify` runs the base branch's own `tools/release.py bump X.Y.Z` and
requires the result to match the PR tree byte for byte. If it matches, the
check passes in about a minute, because step 2 runs the full Windows
verification on the merged commit before it publishes anything. A PR with any
other change gets the full Windows run. Merge with a merge commit.

## 2. Release (automatic)

The merge pushes a `Cargo.toml` change to main, which starts `release.yml` on
that exact commit. You can also start it by hand:
`gh workflow run release.yml --repo $R --ref main -f commit=<sha> -f dry_run=false`
(with `-f dry_run=true` to check without publishing).

The gate (about 10 s) decides:
- **Nothing to release.** On a push whose version is already pinned, or
  already tagged on an ancestor (a later dependency-only push), it passes
  quietly and skips every other job.
- **Not releasable.** It fails loudly if the commit is not main's clean head,
  Cargo.toml and Cargo.lock disagree, `PIN_REVISION` is not above the live pin,
  the allow-list entry is missing, or the tag points at another commit.
- **Releasable.** Three Windows jobs run in parallel on the commit:
  - `rust-native`: the native theme check and native-default `cargo test`.
  - `rust-html`: HTML-default `cargo test` and Clippy.
  - `ui` (two shards): typecheck, lint, the CJK baseline, Shield, and the UI suite split with `--test-shard`.
  - `ui-repro`: rebuilds the UI and requires the committed `ui/dist`, `ui/dist.manifest` and `icons.gen.ts` to match byte for byte.
  - `build`: compiles from the committed `ui/dist` (`build.rs` refuses a dist that differs from its manifest), runs the native smoke test and records the build metadata. It installs no npm packages.

  `verdict` requires all five jobs to pass and the same `rustc` in both Rust jobs and the build.
  `publish`, the only job allowed to write releases, creates a draft. It
  uploads the files, downloads them back by asset id and verifies them, then
  publishes with `make_latest=false`.

Then the `pin` job downloads the public `Bowser.exe` and `build-metadata.json`
again and hashes the executable. It requires them to agree with the release
identity, and requires the released commit to be on main and main to still pin
an older release. It then creates `release/pin-vX.Y.Z` from main's head with a
one-commit `stable/release.json` change, using the old file's blob sha as a
compare-and-swap. It dispatches `windows-verify` and `stable-package` on that
branch and opens the PR. If the repository does not allow Actions to create
pull requests, the run summary links to the ready branch. Open the PR there
yourself, or enable Settings > Actions > General > "Allow GitHub Actions to
create and approve pull requests". A rerun reuses an identical branch and stops
on a different one; it never forces.

Only one release run waits at a time: a newer queued run replaces an older
one. A failed run is dispatched again, never partially re-run: the publisher
requires the candidate's run id and attempt to be its own. If a run fails after
the publish job has started, inspect the remote state first. Existing drafts
stop publication. Never delete, replace, retag or blindly retry them.

## 3. Stable pin PR: the production switch

Review the PR the pin job opened. Its only change is `stable/release.json`, so
`windows-verify` skips the Windows build. It runs `tools/release.py check-pin`
instead, which checks the rules of the Rust test
`the_shown_version_follows_the_pinned_release`, that the revision equals
`PIN_REVISION`, and the entry in `stable/allowlist.json`. `stable-package` installs the public
package in isolation. Merge by hand when both pass. The workflow never merges
or enables auto-merge: a merge by the workflow token would not start the push
verification.

After the merge, the push run of `stable-package` verifies the package again.
Its `mark-latest` job promotes only the exact allow-listed release, after
rechecking the public bytes and the pin, and verifies the public
latest-download route. Publishing the files alone is not a stable rollout.

To pin by hand instead (for example, if the pin job failed), run
`python tools/release.py pin vX.Y.Z` on a branch from main. It does the same
download and hash checks. Commit the one changed file and open the PR.

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
