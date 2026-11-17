# Release procedure

## Versioning
Semantic versioning in `package.json` (`MAJOR.MINOR.PATCH`). `build/build.mjs` stamps the same version into the Chrome manifest and both bundles. Chrome accepts 1 to 4 dot-separated integers (each 0 to 65535), which semver satisfies. Never reuse a version that was uploaded to the Web Store.

- PATCH: lexicon additions, selector drift fixes, bug fixes.
- MINOR: new capabilities (for example v1.1 OCR), new modes, UI changes.
- MAJOR: behaviour or permission changes users must re-approve.

Adding a permission or host access disables the extension for existing users until they accept. Avoid that in minor releases.

## Steps
1. Edit the version in `package.json`.
2. Update the changelog below.
3. `npm test`, `npm run e2e`, `node test/chrome/load-extension.mjs`.
4. `node build/build.mjs`, then `node build/build.mjs --check`. The Bowser bundle `dist/bowser/spoiler-firewall.js` is committed and embedded in Bowser via `include_str!`; commit the regenerated file together with the Bowser change that ships it.
5. `npm run package` (build, validate, zip). Outputs `release/spoiler-firewall-chrome-<version>.zip` and a `.sha256` file.
6. Submit the zip following `submission-checklist.md`.

## Changelog
### 1.0.0 (unreleased)
- First release: text-based spoiler detection, click-to-reveal shields, three caution levels, popup and options page. No OCR.
