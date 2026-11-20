# Keep Settings navigation and views in one host document

## Context

Bowser exposes Settings through `bowser://settings[/section]`. Its native host renders a fixed chrome document and validates the document and message source against the exact start URL. The Settings UI also needs to maintain stable section links while grouping controls by how people use them.

## Decision

- Keep section navigation in the chrome document. Use buttons and scroll targets, report section changes to the native host, and let the native host update the visible tab address. Follow native `settingsgo` messages as incoming navigation and hold the first destination until the initial settings snapshot arrives.
- Stage controls by native section, then move them into the areas, cards, disclosures, and editor pages specified by the Settings layout table. Keep the native section identifiers for deep links. An area link goes to its heading. A link to a section inside an area goes to the corresponding group and opens More when the group is folded.
- Use one live DOM node per control. Simple temporarily moves those nodes, or the openers for editor pages, into its own groups and leaves a marker for returning them to All. Store the Simple/All preference and the More disclosure state in local storage because they describe how the viewer uses the page. The current visit still works when storage is unavailable. Search and section links can show All without overwriting the saved view preference.
- Let the navigator retain the chosen destination across late snapshots, layout changes, and the end-of-page scroll limit. Observe groups as well as their containers because a group can move while the container keeps its size. Correct the position only when the computed destination changes, and release it when the user interacts with the scroll surface. A jump within the same view can scroll smoothly. A jump that exchanges Simple, search results, or an editor for All is instant.
- Index an embedded editor or dashboard as one Settings row. Search leads to the opener for an editor, preserves result scroll on a snapshot refresh, and keeps a disclosure open if someone focuses a result inside it. Suppress headings that duplicate the editor page heading and keep the editor indexed through its row metadata. Check highlights against the rendered text so snapshot label updates require no separate cache. Keep the slash shortcut available outside typing controls.
- Keep the default-browser prompt on the main view only when the host confirms Bowser is out of that role. The regular setting remains the search entry. Give each More button an accessible name that identifies its area. Keep the Settings controls and dialogs inert while the first-run welcome is showing.

## Consequences

Settings can change its information architecture without changing native navigation identifiers. Reusing live controls keeps event bindings and state consistent across the two Settings views. Search, navigation, and accessibility depend on the physical placement and visibility of those same controls.

See [Settings page](../design/settings-page.md), [Settings structure](../design/settings-structure.md), [Settings UI](../../ui/src/settings-page.ts), and [the native host](../../src/chrome_host/host.rs).
