import type { RowSpec } from './settings-page-logic';

export const REFRESH_CHOICES = [0, 15, 30, 60, 180, 720] as const;
export type RefreshChoice = typeof REFRESH_CHOICES[number];

export function refreshLabel(minutes: number): string {
  if (minutes === 0) return 'Never';
  if (minutes < 60) return `${minutes} min`;
  const hours = minutes / 60;
  return hours === 1 ? '1 hour' : `${hours} hours`;
}

export function isRefreshChoice(value: unknown): value is RefreshChoice {
  return typeof value === 'number' && (REFRESH_CHOICES as readonly number[]).includes(value);
}

export function feedAssignment(key: 'feeds_detect' | 'feeds_refresh_minutes', value: boolean | number): string | null {
  if (key === 'feeds_detect') return typeof value === 'boolean' ? `feeds_detect=${value}` : null;
  return isRefreshChoice(value) ? `feeds_refresh_minutes=${value}` : null;
}

export const FEED_ROWS: readonly RowSpec[] = [
  { id: 'feedsOpen', section: 'address', label: 'Feeds', description: '', keywords: 'feeds rss atom podcast reader subscribe news youtube opml Feeds Subscribe to feeds, read them and play podcast episodes. Bowser fetches only the feeds you subscribe to, when you open Feeds or press Refresh, and sends nothing to anyone else.' },
  { id: 'feedsDetect', section: 'address', label: 'Feed button', aria: 'Show a feed button when a page offers one', description: '', keywords: 'feed button address bar rss atom detect subscribe icon Show a feed button when a page offers one A button appears in the address field to preview the page’s feed and subscribe. Bowser only reads the page’s own feed links; nothing is fetched until you press the button.', reload: true },
  { id: 'feedsRefresh', section: 'address', label: 'Refresh', aria: 'Fetch feeds again when Feeds opens', description: '', keywords: 'feeds refresh interval update fetch minutes hour when opens Fetch feeds again when Feeds opens Feeds that have not been fetched for longer than this are fetched when the Feeds panel opens. Never fetches only when you press Refresh. Bowser fetches nothing in the background.' },
];
