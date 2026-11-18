// Bowser's extension installation page (bowser://store over ?view=settings). It shows one card while
// a package is in flight: which extension, what it can do, and Add or Cancel. Otherwise it is the
// installed-extensions manager. Uses the trusted settings bridge; no frame, external assets or
// URL navigation. Discovery is the stores' own websites (Add to Bowser in the toolbar).
import { el, icon } from './dom';
import { textButton } from './panels/kit';
import { mountExtensionsSection, type ExtensionsSectionOptions } from './extensions-section';
import type { PanelFields, SnapshotMsg } from './protocol';
import {
  isStoreId, parseStoreSnapshot, PERMISSION_GROUPS, STORE_SOURCES, storeAccess, storeBusy, storeDisplayName, storeFailure, storeListingCommand, storeProgress,
  storeRequestCommand, storeRetryCommand, storeStatus, type StoreProgress, type StoreSnapshot,
} from './store-logic';

export function mountStoreSection(options: ExtensionsSectionOptions): { readonly root: HTMLElement; receive(message: SnapshotMsg): boolean | null; hide(): void } {
  const root = el('div', 'bs bw-scroll');
  root.hidden = true;
  let snapshot: StoreSnapshot | null = null;
  let pending = false;
  let invalid = false;
  let requested = ''; // the ID of a request sent but not yet answered, for the card's identity row
  const header = el('header', 'bs-header');
  const title = el('h1'); title.textContent = 'Extensions'; title.tabIndex = -1;
  header.append(title, textButton('Settings', () => options.send({ a: 'section', text: 'extensions' }), { icon: 'gear', subtle: true, aria: 'Extension settings' }));
  const status = el('section', 'bs-status'); status.setAttribute('aria-label', 'Extension installation');
  // Two persistent live regions: the card itself is rebuilt on every snapshot (progress ticks too),
  // so it is not live; these say each new state once, errors assertively.
  const live = el('p', 'tw:sr-only'); live.setAttribute('role', 'status'); live.setAttribute('aria-live', 'polite');
  const alertLive = el('p', 'tw:sr-only'); alertLive.setAttribute('role', 'alert');
  const feedback = el('p', 'bs-feedback'); feedback.hidden = true; feedback.setAttribute('aria-live', 'polite');
  const installed = mountExtensionsSection({ ...options, idPrefix: 'bs', toast: (text, ok) => { feedback.textContent = text; feedback.hidden = false; feedback.setAttribute('role', ok ? 'status' : 'alert'); } });
  delete installed.root.dataset.row; // Store management is not a second searchable Settings row.
  installed.root.classList.add('bs-installed');
  const main = el('main', 'bs-main');
  main.append(feedback, live, alertLive, status, installed.root);
  root.append(header, main);

  const paragraph = (value: string, cls: string): HTMLElement => { const p = el('p', cls); p.textContent = value; return p; };

  // ---- The card: which package, its state, then its actions ----
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
  /** The tile and the state heading (the focus target after a step changes), with an optional line under it. */
  const head = (s: Pick<StoreSnapshot, 'icon'> | null, text: string, line = '', tone?: 'ok' | 'error'): HTMLElement => {
    const box = el('div', 'bs-head');
    if (s) box.append(identityTile(s.icon));
    else if (tone) { const mark = el('span', 'bs-head-mark'); mark.dataset.tone = tone; mark.append(icon(tone === 'ok' ? 'check' : 'warning', { size: 20 })); box.append(mark); }
    const words = el('div', 'bs-head-text');
    const heading = el('h2', 'bs-status-title'); heading.tabIndex = -1; heading.textContent = text;
    words.append(heading);
    if (line) words.append(paragraph(line, 'bs-head-body'));
    box.append(words);
    if (s && tone) { const mark = el('span', 'bs-head-badge'); mark.dataset.tone = tone; mark.append(icon(tone === 'ok' ? 'check' : 'warning', { size: 16 })); box.append(mark); }
    return box;
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
      wrap.append(paragraph(p.text, 'bs-progress-text'));
    }
    return wrap;
  };
  const action = (label: string, key: string, run: () => void, opts: { primary?: boolean; subtle?: boolean; icon?: 'external'; aria?: string } = {}): HTMLButtonElement => {
    const button = textButton(label, run, opts); button.dataset.focus = key; return button;
  };
  const retry = (command: PanelFields): void => {
    if (pending || !options.send(command)) return;
    pending = true; requested = (command.text ?? '').replace(/^edge:/, ''); render();
  };
  // Folded sections keep their state across the re-renders a snapshot causes.
  const folded = { details: false, tech: false };
  const fold = (key: keyof typeof folded, label: string, cls: string, ...content: HTMLElement[]): HTMLElement => {
    const box = el('details', cls); box.open = folded[key];
    box.addEventListener('toggle', () => { folded[key] = box.open; });
    const summary = el('summary'); summary.textContent = label; summary.dataset.focus = key;
    box.append(summary, ...content); return box;
  };
  /** What it can do, in plain words, then every declared list in full under Details. */
  const review = (s: StoreSnapshot): HTMLElement[] => {
    const { lines, later } = storeAccess(s);
    const can = el('div', 'bs-access');
    if (lines.length) {
      const list = el('ul', 'bs-access-list'); list.setAttribute('aria-label', 'It can');
      for (const line of lines) { const item = el('li'); item.textContent = line; list.append(item); }
      can.append(list);
    } else can.append(paragraph('It needs no special access.', 'bs-access-none'));
    if (later) can.append(paragraph('It may ask for more access later.', 'bs-access-note'));
    const facts = el('dl', 'bs-facts');
    const fact = (label: string, value: HTMLElement | string): void => {
      const term = el('dt'); term.textContent = label;
      const data = el('dd'); if (typeof value === 'string') data.textContent = value; else data.append(value);
      facts.append(term, data);
    };
    const listing = action(STORE_SOURCES[s.source], 'listing', () => { const command = storeListingCommand(s.extensionId, s.source); if (command) options.send(command); }, { subtle: true, icon: 'external', aria: `Open its ${STORE_SOURCES[s.source]} page` });
    fact('From', listing);
    fact('Version', s.version);
    fact('ID', s.extensionId);
    for (const [key, label] of PERMISSION_GROUPS) {
      const values = s[key];
      if (values.length === 0) continue;
      const list = el('ul', key === 'contentScriptDeclarations' ? 'bs-code' : 'bs-chips');
      for (const value of values) { const item = el('li'); item.textContent = value; list.append(item); }
      fact(label, list);
    }
    return [can, fold('details', 'Details', 'bs-more', facts)];
  };
  const fill = (s: StoreSnapshot): { readonly said: string; readonly alert: boolean } => {
    const failure = s.phase === 'error' ? storeFailure(s.error) : null;
    status.dataset.phase = s.phase;
    // The package's icon only once Rust has verified the package (review onwards).
    const own = { icon: s.phase === 'review' || s.phase === 'installing' || s.phase === 'ready' ? s.icon : '' };
    const actions = el('div', 'bs-actions');
    const cancel = (): HTMLButtonElement => {
      const button = action('Cancel', 'cancel', () => {
        const command = storeRequestCommand('storecancel', s.requestId);
        if (command && options.send(command)) { pending = true; render(); }
      }, { aria: 'Cancel installation' }); button.disabled = pending; return button;
    };
    const again = storeRetryCommand(s);
    const retryButton = (primary: boolean): HTMLButtonElement => {
      const button = action('Try again', 'retry', () => { if (again) retry(again); }, { primary }); button.disabled = pending; return button;
    };
    const manage = (): HTMLButtonElement => action('Manage extensions', 'manage', () => options.send({ a: 'section', text: 'extensions' }));
    const name = storeDisplayName(s);
    switch (s.phase) {
      case 'downloading':
        status.append(head(own, name, storeStatus(s)), progressBar(storeProgress(s)!));
        if (s.requestId) actions.append(cancel());
        break;
      case 'review': {
        status.append(head(own, storeStatus(s), `Version ${s.version}`), ...review(s));
        const approve = action('Add extension', 'approve', () => {
          const command = storeRequestCommand('storeapprove', s.requestId);
          if (command && !pending && options.send(command)) { pending = true; render(); }
        }, { primary: true, aria: `Add ${name} ${s.version}` }); approve.disabled = pending;
        actions.append(approve);
        if (s.requestId) actions.append(cancel());
        actions.classList.add('is-sticky');
        break;
      }
      case 'installing':
        status.append(head(own, name, storeStatus(s)), progressBar(storeProgress(s)!));
        break;
      case 'ready':
        status.append(head(own, storeStatus(s), s.restart ? 'Restart Bowser to start it.' : '', 'ok'));
        actions.append(manage());
        break;
      case 'error':
        status.append(head(failure!.identity ? own : null, failure!.title, failure!.body, 'error'));
        if (failure!.detail) status.append(fold('tech', 'Details', 'bs-tech', paragraph(failure!.detail, 'bs-tech-text')));
        if (again) actions.append(retryButton(failure!.retry === 'suggest'));
        if (failure!.manage) actions.append(manage());
        break;
      case 'cancelled':
        status.append(head(own, name, storeStatus(s)));
        if (again) actions.append(retryButton(false));
        break;
      case 'idle': break;
    }
    if (actions.childElementCount) status.append(actions);
    const said = s.phase === 'error' ? [failure!.title, failure!.body].filter(Boolean).join('. ')
      : s.phase === 'ready' && s.restart ? `${storeStatus(s)}. Restart Bowser to start it.`
      : s.phase === 'downloading' || s.phase === 'installing' || s.phase === 'cancelled' ? `${name}: ${storeStatus(s)}` : storeStatus(s);
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
      status.append(head(null, 'Something went wrong', 'Reload the page to try again.', 'error'));
      announce('Something went wrong. Reload the page to try again.', true);
    } else if (pending && (!snapshot || !storeBusy(snapshot.phase))) {
      status.dataset.phase = 'downloading';
      const known = isStoreId(requested) ? snapshot?.catalogue.find((entry) => entry.id === requested)?.name ?? '' : '';
      status.append(head({ icon: '' }, known || 'Getting the extension', known ? 'Getting it…' : ''), progressBar({ kind: 'indeterminate', label: 'Download progress' }));
      announce('Getting the extension…', false);
    } else if (snapshot && reviewing) {
      const { said, alert } = fill(snapshot);
      announce(said, alert);
    } else announce('', false);
    installed.root.hidden = reviewing || pending || invalid;
    root.classList.toggle('is-install', installed.root.hidden);
    if (focusKey) root.querySelector<HTMLElement>(`[data-focus="${focusKey}"]`)?.focus();
    // The control that had focus went with the old state (Cancel, Add, Try again): keep the keyboard
    // on the card by moving to its new heading rather than dropping to the document.
    if (hadFocus && !root.contains(document.activeElement)) (status.hidden ? title : status.querySelector<HTMLElement>('h2') ?? title).focus();
  }
  render();
  return { root, hide: () => { root.hidden = true; snapshot = null; brokenIcon = ''; pending = false; invalid = false; requested = ''; folded.details = false; folded.tech = false; status.replaceChildren(); live.textContent = ''; alertLive.textContent = ''; }, receive: (message) => {
    if (message.p === 'extensions') { installed.receive(message); return null; }
    if (message.p !== 'store') return null;
    const parsed = parseStoreSnapshot(message.d);
    if (!parsed) { invalid = true; snapshot = null; pending = false; render(); return null; }
    const entering = root.hidden && parsed.shown;
    const reviewing = parsed.shown && parsed.phase === 'review' && (snapshot?.phase !== 'review' || snapshot.requestId !== parsed.requestId);
    if (parsed.phase !== 'error') folded.tech = false;
    if (reviewing) folded.details = false;
    invalid = false; snapshot = parsed; pending = false; requested = ''; root.hidden = !parsed.shown; render(); if (reviewing) status.querySelector<HTMLElement>('h2')?.focus(); else if (entering) title.focus(); return parsed.shown;
  } };
}
