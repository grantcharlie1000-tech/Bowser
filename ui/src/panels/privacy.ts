import { sendPanel, type Inbound, type PanelFields, type SnapshotMsg } from '../protocol';
import { el } from '../dom';
import { parsePrivacySnapshot, privacyCommand, type PrivacySnapshot } from './privacy-logic';
import { enter, iconButton, pageSection, panelHeader, segmented, settingRow, textButton } from './kit';

function emit(a: string, fields: { id?: number; n?: number } = {}): void {
  const command = privacyCommand(a, fields);
  if (command) sendPanel('priv', command as PanelFields);
}

const SECURITY: Record<PrivacySnapshot['security'], string> = {
  secure: 'Secure connection',
  notsecure: 'Not secure',
  certproblem: 'Certificate problem',
  internal: 'Browser page',
  unknown: 'Unknown',
  none: 'No active site',
};
const KINDS = ['Site data', 'Cache', 'History'] as const;
const RANGES = [['0', 'Last hour'], ['1', 'Last day'], ['2', '7 days'], ['3', 'All time']] as const;
const onOff = (value: boolean | null, on = 'On', off = 'Off'): string => (value === null ? 'Unavailable' : value ? on : off);
const plain = (value: number): string => value.toLocaleString('en-GB');

export function startPrivacyPanel(root: HTMLElement): (message: Inbound) => void {
  let snapshot: PrivacySnapshot | null = null;
  let issue = '';
  const ranges: string[] = ['0', '0', '0'];
  let confirming: number | null = null;

  const header = panelHeader('Privacy dashboard', 'Close privacy dashboard');
  const status = el('p', 'pn-status');
  status.setAttribute('role', 'status');
  const column = el('div', 'pg-col');
  const scroll = el('div', 'pg-scroll bw-scroll');
  scroll.append(column);
  const host = el('main', 'pn pg-page');
  host.append(header.root, status, scroll);
  enter(host);
  root.replaceChildren(host);

  const render = (): void => {
    const keep = scroll.scrollTop;
    status.textContent = snapshot?.status ?? issue;
    status.hidden = status.textContent === '';
    if (!snapshot) {
      if (!issue) { status.hidden = false; status.textContent = 'Loading privacy information…'; }
      column.replaceChildren();
      return;
    }
    const { stats } = snapshot;
    const sections: HTMLElement[] = [];

    const site = pageSection('This site');
    site.body.append(
      settingRow('Profile', null, `${snapshot.profile}${snapshot.guest ? ' (Guest)' : ''}`),
      settingRow('Site', null, snapshot.site ?? 'No active site'),
      settingRow('Connection', null, SECURITY[snapshot.security]),
      settingRow('Blocking on this site', null, onOff(snapshot.blocking, 'Active', 'Inactive')),
      settingRow('Tracking prevention', snapshot.tracking !== null && snapshot.tracking !== snapshot.trackingSetting ? `Set to ${snapshot.trackingSetting} in Settings. Applies after a restart.` : null, snapshot.tracking ?? 'Unavailable'),
    );
    sections.push(site.root);

    const blocked = pageSection('Blocked requests');
    blocked.body.append(
      settingRow('All time', null, plain(stats.allTime)),
      settingRow('This session', null, plain(stats.session)),
      settingRow('This site', null, plain(stats.currentSite)),
      settingRow('Statistics', 'Resets the counts above and the lists below.', textButton('Clear statistics', () => emit('clear-stats'))),
    );
    sections.push(blocked.root);

    const top = pageSection('Top blocked sites');
    for (const item of stats.topSites) top.body.append(settingRow(item.host, null, plain(item.count)));
    if (!stats.topSites.length) top.body.append(settingRow('Nothing blocked yet', null));
    sections.push(top.root);

    const days = pageSection('Last seven days');
    for (const item of stats.days) days.body.append(settingRow(item.day, null, plain(item.count)));
    if (!stats.days.length) days.body.append(settingRow('No activity recorded', null));
    sections.push(days.root);

    const protection = pageSection('Protection');
    protection.body.append(
      settingRow('Tracker blocking', null, onOff(snapshot.blockerSetting)),
      settingRow('SmartScreen', null, onOff(snapshot.smartScreen)),
      settingRow('Tab sleeping', null, snapshot.sleeping ? `On, after ${snapshot.sleepAfter} seconds` : 'Off'),
      settingRow('Efficiency mode', null, snapshot.efficiencyActive ? 'Active' : 'Not active'),
    );
    sections.push(protection.root);

    const permissions = pageSection('Site permissions');
    snapshot.permissions.forEach((item, index) => {
      const choice = segmented([['allow', 'Allow'], ['block', 'Block']] as const, item.decision as 'allow' | 'block', (value) => emit('permission', { id: index, n: value === 'allow' ? 0 : 1 }), `${item.permission} for ${item.origin}`);
      const control = el('div', 'pg-pair');
      control.append(choice, iconButton('reset', `Clear saved choice for ${item.permission} on ${item.origin}`, () => emit('permission', { id: index, n: 2 })));
      permissions.body.append(settingRow(item.permission, item.origin, control));
    });
    if (!snapshot.permissions.length) permissions.body.append(settingRow('No saved choices', 'Sites you allow or block appear here.'));
    sections.push(permissions.root);

    const clear = pageSection('Clear browsing data');
    KINDS.forEach((kind, kindIndex) => {
      const run = textButton('Clear', () => {
        if (confirming !== kindIndex) { confirming = kindIndex; paintRun(); return; }
        confirming = null;
        paintRun();
        emit('clear-data', { id: kindIndex, n: Number(ranges[kindIndex] ?? '0') });
      }, { aria: `Clear ${kind}` });
      const paintRun = (): void => {
        const on = confirming === kindIndex;
        run.replaceChildren(document.createTextNode(on ? 'Confirm' : 'Clear'));
        run.classList.toggle('is-primary', on);
        run.setAttribute('aria-label', on ? `Confirm clearing ${kind}` : `Clear ${kind}`);
      };
      run.addEventListener('blur', () => { if (confirming === kindIndex) { confirming = null; paintRun(); } });
      const range = segmented(RANGES, (ranges[kindIndex] ?? '0') as '0', (value) => { ranges[kindIndex] = value; if (confirming === kindIndex) { confirming = null; paintRun(); } }, `${kind} time range`);
      const control = el('div', 'pg-pair');
      control.append(range, run);
      clear.body.append(settingRow(kind, null, control));
    });
    sections.push(clear.root);

    column.replaceChildren(...sections);
    scroll.scrollTop = keep;
  };

  render();
  return (message: Inbound): void => {
    if (message.t !== 'snap' || message.p !== 'privacy') return;
    const incoming = parsePrivacySnapshot((message as SnapshotMsg).d);
    if (!incoming) { snapshot = null; issue = 'Privacy information could not be loaded because the response was invalid.'; render(); return; }
    issue = '';
    snapshot = incoming;
    render();
  };
}
