// Page zoom values shared by the toolbar chip and the site information page. Pure (no DOM, no
// imports), tested in test/zoom.mjs. The range matches Rust (site_prefs::ZOOM_MIN, ZOOM_MAX).

/** Page zoom limits and the default, in percent. */
export const MIN_ZOOM = 25;
export const MAX_ZOOM = 500;
export const DEFAULT_ZOOM = 100;

/** True for a zoom the host may send: a whole percent inside the supported range. */
export function validZoom(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value >= MIN_ZOOM && value <= MAX_ZOOM;
}

/** The chip shows only while the page is zoomed; 100% and an absent value show nothing. */
export function isZoomed(zoom: number | undefined): zoom is number {
  return validZoom(zoom) && zoom !== DEFAULT_ZOOM;
}

/** Text on the chip and in the site information row. */
export function zoomText(zoom: number): string {
  return `${zoom}%`;
}

/** Accessible name of the chip: it resets the zoom. */
export function zoomChipLabel(zoom: number): string {
  return `Zoom ${zoom}%. Reset to ${DEFAULT_ZOOM}%`;
}

/** Tooltip of the chip. */
export function zoomChipTip(zoom: number): string {
  return `Zoom ${zoom}% (Ctrl+0 resets)`;
}
