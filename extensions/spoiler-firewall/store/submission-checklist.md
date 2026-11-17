# Chrome Web Store submission checklist

Preparation only. Nothing here has been submitted. Approval cannot be assumed.

## Before building
- [ ] `package.json` version bumped (see release.md); changelog updated.
- [ ] `npm test` green; `npm run e2e` and `node test/chrome/load-extension.mjs` green.
- [ ] `node build/build.mjs --check` passes (Bowser bundle fresh).
- [ ] `npm run package` succeeds (build, validate, zip). Record the zip path and SHA-256.

## Developer account
- [ ] Chrome Web Store developer account (one-time registration fee; 2-step verification on).
- [ ] Contact email verified. Declare trader or non-trader status honestly (free hobby tool: non-trader).

## Dashboard: Package
- [ ] Upload `release/spoiler-firewall-chrome-<version>.zip`.
- [ ] Use "Defer publishing" so you control the release moment after approval.

## Dashboard: Store listing
- [ ] Title, summary, description from `listing.md` (no keyword stuffing).
- [ ] Icon 128x128: `store/assets/store-icon-128.png`.
- [ ] Screenshots 1280x800 (at least 3, up to 5) from `store/screenshots/`.
- [ ] Small promo tile 440x280: `store/assets/promo-small-440x280.png`.
- [ ] Category, language, support URL or email, and a public privacy policy URL (host `privacy-policy.md` as a web page and paste the URL). Fill in the contact line in the policy first.

## Dashboard: Privacy practices
- [ ] Single purpose: copy from `permission-justifications.md`.
- [ ] Justify `storage`, `alarms` and the all-sites content scripts (same file). The signed term-update download is explained there under "Term update feed"; the privacy policy "Network" section discloses it. Re-check the Data usage form with it in mind: no user data is sent.
- [ ] Remote code: "No, I am not using remote code".
  - Explain in the notes: bundled WebAssembly (Tesseract, `vendor/`, SHA-256 pinned by `build/validate.mjs`), extension-page CSP `script-src 'self' 'wasm-unsafe-eval'`, no code fetched from a network.
- [ ] Optional permission `offscreen` (image scanning, off by default): justification is in `permission-justifications.md`; privacy policy section "Network" describes the optional image download. No host permission is declared.
- [ ] Data usage: the extension reads page text locally to classify it but collects nothing: no data is transmitted, stored off-device or shared. Re-read the form's current wording at submission time. If "Website content" is defined to include on-device processing, tick it and state it is processed locally only; otherwise leave the categories unticked. Choose the answer that is true; do not guess.
- [ ] Certify the three limited-use statements (not sold to third parties; not used for unrelated purposes; not used for creditworthiness or lending).

## Trademark and policy caution
- Use "GTA VI" only descriptively. No Rockstar or Take-Two logos, game art, screenshots, fonts or trailers anywhere (listing, icons, promo, screenshots).
- Keep "Unofficial" in the name and the disclaimer in the description.
- Do not imply endorsement. Do not use "official", "Rockstar" or "Take-Two" in the name or summary.
- Be prepared for a trademark complaint from the rights holder; respond promptly and do not re-upload a rejected variant to evade it.

## Review expectations
- `<all_urls>` content scripts trigger in-depth review. Realistic timing is several days to a few weeks, sometimes longer after a rejection. The GTA VI launch (2026-11-19) may pass before approval; do not promise users a date.
- Common rejection reasons to pre-empt: unclear single purpose, overbroad justification, missing privacy policy URL, listing text not matching behaviour, trademark use.
- Fallbacks while waiting: the unpacked build (developer-mode install) and Bowser's built-in protection.

## After approval
- [ ] Install from the store into a clean profile and smoke-check (popup, shield on the fixture page).
- [ ] Tag the release in git manually.
