# Bowser HTML chrome (ui/)

TypeScript, HTML and CSS for the toolbar row of the HTML chrome. The bridge contract is in `docs/html-chrome-protocol.md`.

## Build

```sh
cd ui
npm install
npm run build       # esbuild bundle + copy into dist/
npm run typecheck   # tsc --noEmit (strict)
npm run lint        # forbids innerHTML, eval, inline handlers, etc. in src/
```

`dist/` (`app.js`, `index.html`, `app.css`) is committed. Cargo builds embed these files with `include_bytes!`, so Rust builds do not need Node. Rebuild and commit `dist/` after changing anything in `src/`.
