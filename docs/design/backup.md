# Back up and restore

Settings > About > **Back up and restore**. **Export browsing data** writes one file that a fresh
Bowser can read back with **Import browsing data**: for a reinstall, a Windows reset or a new PC.
Uninstalling keeps `%LOCALAPPDATA%\Bowser` anyway (`Uninstall-Bowser.ps1`); the backup is for when
that folder is gone too.

## What it holds

| Part | Exported from | Restored how |
| --- | --- | --- |
| Cookies (sign-ins, site preferences) | WebView2's cookie manager, every cookie of the profile (`EngineAdapter::export_cookies`) | Added at once (`import_cookies`, `AddOrUpdateCookie`). Expired ones are skipped. An export the engine gives no cookies for is not saved. |
| Saved passwords | `PasswordStore::export_csv` (decrypted from the DPAPI vault) | Merged at once (`preview_csv` + `commit`): only logins the vault lacks are added. |
| Profile files: settings, history, bookmarks, start page, notes, workspaces, sidebar, search engines, site permissions, blocker rules, feeds, the downloads list and the rest of `backup::FILES` | Read from the profile folder | Staged in `restore-pending\`, then moved in place when Bowser restarts, replacing the profile's own. |

Not included, on purpose:

- `session.txt` and `running.lock` (crash recovery).
- `Autofill.dat`, which is locked to the Windows account.
- Extensions, favicons and other caches.
- Web app panels' own cookies: they live in a separate WebView2 environment.
- The WebView2 folder itself; only its cookies travel.

Only names in `backup::FILES` are ever written on restore, so a crafted file cannot put anything
else, or anywhere else, in the profile.

## File

`*.bowserbackup`, password-encrypted (`src/backup/mod.rs`):

```
"BWSRBAK1" | version 1 | kdf 1 | iterations u32 LE | salt [16] | nonce [12] | ciphertext | tag [16]
```

- **Key:** PBKDF2-HMAC-SHA256 of the UTF-8 password, 600,000 iterations. A file may ask for 100,000
  to 10,000,000 iterations and nothing outside that range.
- **Cipher:** AES-256-GCM, with the 42-byte header as associated data. Every byte of the file is
  authenticated, and a wrong password and a damaged file give the same answer.
- **Crypto:** Windows CNG (`src/backup/cng.rs`). No new dependencies. The known-answer tests (PBKDF2
  vectors, GCM spec cases 13 and 14) run on Windows CI.
- **Plaintext:** a list of named entries: the profile files by name, plus `passwords.csv` and
  `cookies.tsv`. Decoding is strict: exact lengths, no unknown or repeated names, no trailing bytes.
- **Memory:** secrets are wiped from memory when dropped (`zeroize`). The page clears the password
  fields as soon as it sends them, and `BackupPassword` never prints its contents.

New backups need a password of at least 8 characters. An existing backup opens with whatever
password it was saved with.

## Flow

**Export**
1. The user types a password twice, then picks Export…
2. The page checks the password and sends `{a:"backupexport",text}`.
3. Bowser shows the save dialog, which suggests `Bowser backup YYYY-MM-DD.bowserbackup`.
4. Bowser saves settings and flushes history, reads the cookies, seals the file and writes it
   atomically. The page then shows "Backup saved".

**Import**
1. The user types the password, then picks Import…, which sends `{a:"backupimport",text}`.
2. Bowser shows the open dialog, decrypts the file and checks every entry.
3. The cookies are added first; if the engine refuses them, nothing else changes. Then the passwords
   are merged and the files are staged.
4. Bowser restarts the same way "Restart to update" does. The closing process moves the staged files
   in after its own last writes (`main.rs`, `backup::apply_pending_restore`), so nothing still in
   memory is saved over them. The new process starts with the restored profile and restores the
   open tabs.

**Guards**
- Cookies are reached through an open web page. If none is open, export, and an import that carries
  cookies, stop before doing anything and ask for one.
- A staged restore older than 15 minutes, or one without its ready marker (an import cut short), is
  dropped untouched instead of applied.
- A guest window can neither export nor import.
- Other windows of the same profile are separate processes and can save their own settings over a
  restore when they close later, so the page asks the user to close them first.

## Not verified live

No live run in a visible window yet:
- the cookie round trip through WebView2, including HttpOnly and partitioned cookies;
- the restart-and-restore sequence.

The pure parts (format, staging, cookie lines, command parsing) are unit-tested.
