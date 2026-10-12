# Product

## Register

product

## Users

One person (the owner) and anyone they hand the laptop to, browsing on Windows 10/11, mostly on a
16:9 laptop panel at 100-150% scaling. They live in the browser all day: research, chat apps, docs,
video. They switch between a horizontal tab strip and a vertical sidebar, keep several workspaces,
and expect keyboard-first control (Ctrl+L, Ctrl+T, Ctrl+Tab, Ctrl+Shift+V, Escape).

## Product Purpose

Bowser is a small native Windows browser on the shared WebView2 runtime. It exists to load real web
pages as fast as possible with very little application overhead, inside a calm translucent window
that feels native to Windows. Success: a cold launch shows a usable page sooner than Edge or Chrome,
the chrome never makes a click, keystroke or navigation wait, and every surface looks like it was
made by one careful hand.

## Brand Personality

Quiet, precise, quick. The chrome is glass that recedes behind the page: translucent window
material, soft rounded tab "islands", a centred address pill, Segoe UI Variable. Delight comes from
how responsive and tidy it is, not from decoration.

## Anti-references

- Native Win32 popup menus and dialog boxes with grey 3D chrome (the "unstyled native" look).
- Electron-app heaviness: big fonts, padded cards, slow fades, spinners in the chrome.
- Opera GX / gamer chrome: neon accents, heavy gradients, glow.
- Generic dashboard SaaS: card grids, hero metrics, uppercase eyebrows in panels.
- Icon soup: mixed icon families, mismatched stroke weights, bitmap icons that blur at 150%.

## Design Principles

1. The page is the product: chrome yields space, colour and attention to web content.
2. Speed is a feature of the design: no animation or effect may delay input, focus or navigation.
3. One system: every menu, panel, dialog and prompt shares tokens, components, icons and motion.
4. Native where it matters: snap, drag, resize, DPI, keyboard and focus behave like Windows expects.
5. Glass with discipline: translucency is the window material, not a decoration on every element.

## Accessibility & Inclusion

WCAG 2.2 AA: text contrast >= 4.5:1 over the glass across the full backdrop sweep (already enforced
by `theme.rs` tests), non-text >= 3:1, visible focus rings, every control reachable and operable by
keyboard with correct roles and labels, `prefers-reduced-motion` honoured everywhere, crisp at
100-200% DPI, light and dark appearances.
