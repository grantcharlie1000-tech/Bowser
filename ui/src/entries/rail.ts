import { startRail } from '../rail';
import { installTooltips } from '../tooltip';
export function start(root: HTMLElement): void {
  startRail(root);
  installTooltips(root);
}
