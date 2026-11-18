# Welcome (first-run onboarding)

`bowser://settings/welcome`. Drawn by the settings page host over its sections (`ui/src/welcome.ts`,
words and choice mapping in `ui/src/welcome-logic.ts`, styles under "Welcome" in
`ui/src/styles/settings.css`). Every choice goes through the settings page's existing commands, so Rust
checks it exactly as it checks Settings.

## When it shows

On a profile's first run with the HTML chrome, after the first chrome paint (`WM_IMPORT_OFFER_TIMER`
in `win32/window.rs`) the welcome takes the blank first tab and the old "Import from another browser?"
menu is marked answered. It only takes a blank first tab: a first launch that opens a page (a link from
another app, `--open-url`) keeps the page in front and the welcome waits for the next launch. Guest windows
never get it. Typing `bowser://settings/welcome` opens it again.

`Section::Welcome` is a route, not a section: it is never in the page's navigation, and the page's
`SETTINGS_SECTIONS` does not list it. The tab is titled "Welcome to Bowser" and goes back to
"Settings" when the page leaves it (`App::set_internal_section`).

## Steps

| Step | Sends |
|---|---|
| Welcome | nothing (Get started, Skip setup) |
| Import | `importfrom` + `chrome`/`edge`/`brave`/`firefox`/`opera`/`vivaldi`/`helium` (one click: that browser's profile, previewed before import; Opera falls back to Opera GX, Helium is read from `imput\Helium` or `Helium` under Local AppData); the File tile sends `import` |
| Look | `themeopt` schedule/look; `save` with `theme_accent` (the only step with the preview window) |
| Search | `engine` `default\t<id>` for Google, Bing, DuckDuckGo or Brave (only built-ins still in the list) |
| Ad blocking | Yes / No: the protection panel's `blocklevel` `ads` or `none` |
| Default browser | `setdefault` |
| Done | `welcomedone` (a new tab replaces the welcome tab), or `section ""` for Open Settings |

Skip setup sends `welcomedone` from any step.

## Look and motion

Inspired by Zen (a look with a live picture), Helium (say little) and Dia (bring your browsing first,
end on a few keys). Hello and Done are a centred hero with the Bowser logo (`ui/src/assets/bowser-logo.png`,
icons/bowser.png at 192px, served as `bowser-logo.png`). The other steps are one centred column; Look adds
a to-scale mock of Bowser's own slim chrome that follows the look and accent at once and never animates.
Import and Search are big logo tiles with a glow in each brand's colour (`ui/src/brand-marks.ts`: Gil Barbara's
logos set, CC0, and Helium's own logo). Import has a transfer picture over the tiles (the picked browser, dots
flowing across, Bowser; a fan of logos before a pick); Search has a large preview of the search bar that
shows the picked engine.
Copy is kept to titles and button words.

Micro-animations (transform and opacity only, all off under reduced motion): the step slides in from the
side it was reached from while its parts rise in one after another (`--wl-i`), the progress segment fills,
tiles lift on hover and give on press, a pick pops its tick, swatches pop their ring, the primary button
lifts and its arrow leans forward.
