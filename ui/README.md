# Bowser HTML chrome (ui/)

TypeScript, HTML and CSS for the HTML chrome: toolbar, tab strip, vertical sidebar, rail, panels, dialogs and menu popups. The bridge contract is in `docs/html-chrome-protocol.md`; tokens, components, icons, motion rules and file ownership are in `STYLEGUIDE.md`.

## Build

```sh
cd ui
npm install
npm run build       # icons + esbuild bundle + Tailwind CSS v4 -> dist/, then dist.manifest
npm run typecheck   # tsc --noEmit (strict)
npm run lint        # forbids innerHTML, eval, inline handlers, etc. in src/
npm test            # node --test
```

`dist/` (`app.js`, `index.html`, `app.css`) is committed. Cargo builds embed these files with `include_bytes!`, so Rust builds do not need Node. Rebuild and commit `dist/` after changing anything in `src/` or `icons/`.

Styles live in `src/styles/` (one file per surface, compiled into the single static `dist/app.css`); icons come from `icons/*/` through `scripts/gen-icons.mjs` into `src/icons.gen.ts`.
