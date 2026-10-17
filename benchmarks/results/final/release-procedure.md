# How Bowser releases to production (read-only record, written for v0.0.19)

Written during the final pass of the Vivaldi port. Nothing below was performed: no tag, no version bump, no
merge to `main`, no pin change. It records what the repository does today, from PRODUCT.md, README.md,
docs/updater.md, `stable/release.json`, `Open-Bowser.ps1`, `Update-Bowser.ps1`, `Start-Bowser.ps1`,
`src/updater.rs` and the merged PRs #13 to #25.

## What "production" is

- There is no build server. Production is the GitHub release **asset** `Bowser.exe` on
  `grantcharlie1000-tech/Bowser`, built on the owner's PC with
  `cargo build --release --locked --bin bowser --features html-chrome-default` (the exact command
  `Update-Bowser.ps1` runs; `--features html-chrome-default` makes the HTML chrome the default).
- **Stable** is the pin file `stable/release.json` on `main`:
  ```json
  { "version": "0.0.18", "revision": 19, "commit": "<sha built>", "tag": "v0.0.18",
    "repository": "grantcharlie1000-tech/Bowser", "asset": "Bowser.exe",
    "sha256": "<sha256 of Bowser.exe>", "bytes": 5762048 }
  ```
  Two readers follow it:
  - `Open-Bowser.cmd` / `Open-Bowser.ps1` (never builds): fetches the pin from
    `https://raw.githubusercontent.com/grantcharlie1000-tech/Bowser/main/stable/release.json`
    (falls back to the local copy when GitHub is unreachable or its `revision` is lower), downloads
    `https://github.com/<repository>/releases/download/<tag>/<asset>`, refuses anything whose
    SHA-256 or byte count differs from the pin, installs to `%LOCALAPPDATA%\Programs\Bowser`
    (or `Bowser Stable`, `Bowser Stable 2`, ... when the file is in use) and writes `STABLE.txt`.
  - The installed browser (`src/updater.rs`, `stage_github_pin`): with no signed update channel
    configured it reads the same raw URL about a minute after launch and then at most hourly,
    and stages the asset when the pin's SHA-256 differs from the running executable's. It refuses a
    pin that names another repository or asset, a tag not starting with `v`, a `version` lower than
    the running one, a `revision` lower than the compiled-in `PIN_REVISION`, or a SHA-256 it rolled
    back from earlier. The staged build is swapped in at the next start (crash-loop guard keeps
    `bowser.old.exe`).
- `Update-Bowser.ps1` is the owner's local installer from source (builds and replaces
  `%LOCALAPPDATA%\Programs\Bowser\Bowser.exe`); `Start-Bowser.ps1` / `Dev-Bowser.ps1` build dev copies
  with their own profile. Neither publishes anything.
- The signed-manifest channel in docs/updater.md (`BOWSER_UPDATE_URL` / `BOWSER_UPDATE_PUBKEY`) is not
  configured; `Update-Bowser.ps1` clears both variables for the build. Releases are not
  Authenticode-signed (README: SmartScreen "More info, Run anyway").

## What must agree (enforced by tests)

- `src/updater.rs::the_shown_version_follows_the_pinned_release`: `stable/release.json` `tag` must equal
  `"v" + version`, and `CARGO_PKG_VERSION` (Cargo.toml `version`) must be `>=` the pinned version.
  So Cargo.toml and Cargo.lock move to the new number **before or with** the pin, never after.
- `PIN_REVISION` (`src/updater.rs`, currently `19`) must be bumped in the commit that is built and
  released, and `release.json` `revision` must equal that number: a browser running the new build then
  ignores any cached older pin, and a browser on the old build accepts the new one. History: v0.0.15 =
  revision 16, v0.0.16 = 17, v0.0.17 = 18, v0.0.18 = 19, so **v0.0.19 = revision 20**.
- The `commit` in the pin is the commit the asset was built from, and the tag points at that commit.
  For v0.0.18: tag `v0.0.18` -> `fb9059fa` ("Bump the production pin revision ..."), which is the
  pin branch's first commit; the second commit `d314e534` changes only `stable/release.json`.
- `ui/dist` and `ui/dist.manifest` must match (`build.rs` panics otherwise), so the release commit
  carries a rebuilt `ui/dist` (`cd ui; npm run build`).

## The pattern of PRs #13 to #25 (two PRs per release)

1. **Feature PR** (for example #24 "Lift tabs while dragging ... (v0.0.18)", head `t3/8d433813`):
   the work plus `Cargo.toml`/`Cargo.lock` bumped to the new version (v0.0.18 was the first release
   whose Cargo version equals the tag; earlier builds reported 0.0.1). Merged to `main` by the user.
2. **Pin PR** (for example #25 "Pin Stable to the v0.0.18 tab-lift and four-pane split build",
   head `t3/pin-v0.0.18`), two commits:
   - "Bump the production pin revision ..." : `src/updater.rs` `PIN_REVISION` +1 (one line).
     This commit is built, tagged and released.
   - "Pin Stable to the vX.Y.Z ... build." : `stable/release.json` rewritten with the new version,
     revision, commit, tag, sha256 and bytes of the uploaded asset.
   The PR body states the asset size, SHA-256 prefix, the build commit and the revision, and says
   "Merging this is what makes installed Bowser offer the update." Earlier pins (#13, #15, #17, #19)
   changed only `stable/release.json` because the revision had been bumped in the feature PR.

## Exact steps for v0.0.19 (not performed)

Preconditions: `vivaldi/final` (PR to `main`) reviewed and merged by the user; `cargo test --locked`,
`cargo clippy --locked -- -D warnings`, `cd ui; npm run build && npm run lint && npm run typecheck &&
npm test` green on `main`; the real-app checklist in the PR walked through.

1. On a branch from `origin/main` (name `t3/pin-v0.0.19` to match the pattern):
   - `Cargo.toml`: `version = "0.0.19"`; run `cargo update -p bowser --offline` (or edit
     `Cargo.lock`'s `bowser` package version to `0.0.19`) so `--locked` still passes.
     (If the feature PR already bumped it, skip.)
   - `src/updater.rs`: `const PIN_REVISION: u32 = 20;`
   - `cd ui && npm run build` if `ui/src` changed since the last `ui/dist` rebuild; commit `ui/dist`
     and `ui/dist.manifest` with it.
   - Commit: "Bump the production pin revision for the Vivaldi port build." Note its SHA (`BUILD_SHA`).
2. Build from exactly that commit, clean tree (`git status` empty):
   `cargo build --release --locked --bin bowser --features html-chrome-default`
   -> `target\release\bowser.exe`. Smoke-test it from a throwaway data dir
   (`target\release\bowser.exe --data-dir C:\temp\bowser-v19`), not the real profile.
3. Record the asset: `Get-FileHash target\release\bowser.exe -Algorithm SHA256` (lower-case hex) and
   `(Get-Item target\release\bowser.exe).Length`.
4. Tag and publish the release (the tag must point at `BUILD_SHA`):
   `git tag v0.0.19 BUILD_SHA && git push origin v0.0.19`
   `gh release create v0.0.19 target\release\bowser.exe#Bowser.exe --title "Bowser v0.0.19" --notes "<one paragraph>"`
   (The asset name must be exactly `Bowser.exe`; the release target is `BUILD_SHA`.)
   Download the asset back and confirm the SHA-256 and size match step 3.
5. Second commit on the same branch, `stable/release.json` only:
   `version` `0.0.19`, `revision` `20`, `commit` `BUILD_SHA` (full 40 hex), `tag` `v0.0.19`,
   `repository` `grantcharlie1000-tech/Bowser`, `asset` `Bowser.exe`, `sha256` and `bytes` from step 3.
   Message: "Pin Stable to the v0.0.19 Vivaldi port build."
   Run `cargo test --locked --lib updater::` (checks tag/version/revision agreement).
6. Push the branch, open the pin PR against `main` titled
   "Pin Stable to the v0.0.19 Vivaldi port build" with the body pattern of #25 (asset bytes, sha256
   prefix, build commit, PIN_REVISION 20, "Merging this is what makes installed Bowser offer the
   update"). The user merges it.
7. After the merge: `Open-Bowser.cmd` on a second machine (or `-InstallDirectory` to a test folder)
   must report "Downloading stable Bowser 0.0.19" and install the pinned hash; an installed v0.0.18
   offers "Restart to update" within the hour (`%LOCALAPPDATA%\Bowser\updates\status.txt`).
8. Optional housekeeping (not done in #13 to #25): README.md "Stable download" still links v0.0.5 and
   the heading says "Bowser 0.0.1"; update both to v0.0.19 in the pin PR if wanted.

Rollback: merge a pin PR that points back at the v0.0.18 asset with `revision` 21 (a lower revision is
ignored by the new build) and a `version` not lower than the running one; the browser refuses to move
to a lower version, so a true downgrade needs a reinstall through `Open-Bowser.cmd` with that pin.
