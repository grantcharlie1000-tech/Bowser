# Passwords and autofill

Saved sign-ins, addresses and payment cards; filling them into pages; a strong-password generator;
and moving passwords in from other apps.

## Where things live

| Piece | File |
|---|---|
| Password vault (DPAPI, `Passwords.dat`), CSV import/export | `src/passwords/mod.rs` |
| Generator and strength estimate | `src/passwords/generator.rs` |
| Which app wrote a CSV, and its columns | `src/passwords/providers.rs` |
| Address and card wallet (DPAPI, `Autofill.dat`), field kinds, fill-script builder | `src/autofill/mod.rs` |
| Field classifier shared by both page scripts | `src/autofill/fields.js` |
| Detector added to every eligible top-level page | `src/autofill/page.js` |
| Fill script body (runs only after a pick in Bowser's menu) | `src/autofill/fill.js` |
| Page messages (strict JSON shapes, origin checks) | `passwords::validate_login_message` |
| Fill menu under the field, save prompts | `src/win32/autofill_ui.rs` |
| Settings section (`bowser://settings/passwords`): wire | `src/chrome_host/passwords_page.rs` |
| Settings section: host commands | `src/win32/passwords_section.rs` |
| Settings section: page | `ui/src/passwords-section.ts`, `ui/src/passwords-logic.ts` |

## Recognised fields

The classifier reads, in order: the page's `autocomplete` tokens, `type=password`, then the
control's name, id, label, placeholder and ARIA text. It recognises username, email, current and
new password, given/family/full name, organisation, phone, address lines, town, region, postcode,
country, and card name, number, expiry (combined or month/year) and security code. Search boxes,
one-time codes, coupons and captchas are never filled.

## In the page

- A small button appears inside a recognised field (closed shadow root, family icons). Selecting it,
  or pressing Alt+Down (Down in an empty field), asks Bowser for its menu under the field.
- The menu lists the site's saved logins, "Use a strong password", saved addresses or saved cards,
  and a link to the settings section. A pick runs a fill script pinned to the origin the menu was
  built for; it fills only visible, editable, recognised fields in the focused field's form.
- A generated password is filled into every new-password field (password and confirmation) and saved
  automatically when the same site submits it.
- Submissions are caught on `submit`, on sign-in/checkout buttons, on Enter in a password form, and on
  `pagehide` after typing a password (script-driven sign-ins). Multi-step sign-ins remember the
  username from the first step. Bowser then offers to save or update the login, the address or the
  card. Card security codes are never captured or stored.

## Settings section

Search and Weak/Reused filters; show, copy (clipboard clears after 30 s), edit and delete each login;
the generator (length 8 to 64, character classes, no look-alikes); import from a CSV export with
per-app export steps and a preview before anything is written; export to a Chrome-format CSV;
addresses; cards; the save/fill switches; and the sites marked "Never".

Recognised exports: Chrome, Edge, Brave, Opera, Vivaldi, Firefox, Safari/Apple Passwords, 1Password,
Bitwarden, LastPass, Dashlane, NordPass, Proton Pass, KeePass/KeePassXC, RoboForm, Enpass, Keeper and
Zoho Vault, plus any CSV with site, username and password columns. Sites without a scheme become
`https://`.

The HTML settings page shows a password only after "Show" (cleared after 30 s or when the page is
hidden) and shows generated passwords. The previous rule that kept the store out of the HTML chrome
(`chrome_host/guard.rs`) was removed for this section by the owner's decision.
