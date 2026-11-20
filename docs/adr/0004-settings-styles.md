# Settings stylesheet structure and accessibility

Status: Accepted

The Settings surface owns `ui/src/styles/settings.css`. `settings-page.ts` renders it over
`bowser://settings`, and `settings-page-logic.ts` defines its row labels and search index. The
Settings tab, start page, and toolbar load the same stylesheet bundle. Settings also hosts the
extension store and first-run Welcome view. Shared styles and host state affect how Settings renders,
which controls remain available, and how navigation follows the page scroll.

## Cascade and surfaces

- Scope selectors also used by the start page to the Settings root with `:where(#app.sp-root)`.
  This isolates the views while preserving the selector weights that determine which Settings
  variants override the base rules. Restore the standard icon stroke on that root because the
  shared toolbar stylesheet sets a narrower stroke for the default view.
- Use the shared palette and typography tokens, then derive local Settings surface tokens.
  Keep one radius across cards, fields, menus, and other nested controls so their corners match.
  Switches retain the capsule shape that distinguishes their on/off control. Use the accent to mark
  state, and put vertical spacing on the following element so hiding it leaves no gap.
- Let a normal settings group provide the surface around its title and rows. Dedicated editors,
  including Passwords and Extensions, place their own header and control their content surface.
- Explicitly apply `display: none` to hidden Settings descendants and to the Settings grid itself.
  Author display rules can otherwise override the browser's hidden rule. Heading, controls and
  layout must leave the page together when the store or Welcome hides the grid.

## Navigation and visibility

- Position the Settings scroll container so section `offsetTop` values share its coordinate system.
  The navigation code uses these offsets for jumps and for identifying the visible section.
  Position the sidebar indicator out of flow and move it with a transform so changing the current
  section does not reflow the list.
- Hide dependent controls when they cannot act. Hide a group or More disclosure if that would leave
  it empty. Search keeps those rows available to find, with their controls still disabled.
- Reserve a place for the search engine label in both Settings views. The populated select takes
  over the label when the separate address snapshot arrives. The initial Settings snapshot can
  leave this label blank, as tracked in [issue #197](https://github.com/grantcharlie1000-tech/Bowser/issues/197).
- Keep the Simple view heading as the accessible name of its section. The window title already
  identifies Settings visually. Similarly, keep row descriptions available to assistive technology
  and in the search index while visually hiding boilerplate next to a control. Keep paths, status,
  other data and descriptions on rows with no control visible.
- Show the default-browser prompt only when Bowser is known to be non-default, outside search and
  dedicated editor pages. While the prompt is visible, suppress the ordinary Default browser row
  from browsing. Search still indexes the ordinary row.

## Controls and motion

- In Extensions, use the product's puzzle glyph as a generic extension marker. Keep declarations and
  removal confirmation in the row, and use the primary text colour for broad site access so it
  stands out from narrower scopes. Reserve the install control's height in the header so the heading
  stays put when the empty state hides that control.
- In Keyboard shortcuts, reveal the individual remove buttons when pointer or keyboard activity
  reaches the row. At rest they take no space in the shortcut field. The add and priority actions
  use opacity while idle so they keep their place in the layout and remain keyboard reachable.
  Restore stays visible whenever offered. In Passwords, dim idle row actions while keeping them
  visible and operable on touch and with a keyboard.
- Keep layout motion finite, using transform and opacity for movement. The gear rotates 60 degrees
  on entry because its sixfold symmetry makes that one tooth. Settings suppresses its keyframe motion
  and pressed transforms for reduced motion, and the shared base rules turn transitions into short
  opacity fades. Passwords and Welcome opt into their keyframes only when reduced motion is off.
- Host Welcome above the Settings grid. The Look step alone has a live mock that follows the
  selected theme and accent. Its entrance stays still while the rest of the step can animate in
  direction of travel, staggered by the index assigned in `welcome.ts`. Use fixed light, dark and
  split System pictures on the option tiles so the choices are recognizable under any current theme.

These decisions are implemented in [Settings styles](../../ui/src/styles/settings.css),
[the Settings view](../../ui/src/settings-page.ts), and [Welcome](../../ui/src/welcome.ts).
The [Settings design](../design/settings-page.md), [Settings structure](../design/settings-structure.md),
and [Welcome design](../design/welcome.md) describe user-facing behavior.
