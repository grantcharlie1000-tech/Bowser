import { startToolbar } from '../toolbar';
import { installTooltips } from '../tooltip';
export function start(root: HTMLElement): void {
  startToolbar(root);
  installTooltips(root);
}
