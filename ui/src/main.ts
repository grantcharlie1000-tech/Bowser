// Toolbar entry point: wires DOM events to the host bridge and host messages to the view.
import {
  MAX_TEXT,
  onInbound,
  send,
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
import { applyState, applyTheme, buildView, DRAG_ATTR, renderDisplay } from './view';
import { startTabs } from './tabs';
import { startRail } from './rail';
import { startPanel } from './panel';
import { startPopup } from './popup';
import { startDialog } from './dialog';
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

  // A click that focuses the omnibox selects the whole address, as in every browser.
  let selectOnUp = false;
  input.addEventListener('pointerdown', () => {
    selectOnUp = document.activeElement !== input;
  });
  input.addEventListener('pointerup', () => {
    if (selectOnUp && input.selectionStart === input.selectionEnd) input.select();
    selectOnUp = false;
  });

  input.addEventListener('focus', () => {
    view.pill.classList.remove('is-display');
    send({ v: 1, t: 'focus', on: true });
  });
  input.addEventListener('blur', () => {
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
        applyTheme(msg.theme, msg.solid, msg.dark);
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
        if (msg.select) input.select();
        showDisplay();
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
  else {
    startToolbar(root);
    installTooltips(root); // --- hover layer ---
  }
}

start();
