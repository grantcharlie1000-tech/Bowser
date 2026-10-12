// Toolbar entry point: wires DOM events to the host bridge and host messages to the view.
import { MAX_TEXT, onInbound, send, type CaptionAction, type CompletionMsg, type NavAction, type OpenTarget } from './protocol';
import { applyState, applyTheme, buildView, DRAG_ATTR } from './view';

/** True only when the event target is the drag surface itself, never a control inside it. */
function onDragSurface(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && target.hasAttribute(DRAG_ATTR);
}

function start(): void {
  const root = document.getElementById('app');
  if (!root) return;

  const view = buildView(root);
  const input = view.input;

  let nextSeq = 0;
  let latestSeq = -1;
  let latestText = '';
  let lastInputType = '';
  let pending = false;
  let frame = 0;
  let loading = false;

  // At most one edit per animation frame, each with a fresh seq.
  const flush = (): void => {
    frame = 0;
    if (!pending) return;
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
  view.newTab.addEventListener('click', () => open('newtab'));
  view.menu.addEventListener('click', () => open('menu'));

  input.addEventListener('input', (ev) => {
    lastInputType = ev instanceof InputEvent ? ev.inputType : '';
    pending = true;
    if (frame === 0) frame = requestAnimationFrame(flush);
  });

  input.addEventListener('keydown', (ev) => {
    if (ev.isComposing || ev.keyCode === 229) return;
    if (ev.key === 'Enter') {
      ev.preventDefault();
      if (input.value.length <= MAX_TEXT) send({ v: 1, t: 'submit', text: input.value });
    } else if (ev.key === 'Escape') {
      ev.preventDefault();
      send({ v: 1, t: 'escape' });
    }
  });

  input.addEventListener('focus', () => {
    send({ v: 1, t: 'focus', on: true });
  });
  input.addEventListener('blur', () => {
    send({ v: 1, t: 'focus', on: false });
  });

  // Inline completion is applied only while it still describes exactly what the user has typed.
  const acceptCompletion = (msg: CompletionMsg): void => {
    if (msg.suffix === '' || msg.seq !== latestSeq || pending) return;
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
        input.value = msg.text;
        if (msg.select) input.select();
        break;
      case 'focusOmnibox':
        input.focus();
        if (msg.select) input.select();
        break;
      case 'completion':
        acceptCompletion(msg);
        break;
    }
  });

  // Registered before ready so no state message can be missed.
  send({ v: 1, t: 'ready' });
}

start();
