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

| Extension | Version | Native context result | Compatibility label |
| --- | --- | --- | --- |
| uBlock Origin Lite | 2026.1012.1545 | Genuine popup/options; DNR API object present | Untested |
| Bitwarden | 2026.10.0 | Genuine popup/options; storage/runtime/tabs/windows present | Untested |
| Stylus | 2.4.14 | Genuine popup/options | Untested |
| Tab Session Manager | 7.4.0 | Genuine popup/options | Untested |

Evidence: [open-license runtime probe](https://github.com/grantcharlie1000-tech/Bowser/actions/runs/39086062781).
The returned runtime ID matched the signing-derived Store ID in every case.

This proves neither password autofill nor CSS injection nor ad blocking nor session restore.
The same run found the browsing WebView and extension popup both reported as active tabs in
separate native windows. Correct Bowser-tab targeting is therefore a known unresolved blocker,
not a passing compatibility claim. No website receives a fake `window.chrome`, privileged host
object, debug port, or private Bowser UI bridge.

Earlier diagnostic runs used Dark Reader 4.9.133 and OneTab 2.24. They are not current catalogue
or future acceptance-test candidates. The Dark Reader run actually loaded its popup/options;
its terms' implicit-use acceptance was disclosed separately. This is not redistribution approval.

## Acceptance status and evidence discipline

- Tested: bounded update protocol; CRX3/signing identity; safe verified unpack; genuine native
  extension contexts for the versions above; review guards in test suites (new mid-transfer cancellation tests await CI).
- Partial: persistent profile load/restart fixture, popup/options hosting, enable/remove and
  storage APIs. A generic API object is not a functional application test.
- Untested: real Store search → detail → consent → installation → actual task → restart → removal;
  password-manager dummy-credential use; real CSS application; blocker network effect; tab save/
  restore; profile/private isolation across the complete journey; zero/several-extension RAM,
  startup and page-load comparison.
- Unsupported pending implementation/evidence: guaranteed correct Bowser-tab targeting, action
  click/activeTab forwarding, commands/context-menu integration, full Chrome API parity, production
  verified staged updates with permission-increase consent and rollback.

The focused workflow also captures renderer screenshots using a fake native boundary in a real
Edge browser. Those are rendering/interaction evidence only, never native installation evidence.
The ignored immutable update test is a local same-key fixture feasibility experiment, not the
signed production updater. Experimental adapters remain disabled; “Check for update” gives an
honest unavailable explanation instead of a fake success.

Do not mark the product requirement complete or merge/release on the strength of download/load
success alone. Formal security review and actual Windows acceptance evidence remain required.
