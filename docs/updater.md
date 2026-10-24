# Updater

Installed shipping builds use the fixed GitHub `main/stable/release.json` pin by default. An optional configured signed-manifest channel takes precedence; without its embedded public key that channel is check-only. Neither channel updates an executable in Downloads, Desktop, a source/target directory or another arbitrary location.

## GitHub downloads and installation recovery

- Normal downloaded copies show **Install Bowser**, not the development/benchmark refusal. Explicit `--dev`, benchmark flags, debug builds and `target`/`Bowser Dev` paths remain excluded. Missing Windows application-data paths have their own unavailable explanation. Downloaded launches with custom data-root overrides cannot install, because a clean shortcut must not silently switch data or automatic-update preferences.
- Installation requires an explicit click. The worker writes **compiled-in** `Open-Bowser.ps1` and the compiled local pin into an atomically new temporary directory, holding read locks that prevent payload write/delete access while setup runs. It invokes Windows PowerShell from the Windows system directory with argument-based `-NoProfile -NonInteractive -ExecutionPolicy Bypass -File`, `-NoLaunch`, the running build's minimum version and its compiled minimum pin revision. No adjacent/downloaded script, `SourceExe`, custom install directory or browser launch argument is forwarded. The execution-policy override is process-only; machine/user policies and enterprise application controls are unchanged.
- The installer accepts only the fixed repository and `Bowser.exe` asset, bounds the pin to 16 KiB and streams at most the declared package size (256 MiB maximum). Every redirect remains plain HTTPS (five maximum). SHA-256, exact size, executable-header sanity and Authenticode status are checked; invalid/untrusted signatures are rejected. Reparse paths are refused and unrelated existing executables are not overwritten.
- A selected pin older than the running build/minimum revision returns exit code 23 before installation. A stale embedded fallback cannot silently downgrade a newer download, including when GitHub is unavailable. The source and the published pin can temporarily differ; publishing the matching verified release/pin is a separate release task.
- Installation creates update-enabled install-folder shortcuts, but never starts another browser while the downloaded copy owns its profile. On success close **all** Bowser windows and use the new shortcut. The download remains unchanged, and existing profile data and the automatic-update preference are preserved. Repeated clicks are serialized in-process; a per-user global named mutex serializes cooperating installer processes across sessions from candidate preflight through receipt/shortcut writes. Busy or uncertain ownership fails closed, while an abandoned mutex is recovered. Existing native updater swap locks are separate. Failures offer retry and expose the setup error.
- This is a source fix for future builds, not a retrofit into old executables. A pre-fix Downloads copy (such as v0.0.21) first closes all Bowser windows, then needs one run of the official `Open-Bowser.cmd` installer from an extracted checkout/ZIP and must then be opened using the installed shortcut. No access to that computer is needed to prepare this fix, but its old executable cannot install an updater it never contained.

The installed-only `startup` and `spawn_background` gates are unchanged in scope: installation recovery never writes shared update pending/trial records or swaps a downloaded executable.

## How it behaves

- The default GitHub pin is checked at most once an hour; an optional signed channel at most once per 24 hours (`%LOCALAPPDATA%\Bowser\updates\last-check.txt`). The background thread sleeps 45 s after launch first, so it never blocks startup. A launch that survives those 45 s is also treated as healthy.
- Benchmark launches (`--benchmark-*`), development and debug builds never touch update state.
- Fetches the manifest over HTTPS with WinHTTP. Redirects are followed manually (max 5) and each hop must be plain HTTPS. Manifest limit 16 KiB, package limit 256 MiB.
- A signed channel refuses unsigned or tampered manifests, non-HTTPS URLs, versions that are not newer than the running build, and `min_version` above the running build. The default GitHub channel additionally permits a same-version replacement only when the fixed pin names different bytes; its compiled revision floor and version check refuse stale pins/downgrades. Both updater channels enforce size/SHA checks and refuse previously rolled-back builds.
- Downloads to `%LOCALAPPDATA%\Bowser\updates\bowser-<ver>.download`, verifies size and SHA-256, runs `WinVerifyTrust` (a binary that is Authenticode-signed but not trusted is refused; an unsigned one relies on the signed manifest or the fixed GitHub pin fetched over HTTPS), then stages `bowser-<ver>.exe` and writes `pending.txt`.
- On the next start the staged file is re-hashed, copied next to the exe, the running exe is renamed to `bowser.old.exe`, the new one is renamed into place, and Bowser relaunches. `bowser.old.exe` is kept as the rollback copy.
- Crash-loop guard: after a swap, `trial.txt` counts launches. If three launches pass without one surviving 45 s, the next start restores `bowser.old.exe` (the bad build is kept as `bowser.bad.exe`) and records the version in `rejected.txt` so it is not offered again.
- The install directory must be writable by the user; otherwise the swap fails and the running build is untouched.
- `updates\status.txt` holds the last check result for diagnosis and for the main menu's footer.
- Automatic updates can be turned off in Settings, Updates. The switch is install-wide, in `auto-update.txt` in the data root (`auto_update=false`); a missing or unreadable file means on. When off, the background thread still marks a healthy launch but does not check, and turning it off removes a staged update (`pending.txt` and its `bowser-<ver>.exe`). The rollback guard always runs.
- **Check for updates** (Settings, Updates) runs one check at once, ignoring the interval and the switch, and stages a verified build exactly like the background check. Only one check runs at a time. Dev and benchmark launches show the item disabled.
- **Restart to update** appears while a build is staged. It writes `restart.marker` in the profile's session folder, closes the window cleanly and starts Bowser again with the same arguments; that start applies the swap. A marker younger than two minutes makes the next start reopen the saved tabs without the crash prompt; an older one is ignored.

## Optional signed-channel release configuration

1. **Generate a signing key (ECDSA P-256).** Keep the private key offline. For example with OpenSSL:
   `openssl ecparam -name prime256v1 -genkey -noout -out update-signing.pem`
   Export the public key as 64 raw bytes X||Y in hex (128 hex characters): `openssl ec -in update-signing.pem -pubout -outform DER | tail -c 64 | xxd -p -c 128`.
2. **Embed the public key and default URL at build time** (no source edit needed):
   `set BOWSER_UPDATE_PUBKEY=<128 hex>` and optionally `set BOWSER_UPDATE_URL=https://updates.example.com/bowser/manifest.txt`, then `cargo build --release --locked`. Users can override the URL with `%LOCALAPPDATA%\Bowser\update-url.txt` (one HTTPS line); the key cannot be overridden.
3. **Publish a signed manifest** next to the exe on any HTTPS host. Body lines, LF endings only:
   ```
   bowser-update-manifest-v1
   version=0.2.0
   min_version=0.0.1
   url=https://updates.example.com/bowser/bowser-0.2.0.exe
   sha256=<64 hex of the exe>
   size=<bytes>
   notes=One line of release notes
   ```
   Compute `SHA-256(body)` over the exact body bytes (including the final newline), sign it with ECDSA P-256 as a 64-byte r||s (IEEE P1363) signature, and append `signature=<128 hex>` as the final line. With OpenSSL the default DER signature must be converted to r||s. A small signing script is not included yet; verify it end to end against a test key before the first release.

Not yet done: a signing/publishing script; Authenticode signing of releases. Only `bowser.exe` is replaced by the swap, so any other file shipped alongside it (such as a changed `bowser.manifest`) must be handled by the installer.

## Regression checks

`cargo test --locked` covers installed-vs-downloaded/development classification, path failures, embedded payload cleanup and argument construction without running setup. The existing pin/package tests retain version/revision, rejected-build, SHA/size and signature coverage. UI tests cover strict states/results, explicit install, repeated clicks, away/back, retries and the automatic-update-off explanation.

`pwsh -NoProfile -File tests/installer-safety.ps1` parses the installer and evaluates only extracted helper functions with in-memory/mocked transfers, signatures and temporary files. It does not run the top-level installer, network requests, browser or shortcuts. `python -m unittest discover -s tests -p test_installer_contract.py` provides nonexecuting source-contract checks when Windows PowerShell is unavailable. Windows-native compilation, real SmartScreen/policy handling, shortcuts and end-to-end installation still need an isolated Windows validation before a release.
