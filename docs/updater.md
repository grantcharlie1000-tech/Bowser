# Updater

`src/updater.rs` is a secure foundation, not a finished release pipeline. It stays inert until you finish the three steps below. With no manifest URL it does nothing; with a URL but no embedded public key it is check-only and never downloads or installs anything.

## How it behaves

- Checks at most once per 24 hours (`%LOCALAPPDATA%\Bowser\updates\last-check.txt`), on a background thread that sleeps 45 s after launch first, so it never blocks startup. A launch that survives those 45 s is also treated as healthy.
- Benchmark launches (`--benchmark-*`) never touch update state.
- Fetches the manifest over HTTPS with WinHTTP. Redirects are followed manually (max 5) and each hop must be plain HTTPS. Manifest limit 16 KiB, package limit 256 MiB.
- Refuses: unsigned or tampered manifests, non-HTTPS urls, versions that are not newer than the running build (no downgrade), `min_version` above the running build, size over the limit or not matching, SHA-256 mismatch, and a version that was rolled back earlier.
- Downloads to `%LOCALAPPDATA%\Bowser\updates\bowser-<ver>.download`, verifies size and SHA-256, runs `WinVerifyTrust` (a binary that is Authenticode-signed but not trusted is refused; an unsigned one relies on the manifest signature), then stages `bowser-<ver>.exe` and writes `pending.txt`.
- On the next start the staged file is re-hashed, copied next to the exe, the running exe is renamed to `bowser.old.exe`, the new one is renamed into place, and Bowser relaunches. `bowser.old.exe` is kept as the rollback copy.
- Crash-loop guard: after a swap, `trial.txt` counts launches. If three launches pass without one surviving 45 s, the next start restores `bowser.old.exe` (the bad build is kept as `bowser.bad.exe`) and records the version in `rejected.txt` so it is not offered again.
- The install directory must be writable by the user; otherwise the swap fails and the running build is untouched.
- `updates\status.txt` holds the last check result for diagnosis and for the main menu's footer.
- Automatic updates can be turned off in Settings, Updates, or with **Update automatically** in the main menu's Updates section. The switch is install-wide, in `auto-update.txt` in the data root (`auto_update=false`); a missing or unreadable file means on. When off, the background thread still marks a healthy launch but does not check, and turning it off removes a staged update (`pending.txt` and its `bowser-<ver>.exe`). The rollback guard always runs.
- **Check for updates** (main menu) runs one check at once, ignoring the interval and the switch, and stages a verified build exactly like the background check. Only one check runs at a time. Dev and benchmark launches show the item disabled.
- **Restart to update** appears while a build is staged. It writes `restart.marker` in the profile's session folder, closes the window cleanly and starts Bowser again with the same arguments; that start applies the swap. A marker younger than two minutes makes the next start reopen the saved tabs without the crash prompt; an older one is ignored.

## Remaining release configuration

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
