// Built-in store uses the trusted settings bridge. No frame, external assets or URL navigation.
import { el, icon } from './dom';
import { textButton } from './panels/kit';
import { mountExtensionsSection, type ExtensionsSectionOptions } from './extensions-section';
import type { PanelFields, SnapshotMsg } from './protocol';
import { filterCatalogue, isStoreId, parseStoreSnapshot, PERMISSION_GROUPS, storeBusy, storeDisplayName, storeFailure, storeInstallCommand, storeProgress, storeRequestCommand, storeRetryCommand, storeStatus, storeSteps, type StoreEntry, type StoreProgress, type StoreSnapshot, type StoreStepView } from './store-logic';

export function mountStoreSection(options: ExtensionsSectionOptions): { readonly root: HTMLElement; receive(message: SnapshotMsg): boolean | null; hide(): void } {
  const root = el('div', 'bs bw-scroll');
  root.hidden = true;
  let snapshot: StoreSnapshot | null = null;
  let view: 'discover' | 'categories' | 'installed' = 'discover';
  let category = '';
  let selected = '';
  let pending = false;
  let invalid = false;
  let requested = ''; // the ID of a request sent but not yet answered, for the card's identity row
  const header = el('header', 'bs-header');
  const brand = el('div', 'bs-brand tw:flex tw:items-center tw:gap-3');
  const tile = el('span', 'bs-brand-icon'); tile.append(icon('puzzle', { size: 24 }));
  const title = el('h1'); title.textContent = 'Extension installation'; title.tabIndex = -1; brand.append(tile, title);
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
  const status = el('section', 'bs-status'); status.setAttribute('aria-label', 'Extension installation');
  // Two persistent live regions: the card itself is rebuilt on every snapshot (progress ticks too),
  // so it is not live; these say each new state once, errors assertively.
  const live = el('p', 'tw:sr-only'); live.setAttribute('role', 'status'); live.setAttribute('aria-live', 'polite');
  const alertLive = el('p', 'tw:sr-only'); alertLive.setAttribute('role', 'alert');
  const feedback = el('p', 'bs-feedback'); feedback.hidden = true; feedback.setAttribute('aria-live', 'polite');
  const installed = mountExtensionsSection({ ...options, idPrefix: 'bs', toast: (text, ok) => { feedback.textContent = text; feedback.hidden = false; feedback.setAttribute('role', ok ? 'status' : 'alert'); } });
  delete installed.root.dataset.row; // Store management is not a second searchable Settings row.
  installed.root.classList.add('bs-installed');
  main.append(search, feedback, live, alertLive, status, body, installed.root);
  layout.append(side, main); root.append(header, layout);
  search.addEventListener('input', () => { selected = ''; renderBody(); });
  root.addEventListener('keydown', (event) => {
    if (event.key === 'f' && event.ctrlKey && !event.altKey && !event.shiftKey) { event.preventDefault(); if (snapshot && snapshot.phase !== 'idle') return; view = 'discover'; selected = ''; renderBody(); search.focus(); search.select(); }
  });
  const sendInstall = (id: string): void => {
    const command = storeInstallCommand(id);
    if (!command || pending || (snapshot && storeBusy(snapshot.phase))) return;
    if (options.send(command)) { pending = true; requested = id; render(); }
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
    const reviewing = !!snapshot && snapshot.phase !== 'idle';
    installed.root.hidden = view !== 'installed'; search.hidden = reviewing || view === 'installed'; body.hidden = reviewing || view === 'installed'; side.hidden = reviewing;
    for (const [key, button] of navButtons) { if (key === view) button.setAttribute('aria-current', 'page'); else button.removeAttribute('aria-current'); }
    if (reviewing || view === 'installed') return;
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
  // ---- The installation card: which package, the four steps, the state, then its actions ----
  const STEP_WORDS = { done: 'done', current: 'in progress', failed: 'failed', todo: 'not started' } as const;
  let brokenIcon = ''; // an icon that failed to decode: the card is rebuilt per snapshot, so don't retry it
  /** The package's own icon once Rust has read it from the package (review onwards), else the puzzle. */
  const identityTile = (src: string): HTMLElement => {
    const tile = el('span', 'bs-ident-icon');
    const glyph = (): void => { tile.classList.remove('has-image'); tile.replaceChildren(icon('puzzle', { size: 24 })); };
    if (!src || src === brokenIcon) { glyph(); return tile; }
    const img = el('img', 'bs-ident-img');
    img.alt = ''; // decorative: the name sits beside it
    img.draggable = false;
    img.addEventListener('error', () => { brokenIcon = src; glyph(); });
    img.src = src;
    tile.classList.add('has-image'); tile.append(img); return tile;
  };
  const identity = (s: Pick<StoreSnapshot, 'name' | 'version' | 'extensionId' | 'catalogue'> & { readonly icon?: string }): HTMLElement => {
    const row = el('div', 'bs-ident');
    const tile = identityTile(s.icon ?? '');
    const text = el('div', 'bs-ident-text');
    const name = el('p', 'bs-ident-name'); name.textContent = storeDisplayName(s as StoreSnapshot);
    const meta = el('p', 'bs-ident-meta');
    if (s.version) { const version = el('span'); version.textContent = `Version ${s.version}`; meta.append(version); }
    if (isStoreId(s.extensionId)) { const id = el('span', 'bs-ident-id'); id.textContent = `ID ${s.extensionId}`; meta.append(id); }
    text.append(name, meta); row.append(tile, text); return row;
  };
  const stepper = (steps: readonly StoreStepView[]): HTMLElement => {
    const list = el('ol', 'bs-steps'); list.setAttribute('aria-label', 'Installation steps');
    for (const [index, step] of steps.entries()) {
      const item = el('li', 'bs-step'); item.dataset.state = step.state;
      if (step.state === 'current' || step.state === 'failed') item.setAttribute('aria-current', 'step');
      const mark = el('span', 'bs-step-mark');
      if (step.state === 'done') mark.append(icon('check', { size: 12, fine: true }));
      else if (step.state === 'failed') mark.append(icon('close', { size: 12, fine: true }));
      else mark.textContent = String(index + 1);
      const label = el('span', 'bs-step-label'); label.textContent = step.label;
      const said = el('span', 'tw:sr-only'); said.textContent = `, ${STEP_WORDS[step.state]}`;
      item.append(mark, label, said); list.append(item);
    }
    return list;
  };
  const progressBar = (p: StoreProgress): HTMLElement => {
    const wrap = el('div', 'bs-progress');
    const bar = el('div', `bs-bar ${p.kind === 'determinate' ? 'is-determinate' : 'is-indeterminate'}`);
    bar.setAttribute('role', 'progressbar'); bar.setAttribute('aria-label', p.label);
    bar.append(el('span', 'bs-bar-fill')); wrap.append(bar);
    if (p.kind === 'determinate') {
      const percent = Math.round(p.value * 100);
      bar.setAttribute('aria-valuemin', '0'); bar.setAttribute('aria-valuemax', '100'); bar.setAttribute('aria-valuenow', String(percent)); bar.setAttribute('aria-valuetext', p.text);
      bar.style.setProperty('--bs-f', String(p.value));
      wrap.append(paragraph(`${p.text} · ${percent}%`, 'bs-progress-text'));
    }
    return wrap;
  };
  /** The state heading (focus target after a step changes) with its explanation, and an optional glyph. */
  const head = (text: string, explain: string, tone?: 'ok' | 'error' | 'quiet'): HTMLElement => {
    const box = el('div', 'bs-head');
    if (tone) {
      const mark = el('span', 'bs-head-mark'); mark.dataset.tone = tone;
      mark.append(icon(tone === 'ok' ? 'check' : tone === 'error' ? 'warning' : 'info', { size: 20 }));
      box.append(mark);
    }
    const words = el('div', 'bs-head-text');
    const heading = el('h2', 'bs-status-title'); heading.tabIndex = -1; heading.textContent = text;
    words.append(heading);
    if (explain) words.append(paragraph(explain, 'bs-head-body'));
    box.append(words); return box;
  };
  const action = (label: string, key: string, run: () => void, opts: { primary?: boolean; icon?: 'external' } = {}): HTMLButtonElement => {
    const button = textButton(label, run, opts); button.dataset.focus = key; return button;
  };
  const browseButton = (): HTMLButtonElement => action('Open Chrome Web Store', 'browse', () => options.send({ a: 'storebrowse' }), { icon: 'external' });
  const retry = (command: PanelFields): void => {
    if (pending || !options.send(command)) return;
    pending = true; requested = command.text ?? ''; render();
  };
  let techOpen = false;
  const technical = (detail: string): HTMLElement => {
    const box = el('details', 'bs-tech'); box.open = techOpen;
    box.addEventListener('toggle', () => { techOpen = box.open; });
    const summary = el('summary'); summary.textContent = 'Technical details'; summary.dataset.focus = 'tech';
    const text = el('p', 'bs-tech-text'); text.textContent = detail;
    box.append(summary, text); return box;
  };
  const review = (s: StoreSnapshot): HTMLElement[] => {
    const listing = s.catalogue.find((entry) => entry.id === s.extensionId);
    const about = el('section', 'bs-section'); about.setAttribute('aria-labelledby', 'bs-about-h');
    const aboutHead = el('h3', 'bw-section-header'); aboutHead.id = 'bs-about-h'; aboutHead.textContent = 'About this package';
    const facts = el('dl', 'bs-facts bs-review-facts');
    for (const [label, value] of [
      ['Store / signing ID', s.extensionId],
      ['Package version', s.version],
      ['Publisher', listing ? `${listing.publisher} (catalogue attribution, not independently verified for this package)` : 'Unknown. Not independently verified.'],
      ['Package source', `https://chromewebstore.google.com/detail/${s.extensionId}`],
      ['Provenance', listing ? 'Curated listing metadata, not a verified publisher assertion.' : 'Requested from a validated Chrome Web Store listing. Not part of the curated Bowser catalogue.'],
      ['Compatibility', 'Untested for this resolved package version. Installation alone does not establish compatibility.'],
    ] as const) {
      const term = el('dt'); term.textContent = label;
      const valueText = el('dd'); valueText.textContent = value; facts.append(term, valueText);
    }
    const source = action('View package source', 'source', () => options.send({ a: 'storelisting', text: s.extensionId }), { icon: 'external' });
    about.append(aboutHead, facts, source);
    const access = el('section', 'bs-section'); access.setAttribute('aria-labelledby', 'bs-access-h');
    const accessHead = el('h3', 'bw-section-header'); accessHead.id = 'bs-access-h'; accessHead.textContent = 'What it can access';
    const groups = el('div', 'bs-permissions');
    const missing: string[] = [];
    for (const [key, label] of PERMISSION_GROUPS) {
      const values = s[key];
      if (values.length === 0) { missing.push(label); continue; }
      const group = el('div', 'bs-perm'); group.dataset.group = key;
      const name = el('h4', 'bs-perm-name'); name.textContent = label;
      const count = el('span', 'bs-perm-count'); count.textContent = String(values.length); name.append(count);
      const list = el('ul', key === 'permissions' || key === 'optionalPermissions' ? 'bs-chips' : key === 'contentScriptDeclarations' ? 'bs-code' : 'bs-list');
      for (const value of values) { const item = el('li'); item.textContent = value; list.append(item); }
      group.append(name, list);
      if (key === 'contentScriptDeclarations') group.append(paragraph('Complete static declarations preserve exclusions, file order, frame access, execution world and timing. Documented defaults for omitted fields are shown explicitly, not a guarantee of WebView2 behavior. Unrecognized fields are preserved without assuming their effect. Runtime injection may also use the declared extension APIs.'));
      groups.append(group);
    }
    if (missing.length === PERMISSION_GROUPS.length) groups.append(paragraph('This package declares no permissions, site access or content scripts.', 'bs-perm-none'));
    else if (missing.length) groups.append(paragraph(`None declared: ${missing.join(' · ')}`, 'bs-perm-none'));
    access.append(accessHead, groups);
    return [access, about];
  };
  const fill = (s: StoreSnapshot): { readonly said: string; readonly alert: boolean } => {
    const failure = s.phase === 'error' ? storeFailure(s.error) : null;
    status.dataset.phase = s.phase;
    const packageIcon = s.phase === 'review' || s.phase === 'installing' || s.phase === 'ready' ? s.icon : '';
    if (!failure || failure.identity) status.append(identity({ ...s, icon: packageIcon }));
    const steps = storeSteps(s);
    if (steps.length) status.append(stepper(steps));
    const actions = el('div', 'bs-actions');
    const cancel = (): HTMLButtonElement => {
      const button = action('Cancel installation', 'cancel', () => {
        const command = storeRequestCommand('storecancel', s.requestId);
        if (command && options.send(command)) { pending = true; render(); }
      }); button.disabled = pending; return button;
    };
    const again = storeRetryCommand(s);
    const retryButton = (primary: boolean): HTMLButtonElement => {
      const button = action('Try again', 'retry', () => { if (again) retry(again); }, { primary }); button.disabled = pending; return button;
    };
    switch (s.phase) {
      case 'downloading':
        status.append(head(storeStatus(s), 'Bowser checks the package’s signature and contents before it shows you what the extension can access.'), progressBar(storeProgress(s)!));
        if (s.requestId) actions.append(cancel());
        break;
      case 'review': {
        status.append(head(storeStatus(s), 'Required access is granted whenever the extension is on, and Bowser cannot narrow it. Optional declarations are shown separately and are not a promise of enforcement.'), ...review(s));
        const approve = action(`Install ${s.name} ${s.version}`, 'approve', () => {
          const command = storeRequestCommand('storeapprove', s.requestId);
          if (command && !pending && options.send(command)) { pending = true; render(); }
        }, { primary: true }); approve.disabled = pending;
        actions.append(approve);
        if (s.requestId) actions.append(cancel());
        actions.classList.add('is-sticky');
        break;
      }
      case 'installing':
        status.append(head(storeStatus(s), `Adding ${storeDisplayName(s)} to this profile.`), progressBar(storeProgress(s)!));
        break;
      case 'ready':
        status.append(head(storeStatus(s), s.restart ? 'Restart Bowser to finish loading it. It does not run until then.' : 'Find it in Installed, where you can turn it off, check its access or remove it.', 'ok'));
        actions.append(action('Manage installed extensions', 'manage', () => { view = 'installed'; selected = ''; render(); }), browseButton());
        break;
      case 'error':
        status.append(head(failure!.title, failure!.body, 'error'));
        if (failure!.detail) status.append(technical(failure!.detail));
        if (again) actions.append(retryButton(failure!.retry === 'suggest'));
        if (failure!.manage) actions.append(action('Manage installed extensions', 'manage', () => { view = 'installed'; selected = ''; render(); }));
        actions.append(browseButton());
        break;
      case 'cancelled':
        status.append(head(storeStatus(s), 'Nothing was installed. You can start again whenever you like.', 'quiet'));
        if (again) actions.append(retryButton(false));
        actions.append(browseButton());
        break;
      case 'idle': break;
    }
    if (actions.childElementCount) status.append(actions);
    const said = s.phase === 'error' ? `${failure!.title}. ${failure!.body}` : s.phase === 'ready' && s.restart ? `${storeStatus(s)}. Restart Bowser to finish loading it.` : storeStatus(s);
    return { said, alert: s.phase === 'error' };
  };
  const announce = (said: string, alert: boolean): void => {
    const [speak, quiet] = alert ? [alertLive, live] : [live, alertLive];
    quiet.textContent = '';
    if (speak.textContent !== said) speak.textContent = said; // progress ticks re-render; say each state once
  };
  function render(): void {
    const focused = document.activeElement as HTMLElement | null;
    const hadFocus = !!focused && root.contains(focused);
    const focusKey = hadFocus ? focused!.dataset.focus : undefined;
    const reviewing = !!snapshot && snapshot.phase !== 'idle';
    status.replaceChildren(); delete status.dataset.phase;
    status.hidden = !invalid && !pending && !reviewing;
    if (invalid) {
      status.dataset.phase = 'error';
      status.append(head('The store response could not be read', 'Refresh the page to try again.', 'error'));
      announce('The store response could not be read. Refresh the page to try again.', true);
    } else if (pending && (!snapshot || !storeBusy(snapshot.phase))) {
      status.dataset.phase = 'downloading';
      if (isStoreId(requested)) status.append(identity({ name: '', version: '', extensionId: requested, catalogue: snapshot?.catalogue ?? [] }));
      status.append(head('Requesting the extension package…', ''), progressBar({ kind: 'indeterminate', label: 'Download progress' }));
      announce('Requesting the extension package…', false);
    } else if (snapshot && reviewing) {
      const { said, alert } = fill(snapshot);
      announce(said, alert);
    } else announce('', false);
    renderBody();
    // Production entrypoints browse Google's original site. The internal host is only review /
    // management; never present the old curated cards behind an active installation request.
    layout.classList.toggle('is-review', reviewing);
    root.classList.toggle('is-install', reviewing);
    side.hidden = reviewing;
    if (reviewing) { search.hidden = true; body.hidden = true; }
    if (focusKey) root.querySelector<HTMLElement>(`[data-focus="${focusKey}"]`)?.focus();
    // The control that had focus went with the old state (Cancel, Install, Try again): keep the
    // keyboard on the card by moving to its new heading rather than dropping to the document.
    if (hadFocus && !root.contains(document.activeElement)) (status.hidden ? title : status.querySelector<HTMLElement>('h2') ?? title).focus();
  }
  render();
  return { root, hide: () => { root.hidden = true; snapshot = null; brokenIcon = ''; pending = false; invalid = false; requested = ''; techOpen = false; status.replaceChildren(); live.textContent = ''; alertLive.textContent = ''; }, receive: (message) => {
    if (message.p === 'extensions') { installed.receive(message); return null; }
    if (message.p !== 'store') return null;
    const parsed = parseStoreSnapshot(message.d);
    if (!parsed) { invalid = true; snapshot = null; pending = false; render(); return null; }
    const entering = root.hidden && parsed.shown;
    const reviewing = parsed.shown && parsed.phase === 'review' && (snapshot?.phase !== 'review' || snapshot.requestId !== parsed.requestId);
    if (parsed.phase !== 'error') techOpen = false;
    invalid = false; snapshot = parsed; pending = false; requested = ''; root.hidden = !parsed.shown; render(); if (reviewing) status.querySelector<HTMLElement>('h2')?.focus(); else if (entering) title.focus(); return parsed.shown;
  } };
}
