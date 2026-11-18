// Bowser fill script body. Prefixed by the host with `O` (the origin the user chose for), `M`
// (`login`, `generate` or `form`) and `V` (a Map of field kind to value), then by fields.js.
// Fills only recognised, visible, editable controls in the focused field's form or container.
const bwSetter = (e) => Object.getOwnPropertyDescriptor(Object.getPrototypeOf(e), 'value')?.set
  || Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
const bwFire = (e) => {
  e.dispatchEvent(new Event('input', { bubbles: true }));
  e.dispatchEvent(new Event('change', { bubbles: true }));
};
const bwNorm = (s) => String(s || '').toLowerCase().replace(/[^\p{L}\p{N}]+/gu, '');
// Picks the option of a <select> that means `value`: same value, same text, the same number
// (month "04" is option "4"), or a two-letter year matching "2028".
const bwChoose = (select, value) => {
  const want = bwNorm(value);
  const num = /^\d+$/.test(value) ? Number(value) : NaN;
  const options = [...select.options];
  const hit = options.find((o) => bwNorm(o.value) === want || bwNorm(o.text) === want)
    || (!Number.isNaN(num) && options.find((o) => Number(o.value) === num || Number(o.text) === num || Number(o.value) === num % 100 || Number(o.text) === num % 100))
    || (want.length >= 3 && options.find((o) => bwNorm(o.text).startsWith(want) || want.startsWith(bwNorm(o.text)) && bwNorm(o.text).length >= 3));
  if (!hit || hit.selected) return !!hit;
  select.value = hit.value;
  bwFire(select);
  return true;
};
const bwPut = (e, value) => {
  if (e.readOnly || e.disabled) return false;
  if (e instanceof HTMLSelectElement) return bwChoose(e, value);
  let v = value;
  const max = e.maxLength > 0 ? e.maxLength : 0;
  const kind = bwKind(e);
  if (kind === 'cc-exp-year' && max === 2) v = v.slice(-2);
  if (kind === 'cc-exp') {
    const hint = `${e.placeholder || ''} ${e.getAttribute('aria-label') || ''}`.toLowerCase();
    const [m, y] = v.split('/');
    if (/yyyy/.test(hint) || (max >= 7 && !/yy/.test(hint))) v = `${m}/20${y}`;
    if (max === 4 || /mmyy/.test(hint.replace(/\s/g, ''))) v = `${m}${y}`;
  }
  if (kind === 'cc-number' && max > 0 && max < v.length) v = v.slice(0, max);
  e.focus({ preventScroll: true });
  bwSetter(e).call(e, v);
  bwFire(e);
  return true;
};
const bwActive = document.activeElement instanceof Element && bwIsField(document.activeElement) ? document.activeElement : null;
let bwScopeEl = bwActive ? bwScope(bwActive) : null;
if (!bwScopeEl || !bwScopeEl.querySelector) {
  // Nothing focused (the shortcut or menu was used): the first form that has a field we can fill.
  bwScopeEl = [...document.querySelectorAll('form')].find((f) => [...f.elements].some((e) => bwIsField(e) && bwVis(e) && V.has(bwKind(e)))) || document.body;
}
const bwFields = [...bwScopeEl.querySelectorAll('input,select,textarea')].filter((e) => bwIsField(e) && bwVis(e));
let bwFilled = 0;
if (M === 'generate') {
  const fresh = bwFields.filter((e) => e.type === 'password' && bwKind(e) === 'new-password');
  const targets = fresh.length ? fresh : bwFields.filter((e) => e.type === 'password');
  for (const e of targets) if (bwPut(e, V.get('new-password'))) bwFilled++;
} else {
  // Card numbers split over several boxes take one chunk each.
  const numbers = bwFields.filter((e) => bwKind(e) === 'cc-number');
  if (numbers.length > 1 && V.has('cc-number')) {
    let rest = V.get('cc-number');
    for (const e of numbers) {
      const take = e.maxLength > 0 ? e.maxLength : rest.length;
      if (bwPut(e, rest.slice(0, take))) bwFilled++;
      rest = rest.slice(take);
    }
  }
  const has = (k) => bwFields.some((e) => bwKind(e) === k);
  for (const e of bwFields) {
    let kind = bwKind(e);
    if (kind === 'cc-number' && numbers.length > 1) continue;
    if (kind === 'new-password') continue;
    // A sign-in that asks for an email fills the saved username there.
    if (M === 'login' && kind === 'email' && !has('username')) kind = 'username';
    // Forms that ask for a full name only, or first and last names only.
    if (kind === 'name' && !V.has('name') && V.has('given-name')) { if (bwPut(e, [V.get('given-name'), V.get('family-name')].filter(Boolean).join(' '))) bwFilled++; continue; }
    if (!V.has(kind)) continue;
    if (e.value && e !== bwActive && M === 'form' && e.type !== 'select-one') continue;
    if (bwPut(e, V.get(kind))) bwFilled++;
  }
}
if (bwActive && bwActive.isConnected) bwActive.focus({ preventScroll: true });
return bwFilled > 0;
