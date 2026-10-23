# Releasing Bowser v0.0.23 (pin revision 24)

This release ships the page-fullscreen notice removal from PR #88. Use the
existing guarded process: `.github/workflows/release-v0.0.23.yml` builds,
checks, hashes, uploads and publishes on a GitHub-hosted Windows runner.
It is `workflow_dispatch` only; pushes and PRs cannot publish it.
Production changes only through a separate, human-merged `stable/release.json`
PR after the published bytes are independently verified.

Use `$R = 'grantcharlie1000-tech/Bowser'` in the commands below.

## 0. Preconditions

- Merge PR #88 only after its review and required checks pass.
- Merge this release-tooling PR separately before the version-bump PR.
- Merge freeze: nothing else merges from the version-bump PR until the stable
  pin PR is merged. This release must include only the verified current main,
  this fullscreen fix, and its release tooling/identity/pin changes.
- No v0.0.23 release or tag exists, including drafts. Inspect both:
  `gh api repos/$R/releases --paginate --jq '.[]|select(.tag_name=="v0.0.23")|{id,draft}'`
  and `gh api repos/$R/git/ref/tags/v0.0.23` (expected 404).
- Main's stable pin is v0.0.22, revision 23. Never overwrite v0.0.22.

## 1. Version-bump PR

Change exactly `Cargo.toml` and the bowser entry in `Cargo.lock` to version
`0.0.23`, and `src/updater.rs` to `const PIN_REVISION: u32 = 24;`.
Do not change the stable pin. Run `cargo test --locked --lib updater::`,
wait for the PR's `windows-verify` checks to pass, and merge with a merge
commit. That exact main commit is `<BUILD_SHA>`.

## 2. Verify the exact main commit

`windows-verify` does not run on pushes. The release gate requires success
on exactly `<BUILD_SHA>`:

```powershell
gh workflow run windows-verify.yml --repo $R --ref main
gh run list --repo $R --workflow windows-verify.yml --event workflow_dispatch --limit 1 --json databaseId,headSha
gh run watch <RUN_ID> --repo $R --exit-status
```

Confirm `headSha` equals `<BUILD_SHA>` and main has not moved.

## 3. Dry run, then publication

```powershell
gh workflow run release-v0.0.23.yml --repo $R --ref main -f commit=<BUILD_SHA> -f dry_run=true
gh run watch <RUN_ID> --repo $R --exit-status
```

The dry run repeats all Windows/Rust/UI/Shield/reproducibility gates,
builds the shipping HTML-default executable, and smokes that exact file in
an isolated profile. The native smoke requires page fullscreen without the
removed notice, Esc exit and restored bounds. Nothing is published.
The gate and the final dry-run verdict must both say the commit is releasable.

Only after that success, dispatch the same workflow on the same main commit
with `-f dry_run=false`. It repeats verification and uses the existing
main-only publisher. It creates a draft by release id, uploads Bowser.exe and
build-metadata.json, downloads both by asset id, checks their bytes/hash and
provenance, then publishes with `make_latest=false`. The stable pin stays put.

A failed/uncertain publication must be inspected before any retry. Existing
drafts stop publication; never delete, replace, retag or blindly retry them.
A failure before the publish job creates no release and can be freshly
dispatched after its cause is fixed.

## 4. Independent artifact verification

Download both assets independently:

```powershell
$d = "$env:TEMP\bowser-v0.0.23-check"
New-Item -ItemType Directory -Force $d | Out-Null
gh release download v0.0.23 --repo $R --pattern Bowser.exe --pattern build-metadata.json --dir $d
(Get-FileHash "$d\Bowser.exe" -Algorithm SHA256).Hash.ToLower()
(Get-Item "$d\Bowser.exe").Length
gh api repos/$R/git/ref/tags/v0.0.23 --jq .object
```

Require the hash, bytes, metadata version/revision/source/run and tag commit
to agree with the publication run. Verify clean-profile, upgraded-copy and
link-launch behavior against the downloaded executable using isolated data;
never use the owner's real profile for tests.

## 5. Stable pin PR: the production switch

Change only `stable/release.json`, preserving its order and four-space indent:
version `0.0.23`, revision `24`, commit `<BUILD_SHA>`, tag `v0.0.23`, repository
`grantcharlie1000-tech/Bowser`, asset `Bowser.exe`, and the independently
verified sha256 and byte length. Check that revision 24 equals PIN_REVISION.
Wait for `stable-package` and all required Windows checks, then merge.
The push verification must pass; `mark-latest` then promotes only the exact
v0.0.23/revision-24 allow-list entry after rechecking the public bytes and pin.
Verify the raw main manifest and the public latest-download route afterward.
Artifact publication alone is not a stable rollout or an installed update.

## 6. Rollback

The updater will not downgrade a user already running v0.0.23. Use a newly
built and verified fix-forward release with a higher version and pin revision.
Its immutable metadata, stable pin and exact latest allow-list entry must agree.

The current verification contract requires the release metadata revision to
equal the stable pin revision. The immutable v0.0.22 artifact records revision
23, so pointing at it with revision 25 would fail verification and is not a
supported rollback. Never alter published metadata or bypass the identity gates.
Any emergency rollback needs a separately designed and reviewed procedure.
Never delete or retag v0.0.22 or v0.0.23.

Leave all older one-time release and recovery workflows unchanged and do not
dispatch them again. See `docs/release-cloud-v0.0.19.md` for historical recovery.
