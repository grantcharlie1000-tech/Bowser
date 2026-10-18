// Built-in store uses the trusted settings bridge. No frame, external assets or URL navigation.
import { el, icon } from './dom';
import { textButton } from './panels/kit';
import { mountExtensionsSection, type ExtensionsSectionOptions } from './extensions-section';
import type { SnapshotMsg } from './protocol';
import { filterCatalogue, parseStoreSnapshot, PERMISSION_GROUPS, storeBusy, storeInstallCommand, storeRequestCommand, storeStatus, type StoreEntry, type StoreSnapshot } from './store-logic';

export function mountStoreSection(options: ExtensionsSectionOptions): { readonly root: HTMLElement; receive(message: SnapshotMsg): boolean | null; hide(): void } {
  const root = el('div', 'bs bw-scroll');
  root.hidden = true;
  let snapshot: StoreSnapshot | null = null;
  let view: 'discover' | 'categories' | 'installed' = 'discover';
  let category = '';
  let selected = '';
  let pending = false;
  let invalid = false;
  const header = el('header', 'bs-header');
  const brand = el('div', 'bs-brand tw:flex tw:items-center tw:gap-3');
  const tile = el('span', 'bs-brand-icon'); tile.append(icon('puzzle', { size: 24 }));
  const title = el('h1'); title.textContent = 'Bowser Store'; title.tabIndex = -1; brand.append(tile, title);
  header.append(brand, textButton('Extension settings', () => options.send({ a: 'section', text: 'extensions' }), { icon: 'gear' }));
  const layout = el('div', 'bs-layout');
  const side = el('aside', 'bs-side');
  const nav = el('nav', 'bs-nav'); nav.setAttribute('aria-label', 'Store sections');
  const navButtons = new Map<string, HTMLButtonElement>();
  for (const [key, label, glyph] of [['discover', 'Discover', 'glint'], ['categories', 'Categories', 'grid'], ['installed', 'Installed', 'puzzle']] as const) {
    const button = textButton(label, () => { view = key; selected = ''; category = ''; render(); }, { icon: glyph });
    navButtons.set(key, button); nav.append(button);
  }
  const browse = textButton('Chrome Web Store', () => options.send({ a: 'storebrowse' }), { icon: 'external' });
  const asideNote = el('p', 'bs-muted'); asideNote.textContent = 'A curated starting point. Compatibility is reported honestly, not assumed.';
  side.append(nav, asideNote, browse);
  const main = el('main', 'bs-main'); main.setAttribute('aria-label', 'Extension catalogue');
  const search = el('input', 'bw-input bs-search'); search.type = 'search'; search.placeholder = 'Search extensions'; search.setAttribute('aria-label', 'Search extensions'); search.maxLength = 128;
  const body = el('div', 'bs-body');
  const status = el('section', 'bs-status'); status.setAttribute('aria-live', 'polite'); status.setAttribute('aria-label', 'Extension installation');
  const feedback = el('p', 'bs-feedback'); feedback.hidden = true; feedback.setAttribute('aria-live', 'polite');
  const installed = mountExtensionsSection({ ...options, idPrefix: 'bs', toast: (text, ok) => { feedback.textContent = text; feedback.hidden = false; feedback.setAttribute('role', ok ? 'status' : 'alert'); } });
  delete installed.root.dataset.row; // Store management is not a second searchable Settings row.
  installed.root.classList.add('bs-installed');
  main.append(search, feedback, status, body, installed.root);
  layout.append(side, main); root.append(header, layout);
  search.addEventListener('input', () => { selected = ''; renderBody(); });
  root.addEventListener('keydown', (event) => {
    if (event.key === 'f' && event.ctrlKey && !event.altKey && !event.shiftKey) { event.preventDefault(); view = 'discover'; selected = ''; renderBody(); search.focus(); search.select(); }
  });
  const sendInstall = (id: string): void => {
    const command = storeInstallCommand(id);
    if (!command || pending || (snapshot && storeBusy(snapshot.phase))) return;
    if (options.send(command)) { pending = true; render(); }
  };
  const paragraph = (value: string, cls = 'bs-muted'): HTMLElement => { const p = el('p', cls); p.textContent = value; return p; };
  const badge = (entry: StoreEntry): HTMLElement => { const b = el('span', 'bs-badge'); b.textContent = entry.compatibility ? entry.compatibility[0]!.toUpperCase() + entry.compatibility.slice(1) : 'Untested'; return b; };
  const installButton = (entry: StoreEntry): HTMLButtonElement => {
    const button = textButton('Get extension', () => sendInstall(entry.id), { primary: true, icon: 'download', aria: `Get ${entry.name}` });
    button.dataset.focus = `get-${entry.id}`;
    button.disabled = invalid || pending || !!(snapshot && storeBusy(snapshot.phase)); return button;
  };
  const detail = (entry: StoreEntry): HTMLElement => {
    const section = el('section', 'bs-detail'); section.setAttribute('aria-label', entry.name);
    const back = textButton('Back to catalogue', () => { selected = ''; renderBody(); root.querySelector<HTMLElement>(`[data-focus="details-${entry.id}"]`)?.focus(); }, { icon: 'back' });
    const h = el('h2'); h.textContent = entry.name; h.tabIndex = -1;
    const intro = el('div', 'bs-detail-head'); intro.append(h, badge(entry), installButton(entry));
    const facts = el('dl', 'bs-facts');
    for (const [label, value] of [['Publisher', entry.publisher], ['Category', entry.category], ['Package source', entry.source], ['Publisher website', entry.homepage], ['Provenance', entry.provenance], ['Catalogue checked', entry.checked], ['Compatibility', entry.compatibilityNote || 'Untested. No version-specific compatibility result is available.']] as const) {
      const dt = el('dt'); dt.textContent = label; const dd = el('dd'); dd.textContent = value || 'Not provided'; facts.append(dt, dd);
    }
    const sources = el('div', 'bs-source-actions');
    sources.append(textButton('View original listing', () => options.send({ a: 'storelisting', text: entry.id }), { icon: 'external' }), textButton('Publisher website', () => options.send({ a: 'storesource', text: entry.id }), { icon: 'external' }));
    section.append(back, intro, sources, paragraph(entry.description, 'bs-description'), facts, paragraph('Bowser resolves the current package and shows its version and full declared permissions before you choose whether to install. A listing is not an endorsement or a guarantee of compatibility.'));
    return section;
  };
  function renderBody(): void {
    installed.root.hidden = view !== 'installed'; search.hidden = view === 'installed'; body.hidden = view === 'installed';
    for (const [key, button] of navButtons) { if (key === view) button.setAttribute('aria-current', 'page'); else button.removeAttribute('aria-current'); }
    if (view === 'installed') return;
    if (invalid) { body.replaceChildren(paragraph('The store response could not be read. Refresh the page to try again.')); return; }
    if (!snapshot) { body.replaceChildren(paragraph('Loading the extension catalogue…')); return; }
    const chosen = snapshot.catalogue.find((entry) => entry.id === selected);
    if (chosen) { body.replaceChildren(detail(chosen)); return; }
    const lead = el('div', 'bs-lead');
    const h = el('h2'); h.textContent = view === 'categories' ? 'Find your kind of extension' : 'Make Bowser your own';
    lead.append(h, paragraph('Useful extensions, a clear source, and a permission review before installation.'));
    const categories = el('div', 'bs-categories'); categories.setAttribute('aria-label', 'Filter by category');
    if (view === 'categories') for (const [index, name] of ['', ...new Set(snapshot.catalogue.map((entry) => entry.category))].entries()) {
      const button = textButton(name || 'All extensions', () => { category = name; renderBody(); root.querySelector<HTMLElement>(`[data-category="${index}"]`)?.focus(); }); button.dataset.category = String(index); button.setAttribute('aria-pressed', String(category === name)); categories.append(button);
    }
    const items = filterCatalogue(snapshot.catalogue, search.value, category);
    const grid = el('div', 'bs-grid');
    for (const entry of items) {
      const card = el('article', 'bs-card');
      const glyph = el('span', 'bs-card-icon'); glyph.append(icon('puzzle', { size: 24 }));
      const title = el('h3'); const open = textButton(entry.name, () => { selected = entry.id; renderBody(); body.querySelector<HTMLElement>('h2')?.focus(); }, { subtle: true }); open.dataset.focus = `details-${entry.id}`; title.append(open);
      const meta = paragraph(`${entry.publisher} · ${entry.category}`);
      const foot = el('div', 'bs-card-foot'); foot.append(badge(entry), installButton(entry));
      card.append(glyph, title, meta, paragraph(entry.description, 'bs-card-description'), foot); grid.append(card);
    }
    if (items.length === 0) grid.append(paragraph('No extensions match. Try another search or category.'));
    body.replaceChildren(lead, categories, grid);
  }
  function render(): void {
    const focused = document.activeElement as HTMLElement | null;
    const focusKey = focused && root.contains(focused) ? focused.dataset.focus : undefined;
    status.replaceChildren(); status.hidden = !invalid && !pending && (!snapshot || snapshot.phase === 'idle');
    if (invalid) { status.setAttribute('role', 'alert'); status.append(paragraph('The store response could not be read. Refresh the page to try again.')); }
    else if (pending && (!snapshot || !storeBusy(snapshot.phase))) status.append(paragraph('Requesting extension package…', 'bs-status-title'));
    else if (snapshot && snapshot.phase !== 'idle') {
      const s = snapshot;
      const heading = el('h2', 'bs-status-title'); heading.tabIndex = -1; heading.textContent = storeStatus(s); status.append(heading);
      status.setAttribute('role', s.phase === 'error' ? 'alert' : 'status');
      status.setAttribute('aria-busy', String(s.phase === 'downloading' || s.phase === 'installing'));
      if (s.phase === 'downloading' || s.phase === 'installing') status.append(el('span', 'bw-spinner'));
      if (s.phase === 'review') {
        status.append(paragraph('Review the resolved package below. Required access is granted when the extension is on. Bowser cannot narrow it. Optional declarations are shown separately and are not a promise of enforcement.'));
        const lists = el('dl', 'bs-permissions');
        for (const [key, label] of PERMISSION_GROUPS) {
          const term = el('dt'); term.textContent = label; const values = el('dd');
          const list = el('ul');
          for (const value of s[key]) { const item = el('li'); item.textContent = value; list.append(item); }
          if (s[key].length === 0) { const item = el('li'); item.textContent = 'None declared'; list.append(item); }
          values.append(list); lists.append(term, values);
        }
        const approve = textButton(`Install ${s.name} ${s.version}`, () => {
          const command = storeRequestCommand('storeapprove', s.requestId);
          if (command && !pending && options.send(command)) { pending = true; render(); }
        }, { primary: true }); approve.dataset.focus = 'approve'; approve.disabled = pending;
        status.append(lists, approve);
      }
      if ((s.phase === 'downloading' || s.phase === 'review') && s.requestId) {
        const cancel = textButton('Cancel installation', () => {
          const command = storeRequestCommand('storecancel', s.requestId);
          if (command && options.send(command)) { pending = true; render(); }
        }); cancel.dataset.focus = 'cancel'; cancel.disabled = pending; status.append(cancel);
      }
      if (s.phase === 'ready') status.append(textButton('Manage installed extensions', () => { view = 'installed'; selected = ''; render(); }));
    }
    renderBody();
    if (focusKey) root.querySelector<HTMLElement>(`[data-focus="${focusKey}"]`)?.focus();
  }
  render();
  return { root, hide: () => { root.hidden = true; snapshot = null; pending = false; invalid = false; status.replaceChildren(); }, receive: (message) => {
    if (message.p === 'extensions') { installed.receive(message); return null; }
    if (message.p !== 'store') return null;
    const parsed = parseStoreSnapshot(message.d);
    if (!parsed) { invalid = true; snapshot = null; pending = false; render(); return null; }
    const entering = root.hidden && parsed.shown;
    const reviewing = parsed.shown && parsed.phase === 'review' && (snapshot?.phase !== 'review' || snapshot.requestId !== parsed.requestId);
    invalid = false; snapshot = parsed; pending = false; root.hidden = !parsed.shown; render(); if (reviewing) status.querySelector<HTMLElement>('h2')?.focus(); else if (entering) title.focus(); return parsed.shown;
  } };
}
