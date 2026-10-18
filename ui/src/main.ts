// Toolbar entry point: wires DOM events to the host bridge and host messages to the view.
import {
  MAX_TEXT,
  onInbound,
  send,
  sendDownloadToggle,
  sendAddr,
  sendExtensionsButton,
  rectWire,
  sendSuggestEnd,
  sendSuggestPick,
  sendSuggestPreview,
  sendTool, // --- toolbars and feeds (thread T13) ---
  type CaptionAction,
  type CompletionMsg,
  type NavAction,
  type OpenTarget,
  type SearchFieldMsg,
  type SuggestMsg,
} from './protocol';
// --- address bar, search and bookmarks ---
import { DEFAULT_ADDR, blurAddressText, clipboardText, focusAddressText, keyboardFocusSelection, showsTypedHistoryOnFocus, stripPastedJavascript, submitKind, type AddrFlags } from './address-logic';
import { engineLabel, monogram } from './address-settings-logic';
// --- end address bar, search and bookmarks ---
import { moveSuggestSelection, suggestPreviewText } from './suggest-logic'; // --- omnibox suggest ---
import { applyMenuOpen, applyState, applyTheme, buildView, DRAG_ATTR, renderDisplay, syncSiteIcon } from './view';
import { startTabs } from './tabs';
import { startRail } from './rail';
import { startPanel } from './panel';
import { startPopup } from './popup';
import { startDialog } from './dialog';
import { startBookmarkBar } from './bookmark-bar'; // --- address bar, search and bookmarks ---
// Types only: the settings and history pages are separate bundles (see loadPage).
import type { startSettingsPage } from './settings-page';
import type { startHistoryPage } from './history-page';
import { startDownloadsPage } from './downloads-page';
// --- split view ---
import { startSplit } from './split';
import { syncSplitButton } from './split-chrome';
// --- end split view ---
// --- hover layer ---
import { startHover } from './hover';
import { installHoverTargets, installTooltips, tabStripHoverTarget } from './tooltip';
// --- end hover layer ---

/** True only when the event target is the drag surface itself, never a control inside it. */
function onDragSurface(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && target.hasAttribute(DRAG_ATTR);
}

function startToolbar(root: HTMLElement): void {
  const view = buildView(root);
  const input = view.input;

  let nextSeq = 0;
  let latestSeq = -1;
  let latestText = '';
  let lastInputType = '';
  let pending = false;
  let frame = 0;
  let loading = false;
  let composing = false;
  // --- omnibox suggest ---
  // The dropdown's rows, as Rust last sent them for the latest edit (the popup draws them).
  let list: SuggestMsg | null = null;
  let listSel = 0;
  // --- end omnibox suggest ---
  // --- address bar, search and bookmarks ---
  let flags: AddrFlags = DEFAULT_ADDR;
  let sfield: SearchFieldMsg | null = null;
  const searchInput = view.search.input;
  // --- end address bar, search and bookmarks ---

  // At most one edit per animation frame, each with a fresh seq.
  const flush = (): void => {
    frame = 0;
    if (!pending || composing) return;
    pending = false;
    const text = input.value;
    if (text.length > MAX_TEXT) return;
    latestSeq = nextSeq;
    nextSeq += 1;
    latestText = text;
    send({ v: 1, t: 'edit', seq: latestSeq, text });
  };

  // --- address bar, search and bookmarks ---
  /** The list of the other field is stale the moment this one is edited: only the latest edit counts. */
  const newEdit = (text: string): number => {
    latestSeq = nextSeq;
    nextSeq += 1;
    latestText = text;
    list = null;
    return latestSeq;
  };
  const flushSearch = (): void => {
    const text = searchInput.value;
    if (!sfield || text.length > MAX_TEXT) return;
    const seq = newEdit(text);
    sendAddr({ a: 'sedit', text, id: sfield.engine, n: seq });
  };
  let searchFrame = 0;
  const showSearch = (): void => {
    const visible = sfield !== null && sfield.mode !== 'off';
    view.search.root.hidden = !visible;
    if (!sfield || !visible) return;
    view.search.root.classList.toggle('is-button', sfield.mode === 'button');
    const engine = sfield.engines[sfield.engine];
    const label = engine ? engineLabel({ name: engine.n, nick: engine.k }, sfield.nick) : 'Search';
    searchInput.placeholder = engine ? `Search ${engine.n}` : 'Search';
    searchInput.setAttribute('aria-label', `Search with ${label}`);
    view.search.engine.setAttribute('aria-label', `Search engine: ${label}`);
    view.search.engine.setAttribute('data-tip', `Search engine: ${label}`);
    view.search.tile.textContent = engine ? monogram(engine.n) : '?';
    view.search.tile.hidden = !sfield.icon;
  };
  // --- end address bar, search and bookmarks ---

  const nav = (a: NavAction): void => {
    send({ v: 1, t: 'nav', a });
  };
  const open = (a: OpenTarget): void => {
    send({ v: 1, t: 'open', a });
  };
  const caption = (a: CaptionAction): void => {
    send({ v: 1, t: 'caption', a });
  };

  // Empty toolbar areas: left press starts a native move, double-click toggles maximise,
  // right-click opens the system menu. Controls keep their own mouse behaviour.
  root.addEventListener('pointerdown', (ev) => {
    if (ev.button !== 0 || !onDragSurface(ev.target)) return;
    send({ v: 1, t: 'drag' });
  });
  root.addEventListener('dblclick', (ev) => {
    if (!onDragSurface(ev.target)) return;
    caption('toggleMaximize');
  });
  root.addEventListener('contextmenu', (ev) => {
    if (!onDragSurface(ev.target)) return;
    ev.preventDefault();
    caption('systemMenu');
  });

  view.back.addEventListener('click', () => nav('back'));
  view.forward.addEventListener('click', () => nav('forward'));
  view.reload.addEventListener('click', () => nav(loading ? 'stop' : 'reload'));
  view.site.addEventListener('click', () => open('site'));
  view.split.addEventListener('click', () => open('split'));
  view.zoom.addEventListener('click', () => open('zoomreset'));
  view.reader.addEventListener('click', () => open('reader'));
  view.translate.addEventListener('click', () => open('translate'));
  view.menu.addEventListener('click', () => open('menu'));
  // --- toolbars and feeds (thread T13) --- the buttons the address bar's list adds (docs/toolbars.md).
  const tool = (item: string): void => { sendTool('address', item, view.bar.items.indexOf(item)); };
  view.bookmark.addEventListener('click', () => tool('bookmark'));
  view.newtab.addEventListener('click', () => tool('newtab'));
  view.feed.addEventListener('click', () => open('feeds'));
  // --- end toolbars and feeds ---
  view.downloads.button.addEventListener('click', () => { sendDownloadToggle(view.downloads.button.getBoundingClientRect()); });
  view.extensions.button.addEventListener('click', () => { sendExtensionsButton(view.extensions.button.getBoundingClientRect()); });

  input.addEventListener('input', (ev) => {
    if (composing || (ev instanceof InputEvent && ev.isComposing)) return;
    lastInputType = ev instanceof InputEvent ? ev.inputType : '';
    pending = true;
    if (frame === 0) frame = requestAnimationFrame(flush);
  });

  input.addEventListener('compositionstart', () => { composing = true; });
  input.addEventListener('compositionend', () => {
    composing = false;
    pending = true;
    if (frame === 0) frame = requestAnimationFrame(flush);
  });

  input.addEventListener('keydown', (ev) => {
    if (ev.isComposing || ev.keyCode === 229) return;
    // Ctrl+Shift+V is the tab layout toggle (owned by Rust); never a plain-text paste.
    if (ev.ctrlKey && ev.shiftKey && !ev.altKey && ev.code === 'KeyV') {
      ev.preventDefault();
      return;
    }
    // --- omnibox suggest ---
    if ((ev.key === 'ArrowDown' || ev.key === 'ArrowUp') && !ev.altKey && !ev.ctrlKey && !ev.shiftKey && !ev.metaKey && list && !pending && list.seq === latestSeq) {
      ev.preventDefault();
      listSel = moveSuggestSelection(listSel, list.items.length, ev.key);
      const row = list.items[listSel];
      if (!row) return;
      input.value = suggestPreviewText(latestText, listSel, row.text);
      input.setSelectionRange(input.value.length, input.value.length);
      sendSuggestPreview(list.seq, listSel);
      return;
    }
    // --- address bar, search and bookmarks ---
    if (ev.key === 'ArrowDown' && !ev.altKey && !ev.ctrlKey && !ev.shiftKey && !ev.metaKey && list === null && !pending) {
      // The list is closed: Down opens the typed history on an empty field, or the drop-down for what
      // is typed when typing alone does not open it.
      if (input.value.trim() === '' && flags.typed) {
        ev.preventDefault();
        sendAddr({ a: 'typed', n: newEdit('') });
        return;
      }
      if (input.value.trim() !== '' && !flags.typing) {
        ev.preventDefault();
        sendAddr({ a: 'suggest', text: input.value, n: newEdit(input.value) });
        return;
      }
    }
    // --- end address bar, search and bookmarks ---
    if (ev.key === 'Enter' && list && listSel > 0 && !pending && list.seq === latestSeq && submitKind(ev) === 'plain') {
      ev.preventDefault();
      const seq = list.seq;
      list = null;
      sendSuggestPick(seq, listSel);
      return;
    }
    if (ev.key === 'Escape' && list) {
      // The first Escape only closes the list and puts back what was typed.
      ev.preventDefault();
      if (listSel > 0 && !pending) input.value = latestText;
      list = null;
      sendSuggestEnd();
      return;
    }
    // --- end omnibox suggest ---
    if (ev.key === 'Enter') {
      ev.preventDefault();
      if (input.value.length > MAX_TEXT) return;
      // Alt+Enter and Shift+Enter open a new tab, Ctrl+Enter expands a single word to a domain.
      const kind = submitKind(ev);
      if (kind === 'plain') send({ v: 1, t: 'submit', text: input.value });
      else sendAddr({ a: 'submit', text: input.value, n: kind === 'newtab' ? 1 : 2 });
    } else if (ev.key === 'Escape') {
      ev.preventDefault();
      send({ v: 1, t: 'escape' });
    }
  });

  // --- address bar, search and bookmarks ---
  // A pasted `javascript:` prefix is removed, so a pasted script is plain text, not an address.
  input.addEventListener('paste', (ev) => {
    if (!flags.strip) return;
    const pasted = ev.clipboardData?.getData('text') ?? '';
    const cleaned = stripPastedJavascript(pasted);
    if (cleaned === pasted) return;
    ev.preventDefault();
    input.setRangeText(cleaned, input.selectionStart ?? 0, input.selectionEnd ?? 0, 'end');
    input.dispatchEvent(new InputEvent('input', { inputType: 'insertFromPaste', bubbles: true }));
  });
  // Copying or cutting the whole address puts its percent-encoded form on the clipboard.
  const encodedClipboard = (ev: ClipboardEvent, cut: boolean): void => {
    const text = clipboardText({ enabled: flags.encoded, value: input.value, selStart: input.selectionStart ?? 0, selEnd: input.selectionEnd ?? 0 });
    if (text === null || !ev.clipboardData) return;
    ev.clipboardData.setData('text/plain', text);
    ev.preventDefault();
    if (cut) {
      input.setRangeText('', 0, input.value.length, 'end');
      input.dispatchEvent(new InputEvent('input', { inputType: 'deleteByCut', bubbles: true }));
    }
  };
  input.addEventListener('copy', (ev) => encodedClipboard(ev, false));
  input.addEventListener('cut', (ev) => encodedClipboard(ev, true));
  view.history.addEventListener('click', () => {
    input.focus();
    sendAddr({ a: 'typed', n: newEdit('') });
  });
  view.qr.addEventListener('click', () => { sendAddr({ a: 'qr' }); });

  // The search field: its own text, its own list, and the engine button.
  searchInput.addEventListener('input', (ev) => {
    if (ev instanceof InputEvent && ev.isComposing) return;
    if (searchFrame === 0) searchFrame = requestAnimationFrame(() => { searchFrame = 0; flushSearch(); });
  });
  searchInput.addEventListener('compositionend', () => { flushSearch(); });
  searchInput.addEventListener('keydown', (ev) => {
    if (ev.isComposing || ev.keyCode === 229) return;
    if ((ev.key === 'ArrowDown' || ev.key === 'ArrowUp') && !ev.altKey && !ev.ctrlKey && !ev.shiftKey && !ev.metaKey && list && list.seq === latestSeq) {
      ev.preventDefault();
      listSel = moveSuggestSelection(listSel, list.items.length, ev.key);
      const row = list.items[listSel];
      if (!row) return;
      searchInput.value = suggestPreviewText(latestText, listSel, row.text);
      searchInput.setSelectionRange(searchInput.value.length, searchInput.value.length);
      sendSuggestPreview(list.seq, listSel);
      return;
    }
    if (ev.key === 'Enter') {
      ev.preventDefault();
      if (!sfield || searchInput.value.trim() === '' || searchInput.value.length > MAX_TEXT) return;
      // Alt+Enter and Shift+Enter search in a new tab.
      sendAddr({ a: 'search', text: searchInput.value, id: sfield.engine, n: submitKind(ev) === 'newtab' ? 1 : 0 });
      list = null;
      if (!sfield.keep) searchInput.value = '';
      if (sfield.mode === 'button') view.search.root.classList.remove('is-open');
      return;
    }
    if (ev.key === 'Escape') {
      ev.preventDefault();
      if (list) { list = null; sendSuggestEnd(); return; }
      if (sfield?.mode === 'button') view.search.root.classList.remove('is-open');
      send({ v: 1, t: 'escape' });
    }
  });
  searchInput.addEventListener('focus', () => { send({ v: 1, t: 'focus', on: true }); });
  searchInput.addEventListener('blur', () => {
    list = null;
    send({ v: 1, t: 'focus', on: false });
    if (sfield?.mode === 'button' && searchInput.value === '') view.search.root.classList.remove('is-open');
  });
  view.search.engine.addEventListener('click', () => {
    if (sfield?.mode === 'button' && !view.search.root.classList.contains('is-open')) {
      view.search.root.classList.add('is-open');
      searchInput.focus();
      return;
    }
    const text = rectWire(view.search.engine.getBoundingClientRect());
    if (text !== null) sendAddr({ a: 'engines', text });
  });
  // --- end address bar, search and bookmarks ---

  // Unfocused, a URL shows host-emphasised (view.display); focused, the real input text shows.
  // Updated on setText and blur only, never while typing.
  const showDisplay = (): void => {
    view.pill.classList.toggle('is-display', document.activeElement !== input && renderDisplay(view, input.value, flags));
  };

  // A click on the address selects all of it while it is still the untouched address, as in every
  // browser. The toolbar view keeps `document.activeElement` and `hasFocus()` after the keyboard has
  // gone to the page, so "was the input already focused?" cannot tell a first click from a later one
  // (measured in the real window). What can is whether the user has edited the text since the host
  // last set it: once they type, a click places the caret.
  let selectOnUp = false;
  let pointerIsDown = false;
  let edited = false;
  // minimal_url: the host's condensed text and the full address the focused field holds instead.
  let shownText = '';
  let fullText = '';
  input.addEventListener('input', () => {
    edited = true;
  });
  input.addEventListener('pointerdown', () => {
    pointerIsDown = true;
    selectOnUp = flags.select && (!edited || document.activeElement !== input || !document.hasFocus()); // --- address bar options
  });
  input.addEventListener('pointerup', () => {
    pointerIsDown = false;
    if (selectOnUp && input.selectionStart === input.selectionEnd) input.select();
    selectOnUp = false;
  });

  input.addEventListener('focus', () => {
    view.pill.classList.remove('is-display');
    const focusedText = focusAddressText(input.value, shownText, fullText, edited);
    if (focusedText !== input.value) input.value = focusedText;
    // A press that is still down is what moved the focus here: select now as well as on release.
    if (flags.select && pointerIsDown && input.selectionStart === input.selectionEnd) input.select();
    // --- wave-1 fixes --- keyboard focus (Tab, F6) follows "Select the address on activation" too.
    if (!pointerIsDown) {
      const how = keyboardFocusSelection(flags.select, edited);
      if (how === 'all') input.select();
      else if (how === 'end') input.setSelectionRange(input.value.length, input.value.length);
    }
    send({ v: 1, t: 'focus', on: true });
    // --- address bar, search and bookmarks ---
    // An empty field (or one whose whole text is selected) lists what was typed before.
    queueMicrotask(() => {
      const allSelected = input.value !== '' && input.selectionStart === 0 && input.selectionEnd === input.value.length;
      if (document.activeElement === input && list === null && showsTypedHistoryOnFocus({ enabled: flags.typed, value: input.value, allSelected })) {
        sendAddr({ a: 'typed', n: newEdit('') });
      }
    });
    // --- end address bar, search and bookmarks ---
  });
  input.addEventListener('blur', () => {
    const blurredText = blurAddressText(input.value, shownText, fullText, edited);
    if (blurredText !== input.value) input.value = blurredText;
    edited = false;
    list = null; // --- omnibox suggest ---
    showDisplay();
    send({ v: 1, t: 'focus', on: false });
  });

  // Inline completion is applied only while it still describes exactly what the user has typed.
  const acceptCompletion = (msg: CompletionMsg): void => {
    if (composing || msg.suffix === '' || msg.seq !== latestSeq || pending) return;
    if (document.activeElement !== input || input.value !== latestText) return;
    if (lastInputType.startsWith('deleteContent')) return;
    const end = input.value.length;
    if (input.selectionStart !== end || input.selectionEnd !== end) return;
    if (end + msg.suffix.length > MAX_TEXT) return;
    input.setRangeText(msg.suffix, end, end, 'select');
  };

  onInbound((msg) => {
    switch (msg.t) {
      case 'state':
        loading = msg.loading;
        applyTheme(msg.theme, msg.solid, msg.dark, msg.fontScale, msg.uiFont, msg.density, msg.compactMenus, msg.look);
        applyState(view, msg);
        break;
      case 'setText':
        if (frame !== 0) cancelAnimationFrame(frame);
        frame = 0;
        pending = false;
        latestSeq = -1;
        lastInputType = '';
        list = null; // --- omnibox suggest ---
        input.value = msg.text;
        shownText = msg.text;
        fullText = msg.full ?? '';
        edited = false;
        if (msg.select && flags.select) input.select();
        showDisplay();
        syncSiteIcon(view);
        break;
      case 'focusOmnibox':
        input.focus();
        if (msg.select && flags.select) input.select();
        break;
      case 'completion':
        acceptCompletion(msg);
        break;
      // --- omnibox suggest ---
      case 'suggest':
        // The toolbar's copy (tok 0) drives the keyboard; a list for an older edit is dropped.
        if (msg.tok === 0) {
          list = msg.items.length > 0 && msg.seq === latestSeq ? msg : null;
          listSel = list ? list.sel : 0;
        }
        break;
      case 'suggestOpen':
        // The dropdown is on screen (or fully gone): the pill and the list read as one surface.
        view.pill.classList.toggle('is-open', msg.on);
        break;
      // --- end omnibox suggest ---
      case 'menuOpen':
        applyMenuOpen(view.menu, msg.on);
        break;
      case 'dlbtn':
        view.downloads.apply(msg);
        break;
      // --- address bar, search and bookmarks ---
      case 'addr': {
        const { t: _t, ...next } = msg;
        flags = next;
        view.pill.dataset.hidden = String(!flags.show);
        view.history.hidden = !flags.hbtn;
        view.qr.hidden = !flags.qr;
        showDisplay();
        break;
      }
      case 'sfield':
        sfield = msg;
        showSearch();
        break;
      // --- end address bar, search and bookmarks ---
      case 'extbtn':
        view.extensions.apply(msg);
        break;
      // --- split view ---
      case 'pair':
        syncSplitButton(view.split, msg);
        break;
      // --- end split view ---
    }
  });

  // Registered before ready so no state message can be missed.
  send({ v: 1, t: 'ready' });
}

/**
 * True only when the page's CSP is enforced: with Trusted Types required, assigning a string to an
 * HTML sink must throw. The sink name is assembled at runtime so the bundle holds no literal sink
 * (lint.mjs allows none in dist/app.js).
 */
function trustedTypesEnforced(): boolean {
  const probe = document.createElement('div') as unknown as Record<string, string>;
  const sink = ['inner', 'HTML'].join('');
  try {
    probe[sink] = 'x';
  } catch {
    return true;
  }
  return false;
}

/**
 * Imports a page bundle that build.mjs writes beside app.js, so the views that start with the
 * browser never parse it. The specifier is a variable so esbuild leaves the import() to the browser,
 * which fetches bowser-ui://app/<file> (assets.rs) relative to app.js. A failed load leaves the page
 * blank and unready, like a failed CSP check.
 */
function loadPage<T>(file: './settings-page.js' | './history-page.js'): Promise<T> {
  return (import(file) as Promise<T>).catch((err: unknown) => {
    document.title = 'page-load-failed';
    throw err;
  });
}

function start(): void {
  const root = document.getElementById('app');
  if (!root) return;
  // Without enforced CSP the page must not run: no ready is sent, so the host keeps the native chrome.
  if (!trustedTypesEnforced()) {
    document.title = 'csp-not-enforced';
    return;
  }
  // ?view=tabs selects the tab strip and ?view=rail the left rail; anything else, including no query, is the toolbar.
  const view = new URLSearchParams(location.search).get('view');
  if (view === 'tabs') {
    const stackRow = startTabs(root);
    // --- hover layer --- (and the second row of two-level stacks, T10b)
    for (const host of [root, stackRow]) {
      installTooltips(host);
      installHoverTargets(host, tabStripHoverTarget);
    }
    // --- end hover layer ---
  } else if (view === 'rail') {
    startRail(root);
    installTooltips(root); // --- hover layer ---
  }
  else if (view === 'panel') startPanel(root);
  else if (view === 'popup') startPopup(root);
  else if (view === 'bar') startBookmarkBar(root); // --- address bar, search and bookmarks ---
  else if (view === 'dialog') startDialog(root);
  else if (view === 'hover') startHover(root); // --- hover layer ---
  else if (view === 'split') startSplit(root); // --- split view ---
  else if (view === 'settings') void loadPage<{ startSettingsPage: typeof startSettingsPage }>('./settings-page.js').then((page) => page.startSettingsPage(root)); // bowser://settings (docs/design/settings-page.md)
  else if (view === 'history') void loadPage<{ startHistoryPage: typeof startHistoryPage }>('./history-page.js').then((page) => page.startHistoryPage(root)); // bowser://history (docs/design/history-page.md)
  else if (view === 'downloads') startDownloadsPage(root); // bowser://downloads (docs/design/downloads.md)
  else {
    startToolbar(root);
    installTooltips(root); // --- hover layer ---
  }
}

start();
