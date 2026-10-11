# UI overhaul DPI and pixelation audit

## Findings

- `bowser.manifest` declares both `true/pm` and `PerMonitorV2, PerMonitor`. `build.rs` embeds that manifest in MSVC binaries. There is no separate process DPI-awareness call; the embedded manifest establishes awareness before the process creates windows.
- `WM_DPICHANGED` applies the suggested top-level window rectangle. It then rebuilds the chrome layout and device-pixel surface, clears favicon decode/failure caches, reloads tab images at the new target size, refreshes the address font, and repositions the edit control.
- Chrome coordinates are physical client pixels under PerMonitorV2. `ChromeSurface` allocates a top-down 32-bit DIB at the client width and the DPI-scaled chrome height; it does not render a 96-DPI surface and enlarge it. Surface resize and layered-window upload invalidate prior presentation after size or DPI changes.
- Message fonts come from `SystemParametersInfoForDpi`, which supplies the DPI-scaled message-font height. Chrome text requests `ANTIALIASED_QUALITY`; popup text requests `CLEARTYPE_QUALITY`. Cached chrome fonts are indexed by DPI.
- Favicons are decoded once per source and requested output pixel size. WIC converts to premultiplied BGRA, then scales with `WICBitmapInterpolationModeFant`; the requested size is derived from the current DPI. This is device-size keyed rather than storing redundant entries for two DPI values that round to the same pixel size.
- Rounded chrome and popup strokes use GDI+ antialiasing in `src/win32/smooth.rs`; stroke paths are inset by half their width. The device-pixel DIB and draw coordinates are shared, so no later bitmap enlargement is involved.

## Outcome and verification limits

The source audit did not find a confirmed scaling or nearest-neighbour root cause to patch. The manifest, build embedding, DPI-change path, DIB sizing, font creation, favicon scaler, and antialiased stroke paths are already present. I did not add sharpening or supersampling. Automated layout/surface tests exercise 96, 120, 144, and 192 DPI data, but this run did not change Windows display scaling or visually inspect the browser at 125%, 150%, or 200%. Live appearance and monitor-transition behavior remain unverified.
