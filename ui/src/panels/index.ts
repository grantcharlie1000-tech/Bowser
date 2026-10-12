import type { Inbound, PanelToken } from '../protocol';
import { startHistoryPanel } from './history';

export type PanelRenderer = (root: HTMLElement) => (message: Inbound) => void;
export const panels: Partial<Record<PanelToken, PanelRenderer>> = {
  history: startHistoryPanel,
};
