# Chrome Web Store listing

## Name (manifest `name`, max 75)
Spoiler Firewall for GTA VI (Unofficial)

## Summary (max 132 characters; manifest `description`)
Hides GTA VI spoilers on the pages you visit, locally in your browser. Unofficial fan tool; no data leaves your device.

## Category
Productivity (alternative: Fun)

## Language
English

## Detailed description

Spoiler Firewall for GTA VI hides story spoilers for Grand Theft Auto VI while you browse, so you can keep using social media, forums, search and video sites without stumbling on something you did not want to know.

This is an unofficial, fan-made tool. It is not made, endorsed or sponsored by Rockstar Games or Take-Two Interactive. "GTA VI" is used only to describe what the extension filters.

HOW IT WORKS
- Text on the page is checked on your computer as it loads and as new content appears (infinite scroll, comments, embedded frames).
- Anything that looks like a GTA VI spoiler is covered with a clearly labelled cover. Click it (or press Enter) to reveal that one item. Reveal again hides it. Nothing else is revealed, and reveals are forgotten when you leave the page.
- If the extension fails internally it covers the page rather than showing it.

THREE LEVELS OF CAUTION
- Lockdown: hides anything that could be about GTA VI. Most cautious; hides some harmless content.
- Strict (default, recommended): hides GTA VI posts unless they are clearly release news, and hides likely spoilers in GTA conversations.
- Balanced: hides likely spoilers only; plain mentions stay visible.

PRIVACY
- Runs entirely on your device. No accounts, no analytics, no servers. Nothing is uploaded: page text is analysed locally and never leaves the browser.
- Optional (on by default, one switch): about every 12 hours it downloads a small signed word list so new names and missions are covered before the next extension update. The request carries no browsing data, the list is verified against a built-in signature, and it can only add protection.
- Stores only your settings (on/off, caution level, reduced effects) and a temporary per-tab counter that is cleared when the browser closes. Page text is never stored or sent anywhere.

HONEST LIMITATIONS
- Text inside images is read only if you switch on "Scan images for text (local OCR)" in settings (off by default; Chrome asks you to allow it). It reads English, Spanish, French, German, Portuguese, Italian and Russian, on your device. It cannot judge pictures without readable text. Without it, a screenshot or meme is covered only if the surrounding text gives it away or Lockdown is on.
- Video and audio are not listened to; titles, descriptions and captions are checked.
- The address bar, tab titles, browser history and desktop notifications are outside what an extension can change.
- Tabs that were open before you installed the extension are unprotected until reloaded. Browser pages and the Chrome Web Store cannot be protected.
- In Incognito, enable the extension in chrome://extensions first.
- No filter is perfect: it can miss unusual wording and can cover harmless content.

PERMISSIONS
- "Read and change all your data on all websites" (content scripts on all pages): required because spoilers can appear on any site. Page content is analysed locally and never leaves your device.
- Storage: saves your settings and the last downloaded term update.
- Alarms: schedules the twice-daily term-update check. No install warning.
- Optional, only if you switch on image scanning: a hidden background page that runs the on-device text reader ("offscreen"). Nothing is uploaded.

## Graphics
- store/assets/store-icon-128.png (128x128)
- store/assets/promo-small-440x280.png
- store/screenshots/*.png (1280x800, at least 3)
