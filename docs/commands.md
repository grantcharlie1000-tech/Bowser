# Commands, shortcuts, Quick Commands and chains

One registry (`src/commands/`) is the single list of everything Bowser can be told to do. Keyboard shortcuts,
the Quick Commands palette, the Ctrl+F1 cheat sheet, command chains and (later) mouse gestures and toolbar
buttons all read it. Nothing else keeps its own list of commands.

## Adding a command

Append one row to `BUILTINS` in `src/commands/registry.rs`:

```rust
def("capture-visible", "Capture visible area", "Save what is on screen", C::Tools,
    &["Ctrl+Shift+S"], Page, Run(crate::win32::run_capture_visible)),
```

| Field | Meaning |
| --- | --- |
| `id` | Lower-case words joined by `-`, unique, never renamed (a saved shortcut, chain or gesture names it). |
| `label`, `detail` | What the palette, the keyboard editor and the cheat sheet show. |
| `category` | Groups the keyboard editor and the cheat sheet (`Category`). |
| `defaults` | Default chords in the text form `Chord::parse` reads (`Ctrl+Shift+K`, `Alt+Left`, `F5`, `Ctrl+,`). The registry test rejects a chord two commands share. |
| `palette` | `Command` (listed under Commands), `Action` (listed under Page actions: it acts on the page in front) or `Hidden` (assignable and chainable, not searchable). |
| `gate` | `CopyLink`, `ToggleSidebar` or `StackedSplit`: the chord is bound only while that condition holds, so the page keeps the key otherwise. |
| `handler` | `Run(fn(&Context))` for a new command: a function taking the window and the app. `Key(Command)` and `Action("...")` route the commands that existed before the registry to `win32::window`. |
| browser priority | Wrap the row in `browser(...)` when the command should keep working on a site that prioritises its own shortcuts (tab and window commands). |

That is all: the command appears in the palette, the keyboard editor, the cheat sheet and the chain
editor, can be bound, cleared and restored, and can be put in a chain. No other file needs to change.

`run_registered(hwnd, app, id)` in `src/win32/window.rs` runs any command by id. Mouse gestures (T12) and
toolbar buttons (T13) bind to an id and call it. Chains are commands too (`chain:<n>`), so a gesture or a
button can run a chain the same way. `commands::live().store.entries()` lists every command, built-in and
chain, with its current shortcuts.

### Rebinding a chord (for example Ctrl+Shift+C)

Copy address is the registry command `copy-link`, gated by the `ctrl_shift_c_copy_link` setting. To give the
chord to another command, change `defaults` on both rows (or `store.assign("other-id", "Ctrl+Shift+C", true)`,
which moves it); `ctrl_shift_c_is_a_registry_binding_that_moves_with_one_assignment` in `src/commands/mod.rs`
shows it.

## How a key press finds its command

`commands::resolve` is the one lookup, used by all three key paths (page WebView in
`src/engine/webview2.rs`, HTML chrome in `src/chrome_host/host.rs`, the main window in
`src/win32/window.rs::dispatch`). In order:

1. The chord is bound in the live keymap (registry defaults plus the user's assignments).
2. Shortcuts are on (`shortcuts_enabled`). Off leaves only Quick Commands (Ctrl+Shift+K), so nobody is locked out.
3. The chord's gate holds.
4. A single typing key (a letter, digit or punctuation key with no Ctrl or Alt) needs `single_key_shortcuts`,
   and is never taken from a page or while a text field in the chrome has the keyboard.
5. On a site in the prioritised list, and from a page, only commands marked browser priority are claimed.

`src/shortcuts.rs` keeps the default chord table (`command_for`, `command_for_gated`) as a reference: a test
checks that the registry's default keymap is that table key for key, plus Ctrl+F1.

## Files

Per profile, plain text, beside the other profile files:

- `keybindings.txt`: `opt` lines (the options below), `bind` lines (a command's whole shortcut list when it
  differs from the default, possibly empty), `priority` lines, `site` lines.
- `chains.txt`: chains, each with ordered steps (`delay_ms`, command id).

Nothing is sent anywhere and nothing is kept in the cloud. A guest profile keeps them in memory only.

## Quick Commands

Ctrl+Shift+K. Built in `src/win32/window.rs::run_command_palette` from stores that already exist: open tabs,
recently closed tabs (`App::recently_closed`), workspaces, bookmarks, notes (`notes.txt`), history, commands
and page actions (the registry) and the calculator (`src/commands/calc.rs`, `ui/src/calc.ts`).

- Filter prefixes: `bookmark:` `bkm:`, `note:`, `tab:` `tb:` (open and closed tabs), `history:`, `command:`
  `cmd:`, `action:`, `workspace:`.
- Shift (tapped alone) or Right arrow at the end completes the top result.
- A sum (`12*(3+4)`, `sqrt(2)`) adds a calculator row; Enter copies the result.
- The search runs in the page (`ui/src/palette-logic.ts`) and, for the native fallback, in Rust
  (`src/commands/palette.rs`). `ui/test/fixtures/palette-cases.txt` is run through both.

Settings (Settings > Quick commands): which result types show and their order (reset restores the default,
where the best match leads; any other order groups results by it), rows shown for a typed search (3 to 30),
keep the last typed search, open bookmarks and history in a new tab, open notes in the Notes panel (off copies
the note's text).

## Keyboard

Settings > Keyboard: shortcuts on or off, single-key shortcuts, Alt opens the menu, F6 cycles focus between the
address bar and the page, sites that prioritise their own shortcuts (also the command "Prioritise website
shortcuts on this site"), and every command with its shortcuts: several per command, add by pressing the
keys, remove one, clear, restore one, restore all, browser priority per command, and the cheat sheet
(Ctrl+F1). A chord another command holds is reported and can be taken. Editing chords (Ctrl+C, V, X, A, Z,
Y), Esc, Alt+F4, Shift+F10 and Ctrl+Alt with a typing key (AltGr) cannot be assigned.

### Left out, and why

- **Access keys**: Bowser's menus are HTML popups with no `&` mnemonics, so there is nothing for Alt+letter to
  reach. A switch would do nothing.
- **Space fast-forward**: there is no fast-forward feature, and Space scrolling belongs to the page.
- **Open bookmark automatically on a nickname match**: bookmarks have no nickname field yet (T2 adds
  nicknames). The palette can take the matching rule when the field exists.
- **Include English search terms**: Bowser's interface is English only; there is no other language to add
  English terms to.

## Command chains

Settings > Quick commands > Command chains: create, clone, edit, delete, Test chain. A chain is an ordered
list of built-in commands, each with a delay in milliseconds before it runs (0 to 60 000), at most 32 steps
and 64 chains. A chain cannot hold another chain, so none can loop. It runs from Quick Commands, from a
shortcut assigned in Keyboard, from a gesture or from a toolbar button.

A run waits on a helper thread (`commands::chains::schedule`) that only posts `WM_CHAIN_STEP`; every command
runs on the window thread, and nothing waits on the UI thread. A command id that no longer exists is skipped.

## Tests

- `cargo test --lib commands` (registry, chords, store, chains, calculator, resolve, palette search),
  `chrome_host::commands_page` (wire and snapshot fixture), `win32::command_palette` (page items).
- `cd ui && npm test`: `test/palette.mjs`, `test/commands.mjs`, `test/settings-page.mjs`.
- Screenshots: `node benchmarks/results/t4/render.mjs <outDir> before|after` (headless Edge).
