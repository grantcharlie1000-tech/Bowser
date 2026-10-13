import { sendPanel, type Inbound, type PanelFields, type SnapshotMsg } from '../protocol';
import { el } from '../dom';
import { CHOICES, CHOICE_LABEL, KINDS, KIND_LABEL, deniedText, filterSites, hasStored, parsePrivacySnapshot, privacyCommand, setPermissionCommand, storedCount, type Choice, type Kind, type PrivacySnapshot, type SiteRow } from './privacy-logic';
import { emptyState, enter, pageSection, panelHeader, searchField, segmented, settingRow, textButton } from './kit';

type Fields = { id?: number; n?: number; text?: string };
function emit(a: string, fields: Fields = {}): void {
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
const RANGES = [['0', 'Last hour'], ['1', 'Last day'], ['2', '7 days'], ['3', 'All time']] as const;
const CLEAR_KINDS = ['Site data', 'Cache', 'History'] as const;
const CHOICE_OPTIONS = CHOICES.map((choice) => [choice, CHOICE_LABEL[choice]] as const);
/** Sites listed before "Show more": a full table is thousands of controls. */
const SITE_PAGE = 20;
const onOff = (value: boolean | null, on = 'On', off = 'Off'): string => (value === null ? 'Unavailable' : value ? on : off);
const plain = (value: number): string => value.toLocaleString('en-GB');

export function startPrivacyPanel(root: HTMLElement): (message: Inbound) => void {
  let snapshot: PrivacySnapshot | null = null;
  let issue = '';
  const ranges: string[] = ['0', '0', '0'];
  let confirming: number | null = null;
  let query = '';
  let shown = SITE_PAGE;
  let confirmingAll = false;
  /** Focus key to restore after the next rebuild when the focused control is going away. */
  let wantFocus: string | null = null;

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

  // The search box lives outside the per-snapshot rebuild so typing never loses it.
  const search = searchField('Search sites');
  search.input.dataset.focus = 'search';
  const siteList = el('div', 'pg-sites');

  const paintSites = (): void => {
    if (!snapshot) { siteList.replaceChildren(); return; }
    const matches = filterSites(snapshot.sites, query);
    const nodes: HTMLElement[] = matches.slice(0, shown).map((site) => siteCard(site));
    if (!snapshot.sites.length) nodes.push(emptyState('shield', 'No sites yet. The current site and any site that asks for a permission appear here.'));
    else if (!matches.length) nodes.push(emptyState('search', 'No sites match your search.'));
    if (matches.length > shown) {
      const more = textButton(`Show ${Math.min(SITE_PAGE, matches.length - shown)} more of ${matches.length - shown}`, () => { shown += SITE_PAGE; paintSites(); }, { subtle: true });
      more.dataset.focus = 'more';
      const wrap = el('div', 'pg-more');
      wrap.append(more);
      nodes.push(wrap);
    }
    siteList.replaceChildren(...nodes);
  };

  /** One site: its origin, Reset site, and a row per kind with Allow / Block / Default. */
  const siteCard = (site: SiteRow): HTMLElement => {
    const card = el('section', 'pg-site');
    card.setAttribute('aria-label', `Permissions for ${site.origin}`);
    const head = el('div', 'pg-site-head');
    const name = el('span', 'pg-site-name');
    name.textContent = site.origin;
    name.dir = 'auto';
    head.append(name);
    if (site.current) {
      const tag = el('span', 'pg-site-tag');
      tag.textContent = 'Current site';
      head.append(tag);
    }
    const reset = textButton('Reset site', () => emit('perm-reset', { text: site.origin }), { subtle: true, aria: `Reset permissions for ${site.origin}` });
    reset.disabled = !hasStored(site);
    reset.dataset.focus = `reset:${site.origin}`;
    head.append(reset);
    card.append(head);
    KINDS.forEach((kind: Kind, index) => {
      const current: Choice = site.kinds[index] ?? 'default';
      const group = segmented(CHOICE_OPTIONS, current, (choice) => {
        const command = setPermissionCommand(site.origin, kind, choice);
        if (command) sendPanel('priv', command as PanelFields);
      }, `${KIND_LABEL[kind]} for ${site.origin}`);
      group.querySelectorAll('button').forEach((button, choice) => { button.dataset.focus = `set:${site.origin}:${kind}:${CHOICES[choice]}`; });
      const row = settingRow(KIND_LABEL[kind], null, group);
      row.classList.add('pg-kind');
      card.append(row);
    });
    return card;
  };

  search.input.addEventListener('input', () => { query = search.input.value; shown = SITE_PAGE; paintSites(); });

  /** "Reset all" arms an inline confirmation; nothing is sent until it is confirmed. */
  const resetAllRow = (data: PrivacySnapshot): HTMLElement => {
    const total = storedCount(data.sites);
    if (!confirmingAll || total === 0) {
      confirmingAll = false;
      const button = textButton('Reset all', () => { confirmingAll = true; wantFocus = 'reset-all-cancel'; render(); }, { icon: 'reset', aria: 'Reset all site permissions' });
      button.disabled = total === 0;
      button.dataset.focus = 'reset-all';
      return settingRow('Saved choices', total ? `${plain(total)} saved across ${plain(data.sites.filter(hasStored).length)} sites. Sites never get a prompt; anything not allowed is blocked.` : 'Nothing saved. Sites never get a prompt; anything not allowed is blocked.', button);
    }
    const box = el('div', 'pg-confirm');
    box.setAttribute('role', 'alertdialog');
    box.setAttribute('aria-label', 'Confirm resetting all site permissions');
    const message = el('p', 'pg-confirm-text');
    message.textContent = `Reset ${plain(total)} saved ${total === 1 ? 'choice' : 'choices'} for every site? Every site goes back to blocked.`;
    const actions = el('div', 'pg-confirm-actions');
    const cancel = textButton('Cancel', () => { confirmingAll = false; wantFocus = 'reset-all'; render(); });
    cancel.dataset.focus = 'reset-all-cancel';
    const confirm = textButton('Reset all', () => { confirmingAll = false; wantFocus = 'search'; emit('perm-reset-all'); }, { primary: true, aria: 'Confirm: reset all site permissions' });
    confirm.dataset.focus = 'reset-all-confirm';
    actions.append(cancel, confirm);
    box.append(message, actions);
    box.addEventListener('keydown', (event) => { if (event.key === 'Escape') { event.stopPropagation(); confirmingAll = false; wantFocus = 'reset-all'; render(); } });
    return box;
  };

  const render = (): void => {
    const keep = scroll.scrollTop;
    const focused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const focusKey = focused?.dataset.focus;
    const caret = focused === search.input ? [search.input.selectionStart, search.input.selectionEnd] as const : null;
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
      settingRow('Tracking prevention', null, snapshot.tracking ?? 'Unavailable'),
    );
    sections.push(site.root);

    // Permissions come first after the page facts: they are the part the user changes.
    const permissions = pageSection('Site permissions');
    permissions.body.append(resetAllRow(snapshot));
    const bar = el('div', 'pg-bar');
    bar.append(search.root);
    permissions.body.append(bar, siteList);
    paintSites();
    sections.push(permissions.root);

    const denied = pageSection('Recently blocked requests');
    denied.body.append(settingRow('Last 50 this session', 'Site, kind and time only. Kept in memory and gone when Bowser closes.', textButton('Clear list', () => emit('denied-clear'), { subtle: true, aria: 'Clear the list of recently blocked requests' })));
    for (const row of snapshot.denied) {
      denied.body.append(settingRow(row.origin, deniedText(row)));
    }
    if (!snapshot.denied.length) denied.body.append(settingRow('Nothing blocked yet', 'Requests from sites for camera, microphone, location, notifications or popups appear here.'));
    sections.push(denied.root);

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
    );
    sections.push(protection.root);

    const clear = pageSection('Clear browsing data');
    CLEAR_KINDS.forEach((kind, kindIndex) => {
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
    // A snapshot rebuilds the page; put keyboard focus back on the same control.
    const key = wantFocus ?? focusKey;
    wantFocus = null;
    if (key) {
      const again = column.querySelector<HTMLElement>(`[data-focus="${CSS.escape(key)}"]`);
      if (again && !(again as HTMLButtonElement).disabled) {
        again.focus({ preventScroll: true });
        if (caret && again instanceof HTMLInputElement) again.setSelectionRange(caret[0], caret[1]);
      }
    }
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
