# Bowser Store catalogue and evidence

`bowser://store` is a Bowser-owned Discover / Categories / Installed page, not a replacement
rendering of Google's website. The four entries in `src/store.rs` are manually curated links to
real public listings. They have no invented ratings, review counts, installation counts, search
API, or guarantee of Chrome compatibility. “Browse Chrome Web Store” opens Google's ordinary
unprivileged website. A native Bowser Add control is offered only for a validated current
top-level listing and revalidates the current tab at click time. Website script cannot install.

## Provenance and refresh

Each entry records its exact store ID, primary listing, publisher attribution, official project
homepage, category, and date checked. Publisher attribution is catalogue metadata, not a
cryptographic assertion. For a downloaded package, the review instead identifies its signing
identity, actual resolved manifest name/version and complete declared API/host/content-script/
optional permission lists. A signature proves integrity and possession of the identity's key;
it does not certify a publisher or make requested permissions safe.

To refresh: inspect the primary listing and official project, retain its exact 32-character ID,
check source/license provenance, update the checked date, and run the focused Windows workflow.
Do not infer compatibility from a newer version of a previously tested extension. A compatibility
label needs exact extension version + WebView2 version + actual task evidence. Unlisted valid
listings remain installable only with explicit review and an unknown/unverified publisher label.

Appearance and tab-management test candidates are the openly licensed Stylus (GPL-3.0,
https://github.com/openstyles/stylus) and Tab Session Manager (MPL-2.0,
https://github.com/sienori/Tab-Session-Manager). No extension package is committed or uploaded as
an artifact. Temporary downloaded and unpacked copies are deleted after disposable-profile tests.

## Observed 2026-10-18 results

All results below used detected WebView2 154.0.4258.62 and the existing unbranded Chromium update
protocol, not a spoofed Chrome identity. Manifest HTTP 200 / updatecheck `ok`, package HTTP 200,
CRX3 verification, unpack, and `AddBrowserExtension` passed for these versions:

| Extension | Version | Observed actual functionality | Compatibility label |
| --- | --- | --- | --- |
| uBlock Origin Lite | 2026.1012.1545 | Genuine popup/options; default ad-domain request blocked while zero-extension baseline received a response and the normal control loaded; native DNR rule add/remove effect | Partial |
| Bitwarden | 2026.10.0 | Genuine popup intro screen, storage/runtime/tabs/windows; no declared options page; dummy-vault autofill not tested | Untested |
| Stylus | 2.4.14 | Genuine popup; no declared options page; genuine manager imported a disposable style through its own DOM after diagnostic same-extension hyperlink navigation; exact-URL CSS effect, unmatched-URL exclusion and real removal restored original appearance | Partial |
| Tab Session Manager | 7.4.0 | Genuine popup/options; native windows do not correspond to Bowser's tab grouping; actual save/restore not demonstrated | Untested |

Evidence: [exact real-package probe](https://github.com/grantcharlie1000-tech/Bowser/actions/runs/39091891946),
head df612ffadefb3bfe7a4851c284923dc33f07be23, tested merge
1301b0468817d40d46f8bea89766f8a7e3fb2099. Early real-package diagnostic artifact
12122619785 SHA256 f5e61ae1b05a2dba25038f34694c8cb5e9bcd7f108f388f293c4c2c0dcadd267.
The returned runtime ID matched the signing-derived Store ID in every case.

The uBO result is one actual default-filter network effect, not universal filtering or correct
popup/action targeting. The same run found browsing views and the hosted extension popup
reported as active tabs in separate native windows. Correct Bowser-tab targeting remains a
known unresolved blocker. No website receives a fake `window.chrome`, privileged host object,
debug port, or private Bowser UI bridge. Bitwarden and Stylus do not declare options pages in
these packages; earlier generic popup/options wording was too broad.

Earlier diagnostic runs used Dark Reader 4.9.133 and OneTab 2.24. They are not current catalogue
or future acceptance-test candidates. The Dark Reader run actually loaded its popup/options;
its terms' implicit-use acceptance was disclosed separately. This is not redistribution approval.

## Acceptance status and evidence discipline

- Tested: bounded update protocol; CRX3/signing identity; safe verified unpack; native Store
  search → details → resolved full review → cancellation without record → explicit consent →
  install → actual clean-process restart with unique loopback readiness → genuine popup →
  durable Disable/Enable → explicit native removal. This is lifecycle evidence, not complete
  extension functionality. See [native journey](https://github.com/grantcharlie1000-tech/Bowser/actions/runs/39091043544).
- Tested narrow safety controls: real pending native inventory/Add callbacks versus a second
  process return explicit busy with unchanged records; four exact signed/keyed process-crash
  cut points recover prior consented enabled state plus actual worker/content messaging.
  [Control run](https://github.com/grantcharlie1000-tech/Bowser/actions/runs/39091322998).
  Interrupted/failed revocations are not accepted; this is not power-loss or keyless ownership proof.
- Partial: uBO default-filter effect above; native fixture storage/worker restart and immutable
  same-key update/rollback feasibility. Completed durable Disable and Remove remained off/absent
  in separate executable processes with fresh native inventory, in fully green
  [run 39091636525](https://github.com/grantcharlie1000-tech/Bowser/actions/runs/39091636525),
  head 26ac1ffe864fe7a53ff6bd3f0745b69226221745, tested merge fca7d460c23000f4e984c95cccfaa18bba256909.
  Stale-owner replacement, damaged-authority real-environment exclusion and private/profile negatives
  remain distinct pending runtime gates.
- Untested: Bitwarden dummy-credential use; real Stylus CSS application/removal/restart; actual
  Bowser-tab session save/restore; complete private/UI isolation. A 6-row cold/warm zero/four
  release-executable diagnostic series completed in the same green run, preserving process-tree
  identities and K32 PrivateUsage; both arms have diagnostic tracing enabled and use --dev, so
  this is not clean production latency or a performance-improvement claim. Working-set sums
  are not PSS. Artifact 12123297301 SHA256
  1b59be1d6556fce4d971c4ad36fbbdf759be500577b99419e0275be9b5c7cd31.
- Unsupported pending implementation/evidence: guaranteed correct Bowser-tab targeting, general
  action click/activeTab forwarding, commands/context-menu integration, full Chrome API parity,
  production verified staged updates with permission-increase consent and rollback. The researched
  browser-target CDP route returned explicit `Not allowed`; it is not retried or bypassed.

The focused workflow also captures renderer screenshots using a fake native boundary in a real
Edge browser. Those are rendering/interaction evidence only, never native installation evidence.
The ignored immutable update test is a local same-key fixture feasibility experiment, not the
signed production updater. Experimental adapters remain disabled; “Check for update” gives an
honest unavailable explanation instead of a fake success.

Do not mark the product requirement complete or merge/release on the strength of download/load
success alone. Formal security review and actual Windows acceptance evidence remain required.
