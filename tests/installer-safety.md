# Installer safety checks

Run the source-only guardrails on any supported development host:

```sh
python3 -m unittest discover -s tests -p 'test_installer_contract.py' -v
```

On Windows, parse and exercise only extracted helper functions with Windows PowerShell 5.1:

```powershell
powershell.exe -NoProfile -NonInteractive -File tests/installer-safety.ps1
```

The PowerShell suite does **not** run the installer's top-level statements, contact GitHub,
create shortcuts, start Bowser, or access installed browser/profile data. It uses in-memory
streams and its own temporary synthetic files/junction for helper coverage. It tests selected-pin
version/revision refusal, malformed/foreign/oversized pins, unsafe URLs and redirect counts,
actual streaming size boundaries, mocked Authenticode failure statuses, stale install markers,
changed-file replacement refusal, newer/stale install-record refusal, cleared shortcut arguments,
junction ancestors, conservative standalone launch blocking with inert process-list mocks,
and per-user installer-mutex semantics. Mutex tests use synthetic GUID
names only: a tiny C# helper owns a competing background thread to cover busy and abandoned
ownership without another PowerShell process or the production lock.

A selected pin below `-MinimumVersion` or `-MinimumRevision` exits **23** before installation
writes. A demonstrably newer managed install is also refused with code 23 before executable
or shortcut writes. An ambiguous stale managed-install note is a generic failure unless the
current executable exactly matches the selected pin. The native embedded-installer action can therefore distinguish an unpublished/stale
stable pin from a generic installation error. Other exceptions retain a failing process exit.

Setup acquires `Global\BowserStableInstall-<current SID>` before destination preflight and
keeps it through executable/receipt/shortcut writes and the launch decision. A busy installer
fails immediately with a retry message; abandoned ownership is recovered. A `finally` block
releases and disposes the mutex on success, failures, and explicit exits.

Standalone setup never auto-launches while any `Bowser*` process remains, including Downloads,
duplicate-download and unreadable-path copies or an already-running target. It leaves them
running and asks the user to close **all** Bowser windows and use the installed shortcut. The
native `-NoLaunch` route exits before this check or any launch attempt.

These tests do not replace a Windows install smoke test of the signed/unsigned release asset,
shortcuts, existing-process locking, offline fallback and `-NoLaunch` UI integration. Such a smoke
test needs an explicitly isolated Windows test account/VM; do not run it against a user's real
installation just to execute this helper suite.
