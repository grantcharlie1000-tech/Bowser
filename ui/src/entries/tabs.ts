import { startTabs } from '../tabs';
import { installHoverTargets, installTooltips, tabStripHoverTarget } from '../tooltip';
export function start(root: HTMLElement): void {
  const stackRow = startTabs(root);
  for (const host of [root, stackRow]) {
    installTooltips(host);
    installHoverTargets(host, tabStripHoverTarget);
  }
}
