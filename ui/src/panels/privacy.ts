import { sendPanel, type Inbound, type PanelFields, type SnapshotMsg } from '../protocol';
import { parsePrivacySnapshot, privacyCommand, type PrivacySnapshot } from './privacy-logic';

function emit(a: string, fields: { id?: number; n?: number } = {}): void {
  const command = privacyCommand(a, fields);
  if (command) sendPanel('priv', command as PanelFields);
}
function line(label: string, value: string): HTMLElement {
  const row = document.createElement('p');
  const name = document.createElement('strong'); name.textContent = `${label}: `;
  row.append(name, document.createTextNode(value)); return row;
}

export function startPrivacyPanel(root: HTMLElement): (message: Inbound) => void {
  let snapshot: PrivacySnapshot | null = null;
  let issue = '';
  const host = document.createElement('section'); host.className = 'privacy-panel';
  const status = document.createElement('p'); status.setAttribute('role', 'status');
  const content = document.createElement('div');
  host.append(status, content); root.replaceChildren(host);
  const render = (): void => {
    content.replaceChildren();
    if (!snapshot) { status.textContent = issue || 'Loading privacy information…'; return; }
    status.textContent = snapshot.status ?? issue;
    const { stats } = snapshot;
    content.append(line('Profile', `${snapshot.profile}${snapshot.guest ? ' (Guest)' : ''}`));
    content.append(line('Blocked requests', `All time ${stats.allTime} · This session ${stats.session} · Current site ${stats.currentSite}`));
    content.append(line('Current site', snapshot.site ?? 'No active site'));
    content.append(line('Security', snapshot.security));
    content.append(line('Blocking on this site', snapshot.blocking === null ? 'Unavailable' : snapshot.blocking ? 'Active' : 'Inactive'));
    content.append(line('Tracking prevention', snapshot.tracking ?? 'Unavailable'));
    content.append(line('SmartScreen', snapshot.smartScreen === null ? 'Unavailable' : snapshot.smartScreen ? 'On' : 'Off'));
    content.append(line('Tracker blocking', snapshot.blockerSetting ? 'On' : 'Off'));
    content.append(line('Tab sleeping', snapshot.sleeping ? `On (after ${snapshot.sleepAfter} seconds)` : 'Off'));
    const sites = document.createElement('section');
    const heading = document.createElement('h3'); heading.textContent = 'Top blocked sites'; sites.append(heading);
    for (const item of stats.topSites) sites.append(line(item.host, String(item.count)));
    const days = document.createElement('section');
    const daysHeading = document.createElement('h3'); daysHeading.textContent = 'Recent blocked requests'; days.append(daysHeading);
    for (const item of stats.days) days.append(line(item.day, String(item.count)));
    const permissions = document.createElement('section');
    const permissionsHeading = document.createElement('h3'); permissionsHeading.textContent = 'Site permissions'; permissions.append(permissionsHeading);
    snapshot.permissions.forEach((item, index) => {
      const row = document.createElement('article');
      row.append(line(`${item.origin} · ${item.permission}`, item.decision));
      ['Allow', 'Block', 'Clear choice'].forEach((choice, choiceIndex) => {
        const action = document.createElement('button'); action.type = 'button'; action.textContent = choice;
        action.addEventListener('click', () => emit('permission', { id: index, n: choiceIndex })); row.append(action);
      });
      permissions.append(row);
    });
    if (!snapshot.permissions.length) permissions.append(line('Saved choices', 'None'));
    content.append(sites, days, permissions);
    const clearStats = document.createElement('button'); clearStats.type = 'button'; clearStats.textContent = 'Clear privacy statistics'; clearStats.addEventListener('click', () => emit('clear-stats')); content.append(clearStats);
    const clear = document.createElement('section');
    const clearHeading = document.createElement('h3'); clearHeading.textContent = 'Clear browsing data'; clear.append(clearHeading);
    const kinds = ['Site data', 'Cache', 'History'];
    const ranges = ['Last hour', 'Last day', 'Last seven days', 'All time'];
    kinds.forEach((kind, kindIndex) => ranges.forEach((range, rangeIndex) => {
      const action = document.createElement('button'); action.type = 'button'; action.textContent = `${kind} · ${range}`;
      action.addEventListener('click', () => {
        emit('clear-data', { id: kindIndex, n: rangeIndex });
      }); clear.append(action);
    }));
    content.append(clear);
  };
  render();
  return (message: Inbound): void => {
    if (message.t !== 'snap' || message.p !== 'privacy') return;
    const incoming = parsePrivacySnapshot((message as SnapshotMsg).d);
    if (!incoming) { snapshot = null; issue = 'Privacy information could not be loaded because the response was invalid.'; render(); return; }
    issue = ''; snapshot = incoming; render();
  };
}
