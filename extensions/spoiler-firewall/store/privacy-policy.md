# Privacy policy: Spoiler Firewall for GTA VI (Unofficial)

Last updated: 2026-11-17 (term updates added)

Spoiler Firewall for GTA VI ("the extension") is an unofficial fan tool. It is not affiliated with Rockstar Games or Take-Two Interactive.

## Summary
The extension does not collect, transmit, sell or share any personal data or browsing data. Everything happens on your device.

## What the extension processes
To hide spoilers, the extension reads the text of web pages you visit inside your own browser. This analysis is performed locally by the extension's content scripts. Page text is not saved, logged, transmitted or shown to anyone, including the developer.

## What the extension stores
- Settings in `chrome.storage.local`: whether protection is on, the caution level (Lockdown, Strict, Balanced), the reduced-effects option and whether term updates are downloaded. These stay on your device.
- The last downloaded spoiler term update (see "Network"): a word list with its version number and dates, and the outcome of the latest check. It contains no information about you.
- A temporary per-tab counter (numbers only) in `chrome.storage.session`, used to show how many items were covered on the current page. It contains no page text or URLs and is cleared when the browser closes or the tab is closed.

## Network
The extension has no server of its own, and contains no analytics, advertising, tracking or remote code.

Spoiler term updates (switch "Download spoiler term updates", on by default): so that new character, place and mission names are covered before a new version of the extension can be published, about every 12 hours the extension downloads one small static file (the signed word list) from the public address `raw.githubusercontent.com` (GitHub). The request is an ordinary web request without cookies, without credentials and without a referrer, and carries no identifier, no page text, no browsing data and no information about you; as with any web request, the host can see the IP address it came from. The file is data only, never code. It is checked against a digital signature built into the extension; a file that is unsigned, altered, older than the one already held, or too large is ignored. A valid list can only add words or make a word count for more; it can never remove protection. Switching the setting off stops these downloads (the built-in list and any update already downloaded keep working).

Optional image scanning ("Scan images for text (local OCR)", off by default; you turn it on in the extension settings and Chrome asks you to allow it): the extension downloads the images that are shown on pages you visit (the same files your browser is already displaying), without cookies, directly from the site that serves them, and reads the text in them on your device with a text-recognition engine bundled in the extension. The images and the recognised text are not stored on disk, not sent to the developer or to anyone else, and are discarded when the browser closes (a short-lived in-memory cache avoids reading the same image twice). Switching the option off removes the extra permissions and stops all of this.

## Third parties
No data is shared with any third party.

## Your control
Turn protection off or change settings in the extension popup. Uninstalling the extension removes all stored settings.

## Children
The extension does not knowingly collect information from anyone, including children.

## Changes
If this policy changes, the new version will be published with the extension update and the date above will change.

## Contact
FILL IN BEFORE SUBMISSION: support email or issue tracker URL.
