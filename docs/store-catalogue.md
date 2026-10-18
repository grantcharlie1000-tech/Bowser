# Bowser Store catalogue and evidence

Discovery now uses the original Chrome Web Store in an ordinary, unprivileged top-level browsing
view. `bowser://store` and the Extensions settings action open Google's real catalogue/search;
Bowser does not clone it, frame it, scrape its artwork or give it a host bridge. Actual availability
in WebView2 remains a Windows acceptance gate, not an assumption.

Bowser's own **Add to Bowser** control is outside the website and is offered only for a validated,
committed current top-level listing. It revalidates the active nonprivate, nonloading tab at click
time. It opens Bowser's trusted resolved-package/permission review; website script cannot install.
Installed management remains in Extensions settings. The older native catalogue code is retained
only as part of the internal review surface during this transition, not the default discovery UI.
The four curated entries in `src/store.rs` remain provenance/test metadata, without invented ratings,
review counts, installation counts, search API or guarantee of Chrome compatibility.

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
| Stylus | 2.4.14 | Genuine popup; no declared options page; genuine manager imported a disposable style through its own DOM after diagnostic same-extension hyperlink navigation; exact-URL CSS effect, unmatched-URL exclusion, new-executable-process restart, and real removal restoring original appearance | Partial |
| Tab Session Manager | 7.4.0 | Genuine popup/options; genuine Save saved one Bowser window/two tabs as two native windows/two tabs; correct grouping and restoration not demonstrated | Untested |

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

Later exact signed Stylus2.4.14 execution proved imported CSS survives a new executable process
and real removal restores the original appearance. TSM7.4.0's genuine Save button saved two
Bowser tabs in one window as “2 Windows - 2 Tabs”, demonstrating incorrect Bowser-window grouping.
[Run39092946278](https://github.com/grantcharlie1000-tech/Bowser/actions/runs/39092946278),
head722aad1f5f6d875d776059a56e8b5a803060147f, testedmergeaa1c44940dcc6871d0214805a6286b0064b9efa4.
Early real-package artifact12124020376 SHA256
2c361f96b6c213d316cb70808efdad1371c73ba25e6302ef6180801e50192a5d.
No restore-current command was used. These observations do not establish Bitwarden autofill or
TSM restoration, and no guessed IDs or native API replacement were used.

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
- Untested: Bitwarden dummy-credential use; normal Stylus manager-routing integration; actual
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

### Exact worker-current-window prerequisite (test-only)

The independent `live_probe_native_worker_current_query` fixture uses a genuine named Ctrl+Shift+X command. Before any event-handler injection, its real MV3 worker calls `tabs.query({active: true, windowId: chrome.windows.WINDOW_ID_CURRENT})`, matching the targeting query used by Bitwarden browser-v2026.10.0. Only the unique returned native ID is an injection target; the untouched command-event tab is independent comparison evidence, never a fallback. Wrong/empty/ambiguous results fail. The exact focused duplicate-URL COM-owned view must receive the per-event marker, with the existing settled pre-gesture, no-activeTab and cross-origin permission controls retained. The original named-command fixture remains separate.

This is a warm-worker, observer-popup-present, unbound Ctrl+Shift+X prerequisite probe. It is not Bitwarden autofill acceptance, a closed-popup/cold-worker test, or proof of the actual Ctrl+Shift+L assignment/conflict path. No vault credentials, command synthesis, Chrome API replacement or production shortcut routing is involved.

### Native Ctrl+Shift+L conflict prerequisite (test-only)

`live_probe_native_ctrl_l_conflict` changes only a disposable fixture manifest before installation to request Ctrl+Shift+L. It first proves Bowser's unchanged default FillPassword binding actually consumes the real chord, no native extension event arrives and injection is still denied. It then uses the existing command Store API to remove only that binding, saves/reloads the disposable profile's keybindings, and repeats the complete query-first targeting/permission controls. An unwind guard restores the prior process-local command configuration. No product shortcut routing, extension package code or real user settings are changed. The real signed Bitwarden and Tab Session Manager contexts separately report their native command assignments without invoking them. These are warm-worker prerequisite diagnostics, not vault/autofill, cold-worker or combined-package shortcut compatibility acceptance.

The default conflict control observes no event during a fixed two-second warm-worker interval, successful native SetHandled and continued permission denial. Its disposable Static host exercises the production engine accelerator path, not the full App FillPassword operation or a Settings UI gesture. The explicit binding choice uses the existing saved Store API and does not establish combined-package assignment behavior.

### Native API isolation fixture (pending review and Windows execution)

The separate `live_probe_native_api_isolation` fixture declares broad host access only in disposable test profiles. Its genuine extension context attempts native executeScript/sendMessage against every integral ID returned by its exact bounded native tabs.query, without guessed IDs or URL/title/order mapping. The real normal view must prove both effects after a separately observed static-content-listener positive. Actual private, other-profile and privileged Settings views must be ready and remain unaffected; incognito/private/UI metadata exposure is also rejected. UI scheme policy and profile separation are distinct possible enforcement reasons. This is a narrow API/effect test, not proof every extension API is isolated or permission authority for production code. No website bridge, user credential or production permission setting is introduced.

The fixture explicitly supplies the production private blocker and checks the native controller profile name/InPrivate flags. The normal positive is bound by a unique per-returned-ID effect token plus successful native executeScript result and received:true message reply, never a URL-derived identity. Metadata checks reject returned incognito:true and known controlled private/other-profile/UI URLs; a redacted returned identifier is not proven non-private, and this fixture does not claim every private identifier is hidden.

### Genuine context-menu observation prerequisite (pending Windows)

The separate keyed contextMenus/scripting/activeTab fixture has no host/tabs/content-script access.
A genuine creation callback is only the first prerequisite. The isolated single-test gate then requires
a no-input baseline, real right-click on the exact owned textarea, trusted page receipt, exact native
receiving view/IsEditable and complete bounded recursive menu metadata. It never inserts/selects a
host item or writes Handled/SelectedCommandId. One real Escape is allowed only for the positively
owned unchanged visible native popup; disappearance and cleared native popup mode must both follow.
Unknown topology fails without a global Escape/EndMenu. Bounded failure evidence is saved before
termination through the retained original disposable single-test process handle; this stays a failure.
Zero native onClicked events, unchanged document/effects and fresh permission-specific denials for
the original native preflight IDs are mandatory. This warm-worker/observer-present/top-level negative
control does not establish selection grants, actual Bitwarden menus/autofill, iframe or cold-worker use.

### Bowser-owned installer guidance

The validated listing action is a labeled Add to Bowser control in Bowser’s toolbar, not in Google’s document. Its visible note explains that Google installs into Chrome and Bowser support varies. Google’s original Chrome-only notice is neither hidden nor rewritten. Bowser still revalidates the exact successful top-level listing at click time, verifies the signed package and opens its independent permission review; the label does not promise extension compatibility.

Current visual acceptance is limited to the default 100% UI font scale. The two-line installer copy still requires an adaptive layout at enlarged supported font scales: the fixed-height toolbar cannot fit both scaled lines at 200%. This remains an unreleased UX blocker, not a verified supported-scale result. The native journey requires both actual Text rectangles inside the owned button; accessible names alone cannot pass it.
