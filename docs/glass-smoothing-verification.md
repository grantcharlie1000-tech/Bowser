# Glass smoothing verification — 2026-10-10

Baseline: clean `main` at `d3fcb16e553036036136113e831cfeb5a184aecb`.
Changes remain uncommitted. Luna 6 implemented the initial rim changes; Haiku 5.5
audited DPI/presentation and added the overlay resize test. Lead review completed
the optical falloff, focused border, DPI stroke scaling and verification.

## Root cause and implementation

Rounded fills and strokes already had signed-distance anti-aliasing. The brighter
glass rim instead rejected every sample outside the exact silhouette, producing
an aliased highlight over an otherwise smooth fill. Its binary lit/unlit flag
also switched the fringe abruptly between cyan and warm colours. The Fresnel
square-root curve had a brightness cusp at the silhouette. Omnibox borders used
physical rather than logical stroke widths, and saturated cyan made their frames
visually dominant. Integer layout rounding is intentional and was not the primary
defect; gradients were not being stretched by presentation.

`src/chrome/paint.rs` now applies fractional signed-distance coverage to the rim,
including pixel centres just outside the rounded silhouette. A bounded cubic
grazing falloff replaces the cusp, with lower outline intensity and preserved
directional specular reflection. Dispersion continuously interpolates its colour
from the surface normal; its width follows the scaled lens band. Colour mixing
is skipped when quantised fringe coverage is zero. Active-tab and omnibox stroke
widths scale with DPI. Hover/selection emphasis, clipping and premultiplied
source-over compositing remain intact.

`src/theme.rs` reduces unfocused glass-border alpha from 112 to 72 and changes the
focused borders to quiet cool-grey colours. Focus contrast still passes the
existing backdrop-sweep test. Solid fallback colours are unchanged.

The DWM acrylic host and native-size layered BGRA overlay remain intact.
`UpdateLayeredWindow` presents the DIB at 1:1 dimensions. Analytical coverage
requires no new masks, repaint allocations, rendering loops or supersampling.
Existing DIB/font/text storage and straight-rim row reuse remain in use. Dirty
rectangles, 150 ms transitions and reduced-motion handling are unchanged.

## Visual evidence

Artifacts are under `.context/glass-smooth/` (ignored by Git):

- `before-neutral/` and `after/`: fixed themed-background CPU renders, light/dark,
  widths 768/1024/1440 physical pixels, DPI 96/120/144/192, 1/5/12 tabs, and
  empty/address/focused-progress states. Five-tab frames contain hovered, inactive
  and selected tabs. Neutral backgrounds make edge inspection clearer than the
  original diagnostic stripes; these are not simulations of DWM blur.
- `corner-comparison-4x.png`: native-pixel tab/omnibox samples enlarged with nearest
  neighbour interpolation, across all requested widths, themes and DPI values.
- `before-live-{width}.png` and `after-live-{width}.png`: running Windows 10 Bowser
  captures at the actual monitor's 96 DPI, system dark appearance.
- `motion/`: hover/selection samples at 0/40/75/150 ms.

Native-size screenshots and magnified corners show continuous edge coverage and
softer optical transitions. The higher-DPI matrix checks rasterisation and
surface sizes; Windows display settings were not changed. Live mixed-DPI monitor
transitions, other system themes, Windows 11 corners and compositor timing remain
unverified. Existing move/resize overlay lag was only a code-reading hypothesis,
not a demonstrated defect, so no unrelated synchronisation fix was made.

## Performance and memory

Preserved baseline/candidate executables are in `.context/glass-smooth/bin/`.
The baseline renderer was rebuilt in an ignored source snapshot with only the
same test instrumentation and visual-matrix changes; product rendering remained
the baseline. The release paint probe warms up 20 times, measures 201 individual
full repaints, and reports their median. Five alternating runs per variant,
without concurrent builds, produced median-of-run-medians:

| CPU paint workload | Before | After | Change |
| --- | ---: | ---: | ---: |
| 1000 px chrome, 5 tabs, light glass, 96 DPI | 1.9669 ms | 1.9870 ms | +1.02% |

This meets the 5% target for the measured workload. It is CPU paint time, excluding
`UpdateLayeredWindow` and DWM composition. Raw results are
`isolated-paired-perf.txt` and `performance-summary.json`; earlier noisy rounds are
also retained. Performance at every width/DPI/theme combination was not measured.

Visible empty-window samples over approximately 10 seconds:

| Measure | Before | After |
| --- | ---: | ---: |
| Private memory | 2.1953 MiB | 2.1992 MiB |
| Working set | 9.8164 MiB | 9.8242 MiB |
| Process CPU time during idle sample | 0 ms | 0 ms |

These are single bounded samples, not proof of universally zero idle CPU. They
exclude WebView2 page workloads. Raw data: `before-idle.json`, `after-idle.json`.
The pre-existing benchmark-navigation startup panic was not needed for these
renderer/empty-shell measurements and remains outside this fix.

## Quality gates

- `cargo fmt --all` and `cargo fmt --all -- --check`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo test --locked --quiet`: 240 passed, 3 explicit artifact/probe tests ignored.
- `cargo test --locked --release --quiet`: same result.
- Each ignored test was run separately: visual matrix, motion frames and release
  performance probe all passed.
- Debug build passed. Release build passed with `--target-dir target/glass-smooth`;
  the ordinary release exe was locked by the user's running Bowser and left alone.
- `git diff --check`: passed.

New regression coverage checks fractional rim samples, continuous colour through
the old tint seam, bounded silhouette brightness across all four DPI values,
and overlay dimensions through repeated DPI changes.
