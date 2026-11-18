// Bowser autofill detector. Runs in the top document of normal tabs whose profile saves sign-ins.
// It never reads Bowser's saved data: it only tells Bowser (1) that the user asked to fill the
// focused field, with where that field is, and (2) what a sign-in or checkout form submitted, so
// Bowser can offer to save it. Everything it shows lives in a closed shadow root.
const bwPost = (m) => { try { window.chrome.webview.postMessage(m); } catch {} };
const BW_NS = 'http://www.w3.org/2000/svg';
// One family with Bowser's chrome icons: 24-unit grid, 1.5 stroke, round caps and joins.
const BW_GLYPHS = {
  login: [['circle', { cx: '7.5', cy: '11', r: '4' }], ['path', { d: 'M11.5 11H21v3M17 11v3' }]],
  new: [['path', { d: 'M10 3.5c.6 3.6 2.4 5.4 6 6-3.6.6-5.4 2.4-6 6-.6-3.6-2.4-5.4-6-6 3.6-.6 5.4-2.4 6-6z' }], ['path', { d: 'M18 14.5c.3 1.8 1.2 2.7 3 3-1.8.3-2.7 1.2-3 3-.3-1.8-1.2-2.7-3-3 1.8-.3 2.7-1.2 3-3z' }]],
  identity: [['circle', { cx: '12', cy: '7', r: '3.5' }], ['path', { d: 'M4.5 20v-2a7.5 7.5 0 0 1 15 0v2' }]],
  card: [['rect', { x: '2.5', y: '5', width: '19', height: '14', rx: '2' }], ['path', { d: 'M2.5 9.5h19M6 15h4' }]],
};
const BW_LABELS = { login: 'Fill saved sign-in', new: 'Suggest a strong password', identity: 'Fill saved address', card: 'Fill saved card' };
const BW_CSS = `:host{all:initial}
button{all:unset;box-sizing:border-box;display:grid;place-items:center;width:22px;height:22px;border-radius:6px;cursor:pointer;
color:#4b5262;background:rgba(255,255,255,.94);box-shadow:0 0 0 1px rgba(15,23,42,.10),0 1px 3px rgba(15,23,42,.14);
transition:transform 167ms cubic-bezier(0,0,0,1),color 83ms linear,background-color 83ms linear;animation:bw-in 167ms cubic-bezier(0,0,0,1)}
button:hover{color:#0b57d0;background:#fff;transform:scale(1.08)}
button:active{transform:scale(.92);transition-duration:83ms}
button:focus-visible{outline:2px solid #0b57d0;outline-offset:1px}
svg{width:14px;height:14px;fill:none;stroke:currentColor;stroke-width:1.5;stroke-linecap:round;stroke-linejoin:round;overflow:visible}
button[data-group=new] svg path{transform-box:fill-box;transform-origin:center;transition:transform 250ms cubic-bezier(.34,1.56,.64,1)}
button[data-group=new]:hover svg path:first-child{transform:rotate(90deg) scale(1.08)}
button[data-group=new]:hover svg path:last-child{transform:scale(1.25)}
@keyframes bw-in{from{opacity:0;transform:scale(.7)}to{opacity:1;transform:none}}
@media (prefers-color-scheme:dark){button{color:#c8ccd6;background:rgba(36,38,44,.96);box-shadow:0 0 0 1px rgba(255,255,255,.12),0 1px 3px rgba(0,0,0,.5)}button:hover{color:#a8c7fa;background:#2b2e35}button:focus-visible{outline-color:#a8c7fa}}
@media (prefers-reduced-motion:reduce){button,button svg path{animation:none!important;transition:opacity 83ms linear!important}button:hover,button:active{transform:none}}`;

let bwHost = null;
let bwButton = null;
let bwField = null;
let bwFieldKind = '';
let bwFieldGroup = '';
let bwFrame = 0;
let bwHideTimer = 0;
let bwFormSeen = false;
let bwLastUser = '';
let bwLastSent = 0;
let bwTypedPassword = null;
// Selects the user (or a Bowser fill) changed: an untouched select still shows its default.
const bwChanged = new WeakSet();

const bwSetStyle = (el, props) => { for (const [k, v] of Object.entries(props)) el.style.setProperty(k, v, 'important'); };
const bwGlyph = (group) => {
  const svg = document.createElementNS(BW_NS, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('aria-hidden', 'true');
  for (const [tag, attrs] of BW_GLYPHS[group] || BW_GLYPHS.login) {
    const node = document.createElementNS(BW_NS, tag);
    for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, v);
    svg.append(node);
  }
  return svg;
};
const bwMake = () => {
  bwHost = document.createElement('bowser-autofill');
  bwSetStyle(bwHost, { all: 'initial', position: 'fixed', 'z-index': '2147483647', left: '0px', top: '0px', width: '22px', height: '22px', display: 'none', margin: '0', padding: '0', border: '0', transform: 'none', 'pointer-events': 'auto' });
  const root = bwHost.attachShadow({ mode: 'closed' });
  bwButton = document.createElement('button');
  bwButton.type = 'button';
  bwButton.tabIndex = -1;
  try {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(BW_CSS);
    root.adoptedStyleSheets = [sheet];
  } catch {
    bwSetStyle(bwButton, { display: 'grid', 'place-items': 'center', width: '22px', height: '22px', 'border-radius': '6px', background: '#fff', color: '#4b5262', cursor: 'pointer', border: '0', 'box-shadow': '0 0 0 1px rgba(15,23,42,.1)' });
  }
  // Keep focus (and the caret) in the field: the press must not blur it.
  bwButton.addEventListener('pointerdown', (e) => { e.preventDefault(); e.stopPropagation(); }, true);
  bwButton.addEventListener('mousedown', (e) => { e.preventDefault(); e.stopPropagation(); }, true);
  bwButton.addEventListener('click', (e) => { e.preventDefault(); e.stopPropagation(); bwRequest(); }, true);
  root.append(bwButton);
};
const bwPlace = () => {
  bwFrame = 0;
  if (!bwHost || !bwField || !bwField.isConnected || !bwVis(bwField)) { bwHide(); return; }
  const r = bwField.getBoundingClientRect();
  if (r.bottom < 0 || r.top > innerHeight || r.right < 0 || r.left > innerWidth) { bwHost.style.setProperty('display', 'none', 'important'); return; }
  const inside = r.width >= 72;
  const pad = Math.min(8, Math.max(4, (r.height - 22) / 2));
  const left = inside ? r.right - 22 - pad : r.right + 4;
  const top = r.top + (r.height - 22) / 2;
  bwSetStyle(bwHost, { left: `${Math.round(left)}px`, top: `${Math.round(top)}px`, display: 'block' });
};
const bwSchedule = () => { if (!bwFrame && bwField) bwFrame = requestAnimationFrame(bwPlace); };
const bwShow = (field, kind, group) => {
  clearTimeout(bwHideTimer);
  if (!bwHost) bwMake();
  if (!bwHost.isConnected) (document.body || document.documentElement).append(bwHost);
  bwField = field;
  bwFieldKind = kind;
  if (bwFieldGroup !== group) {
    bwFieldGroup = group;
    bwButton.dataset.group = group;
    bwButton.replaceChildren(bwGlyph(group));
    bwButton.setAttribute('aria-label', BW_LABELS[group] || 'Autofill');
    bwButton.title = `${BW_LABELS[group] || 'Autofill'} (Alt+Down)`;
  }
  bwPlace();
};
const bwHide = () => {
  bwField = null;
  if (bwHost) bwHost.style.setProperty('display', 'none', 'important');
};
// Asks Bowser to show its fill menu under the focused field. Bowser decides what to offer.
const bwRequest = () => {
  if (!bwField) return;
  const r = bwField.getBoundingClientRect();
  const field = bwFieldKind === 'email' && bwFieldGroup === 'login' ? 'username' : bwFieldKind;
  bwPost({ kind: 'autofill-request', field, x: Math.round(r.left), y: Math.round(r.top), w: Math.round(r.width), h: Math.round(r.height) });
};

document.addEventListener('focusin', (e) => {
  const t = e.target;
  if (!(t instanceof Element) || !bwIsField(t) || t instanceof HTMLTextAreaElement) return;
  const kind = bwKind(t);
  const group = bwGroup(t, kind);
  if (kind === 'current-password' || kind === 'new-password') {
    if (!bwFormSeen && bwVis(t)) { bwFormSeen = true; bwPost({ kind: 'login-form-seen' }); }
  }
  if (!group || kind === 'cc-csc' || !bwVis(t)) { bwHide(); return; }
  bwShow(t, kind, group);
}, true);
document.addEventListener('focusout', (e) => {
  if (e.target !== bwField) return;
  clearTimeout(bwHideTimer);
  bwHideTimer = setTimeout(() => { if (document.activeElement !== bwField) bwHide(); }, 180);
}, true);
addEventListener('scroll', bwSchedule, { capture: true, passive: true });
addEventListener('resize', bwSchedule, { passive: true });
document.addEventListener('keydown', (e) => {
  const t = e.target;
  if (t !== bwField) return;
  const plainDown = e.key === 'ArrowDown' && !e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey && !t.value
    && !t.getAttribute('aria-autocomplete') && t.getAttribute('role') !== 'combobox' && !t.getAttribute('list');
  if ((e.key === 'ArrowDown' && e.altKey) || plainDown) { e.preventDefault(); bwRequest(); return; }
  if (e.key === 'Escape') bwHide();
}, true);

// ----- saving what was typed -----
document.addEventListener('change', (e) => { if (e.target instanceof HTMLSelectElement) bwChanged.add(e.target); }, true);
document.addEventListener('input', (e) => {
  const t = e.target;
  if (!(t instanceof HTMLInputElement)) return;
  if (t.type === 'password') { bwTypedPassword = t; return; }
  const kind = bwKind(t);
  // Multi-step sign-ins ask for the username on one screen and the password on the next.
  if ((kind === 'username' || kind === 'email') && t.value.trim()) bwLastUser = t.value.trim();
  if (bwField === t) bwSchedule();
}, true);

const bwValueOf = (e) => e instanceof HTMLSelectElement ? ((e.selectedOptions[0] && e.selectedOptions[0].text) || e.value) : e.value;
const bwCapture = (scope) => {
  const now = Date.now();
  if (now - bwLastSent < 1200 || !scope) return;
  const fields = [...scope.querySelectorAll('input,select,textarea')].filter((e) => bwIsField(e) && bwVis(e));
  const passwords = fields.filter((e) => e instanceof HTMLInputElement && e.type === 'password' && e.value);
  let sent = false;
  if (passwords.length) {
    const fresh = passwords.filter((e) => bwKind(e) === 'new-password');
    const pick = (fresh.length ? fresh : passwords).at(-1);
    const before = fields.filter((e) => {
      const kind = bwKind(e);
      return (kind === 'username' || kind === 'email') && e.value.trim() && (e.compareDocumentPosition(pick) & Node.DOCUMENT_POSITION_FOLLOWING);
    }).at(-1);
    const user = before ? before.value.trim() : bwLastUser;
    bwPost({ kind: 'login-submitted', user: user.slice(0, 1024), pass: pick.value });
    sent = true;
  }
  const values = {};
  for (const e of fields) {
    const kind = bwKind(e);
    if (!kind || kind === 'username' || kind === 'cc-csc' || kind.endsWith('password')) continue;
    if (kind === 'email' && passwords.length) continue;
    const value = String(bwValueOf(e) || '').trim().slice(0, 200);
    if (!value) continue;
    // A card number split over several boxes is one number.
    if (kind === 'cc-number' && values[kind]) { values[kind] = (values[kind] + value).slice(0, 40); continue; }
    if (e instanceof HTMLSelectElement && (!bwChanged.has(e) || !e.value)) continue;
    if (Object.prototype.hasOwnProperty.call(values, kind)) continue;
    values[kind] = value;
  }
  const keys = Object.keys(values);
  const card = (values['cc-number'] || '').replace(/\D/g, '').length >= 12;
  if (card || keys.filter((k) => !k.startsWith('cc-')).length >= 3) {
    bwPost({ kind: 'autofill-submitted', f: values });
    sent = true;
  }
  if (sent) { bwLastSent = now; bwTypedPassword = null; }
};
const BW_SUBMIT_TEXT = /log.?in|sign.?in|sign.?up|signin|signup|continue|next|submit|register|create|save|update|pay|place.?order|checkout|confirm|buy|order|anmelden|weiter|connexion|continuer|entrar|iniciar|acceder|accedi/;
document.addEventListener('submit', (e) => { if (e.target instanceof HTMLFormElement) bwCapture(e.target); }, true);
document.addEventListener('click', (e) => {
  const t = e.target instanceof Element ? e.target.closest('button,input[type=submit],input[type=button],input[type=image],[role=button],a') : null;
  if (!t) return;
  const label = `${t.textContent || ''} ${t.getAttribute('value') || ''} ${t.getAttribute('aria-label') || ''} ${t.id || ''}`.toLowerCase().slice(0, 200);
  const submitType = (t instanceof HTMLButtonElement && t.type === 'submit') || (t instanceof HTMLInputElement && (t.type === 'submit' || t.type === 'image'));
  if (!submitType && !BW_SUBMIT_TEXT.test(label)) return;
  bwCapture(bwScope(t));
}, true);
document.addEventListener('keydown', (e) => {
  if (e.key !== 'Enter' || e.isComposing || !(e.target instanceof HTMLInputElement) || !bwIsField(e.target)) return;
  const scope = bwScope(e.target);
  if (bwPasswords(scope).some((p) => p.value)) bwCapture(scope);
}, true);
// A sign-in that navigates away by script, without a submit or a recognisable button.
addEventListener('pagehide', () => {
  if (bwTypedPassword && bwTypedPassword.isConnected && bwTypedPassword.value) bwCapture(bwScope(bwTypedPassword));
}, true);
