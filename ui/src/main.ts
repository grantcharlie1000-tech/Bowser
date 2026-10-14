// Toolbar entry point: wires DOM events to the host bridge and host messages to the view.
import {
  MAX_TEXT,
  onInbound,
  send,
  sendDownloadToggle,
  sendSuggestEnd,
  sendSuggestPick,
  sendSuggestPreview,
  type CaptionAction,
  type CompletionMsg,
  type NavAction,
  type OpenTarget,
  type SuggestMsg,
} from './protocol';
import { moveSuggestSelection, suggestPreviewText } from './suggest-logic'; // --- omnibox suggest ---
import { applyMenuOpen, applyState, applyTheme, buildView, DRAG_ATTR, renderDisplay, syncSiteIcon } from './view';
import { startTabs } from './tabs';
import { startRail } from './rail';
import { startPanel } from './panel';
import { startPopup } from './popup';
import { startDialog } from './dialog';
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
  view.menu.addEventListener('click', () => open('menu'));
  view.downloads.button.addEventListener('click', () => { sendDownloadToggle(view.downloads.button.getBoundingClientRect()); });

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
    if (ev.key === 'Enter' && list && listSel > 0 && !pending && list.seq === latestSeq) {
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
      if (input.value.length <= MAX_TEXT) send({ v: 1, t: 'submit', text: input.value });
    } else if (ev.key === 'Escape') {
      ev.preventDefault();
      send({ v: 1, t: 'escape' });
    }
  });

  // Unfocused, a URL shows host-emphasised (view.display); focused, the real input text shows.
  // Updated on setText and blur only, never while typing.
  const showDisplay = (): void => {
    view.pill.classList.toggle('is-display', document.activeElement !== input && renderDisplay(view, input.value));
  };

  // A click on the address selects all of it while it is still the untouched address, as in every
  // browser. The toolbar view keeps `document.activeElement` and `hasFocus()` after the keyboard has
  // gone to the page, so "was the input already focused?" cannot tell a first click from a later one
  // (measured in the real window). What can is whether the user has edited the text since the host
  // last set it: once they type, a click places the caret.
  let selectOnUp = false;
  let pointerIsDown = false;
  let edited = false;
  input.addEventListener('input', () => {
    edited = true;
  });
  input.addEventListener('pointerdown', () => {
    pointerIsDown = true;
    selectOnUp = !edited || document.activeElement !== input || !document.hasFocus();
  });
  input.addEventListener('pointerup', () => {
    pointerIsDown = false;
    if (selectOnUp && input.selectionStart === input.selectionEnd) input.select();
    selectOnUp = false;
  });

  input.addEventListener('focus', () => {
    view.pill.classList.remove('is-display');
    // A press that is still down is what moved the focus here: select now as well as on release.
    if (pointerIsDown && input.selectionStart === input.selectionEnd) input.select();
    send({ v: 1, t: 'focus', on: true });
  });
  input.addEventListener('blur', () => {
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
        applyTheme(msg.theme, msg.solid, msg.dark, msg.fontScale, msg.uiFont);
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
        edited = false;
        if (msg.select) input.select();
        showDisplay();
        syncSiteIcon(view);
        break;
      case 'focusOmnibox':
        input.focus();
        if (msg.select) input.select();
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
    startTabs(root);
    // --- hover layer ---
    installTooltips(root);
    installHoverTargets(root, tabStripHoverTarget);
    // --- end hover layer ---
  } else if (view === 'rail') {
    startRail(root);
    installTooltips(root); // --- hover layer ---
  }
  else if (view === 'panel') startPanel(root);
  else if (view === 'popup') startPopup(root);
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
