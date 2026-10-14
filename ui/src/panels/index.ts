import type { Inbound, PanelToken } from '../protocol';
import { startHistoryPanel } from './history';
import { startBookmarksPanel } from './bookmarks';
import { startCustomizePanel } from './customize';
import { startNotesPanel } from './notes';
import { startPrivacyPanel } from './privacy';

export type PanelRenderer = (root: HTMLElement) => (message: Inbound) => void;
export const panels: Partial<Record<PanelToken, PanelRenderer>> = {
  history: startHistoryPanel,
  bookmarks: startBookmarksPanel,
  customize: startCustomizePanel,
  notes: startNotesPanel,
  privacy: startPrivacyPanel,
};
