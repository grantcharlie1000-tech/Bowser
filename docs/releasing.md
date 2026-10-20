# Releasing Bowser v0.0.20 (pin revision 21)

The build, hash, draft, upload and publication run on a GitHub-hosted
`windows-latest` runner from `.github/workflows/release-v0.0.20.yml`. The
workflow is `workflow_dispatch` only, so nothing publishes on a push or a PR.
Production changes only when `stable/release.json` changes, and that is always a
separate, human-merged PR. The v0.0.19 history is in `docs/release-cloud-v0.0.19.md`.

`$R = 'grantcharlie1000-tech/Bowser'` in the commands below.

## 0. Preconditions

- This tooling PR is merged to main on its own, before the version bump.
- Merge freeze: nothing else merges to main from the version-bump PR until the
  stable pin PR is merged. Park the stale PRs and either land or exclude #49.
- No `v0.0.20` tag or release exists, drafts included:
  `gh api repos/$R/releases --paginate --jq '.[]|select(.tag_name=="v0.0.20")|{id,draft}'`
  prints nothing and `gh api repos/$R/git/ref/tags/v0.0.20` returns 404.
- `stable/release.json` on main is still v0.0.19, revision 20.

## 1. Version-bump PR (the commit that ships)

Branch from current `origin/main` and change exactly three things:

- `Cargo.toml`: `version = "0.0.20"`
- `Cargo.lock`: the `bowser` package entry: `version = "0.0.20"` (that line only)
- `src/updater.rs`: `const PIN_REVISION: u32 = 21;`

Do not touch `stable/release.json`. Run `cargo test --locked --lib updater::`,
open the PR, wait for `windows-verify` to pass, then merge with a merge commit.
The merge commit on main is `<BUILD_SHA>`.

## 2. Verify the exact main commit

`windows-verify` does not run on pushes, and the release gate requires a
successful `windows-verify` run on exactly `<BUILD_SHA>`:

```powershell
gh workflow run windows-verify.yml --repo $R --ref main
gh run list --repo $R --workflow windows-verify.yml --event workflow_dispatch --limit 1 --json databaseId,headSha
gh run watch <RUN_ID> --repo $R --exit-status   # headSha must be <BUILD_SHA>
```

## 3. Dry run, then publish

`commit` must equal main's head when you dispatch; if main has moved, stop and
start again from step 2.

```powershell
gh workflow run release-v0.0.20.yml --repo $R --ref main -f commit=<BUILD_SHA> -f dry_run=true
gh run watch <RUN_ID> --repo $R --exit-status
```

The dry run reruns every windows-verify check, builds with
`cargo build --release --locked --bin bowser --features html-chrome-default`,
smokes that exe, and prints its sha256 and bytes in the run summary. It never
creates a release. Its last step fails if the gate says the commit is not
releasable, even though the build passed.

When the dry run is green, dispatch again with `-f dry_run=false`. The publish
job then:

1. re-checks the candidate's provenance (commit, 0.0.20, revision 21, run id,
   smoke report),
2. creates draft `v0.0.20` targeting `<BUILD_SHA>`, keeping the release id the
   API returns (a draft has no tag, so it is never looked up by tag),
3. uploads `Bowser.exe` and `build-metadata.json` to that release id,
4. downloads both back by asset id and compares hash and size,
5. publishes by release id with `make_latest=false`, then checks that the tag
   resolves to `<BUILD_SHA>`, and
6. prints the `stable/release.json` values in the run summary.

If any step fails, do not re-run it blindly, and never delete or replace
assets. An existing draft stops the workflow, so inspect it first. A rebuild
produces different bytes.

## 4. Independent verification

```powershell
$d = "$env:TEMP\bowser-v0.0.20-check"; New-Item -ItemType Directory -Force $d | Out-Null
gh release download v0.0.20 --repo $R --pattern Bowser.exe --pattern build-metadata.json --dir $d
(Get-FileHash "$d\Bowser.exe" -Algorithm SHA256).Hash.ToLower(); (Get-Item "$d\Bowser.exe").Length
gh api repos/$R/git/ref/tags/v0.0.20 --jq .object   # commit <BUILD_SHA>
```

The hash, bytes, metadata commit, tag commit and run summary must all agree.
Then run the real-app checks (clean profile, an upgraded copy of a v0.0.19
profile, link launch) against the downloaded file.

## 5. Stable pin PR (the production switch)

Change only `stable/release.json`, keeping its key order and 4-space indent:
version `0.0.20`, revision `21`, commit `<BUILD_SHA>`, tag `v0.0.20`,
repository, asset `Bowser.exe`, and the sha256 and bytes from step 4. Check
by eye that the pin revision (21) equals `PIN_REVISION`, because no test
compares them. `stable-package.yml` verifies the PR. After the merge, its
`mark-latest` job promotes v0.0.20 to latest. v0.0.20 is on that job's
allow-list.

## 6. Rollback

The updater never downgrades a user who already runs v0.0.20. To stop the
rollout, merge a pin PR that points back at the v0.0.19 asset
(`ad5dd58066d12d6b772e7534baef41ab9d26bef1`,
sha256 `341486070d2e74cf9a9e5d5fbeffa90c7dd2ec4fd36bc284a8a3878a7a9bcec1`,
7795712 bytes) with a higher revision, `22`. Add that exact
`{v0.0.19, 0.0.19, 22}` entry to `mark-latest`'s allow-list in the same PR,
otherwise latest stays on v0.0.20. The real fix is to fix forward: release
v0.0.21 with revision 22 by the same procedure. Never delete or retag
v0.0.20 or v0.0.19.

## One-time v0.0.19 workflows

`release-v0.0.19.yml` now arms only on the push that first sets Cargo to 0.0.19.
Later pushes that touch Cargo or updater files run a short gate and skip.
`recover-v0.0.19-draft.yml` runs only when its own file changes, so leave it
unedited. A main push that edits it would re-run its publisher against the
already-public release and fail.
