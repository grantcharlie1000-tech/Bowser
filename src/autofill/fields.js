// Bowser field classifier, shared by the autofill detector (page.js) and every fill script
// (fill.js). It names a form control by the HTML autocomplete token it stands for, or '' when
// Bowser should leave it alone. Order of evidence: the page's own autocomplete attribute, the
// password type, then the control's name, id, label, placeholder and ARIA text.
const BW_AC = new Map([
  ['username', 'username'], ['email', 'email'], ['current-password', 'current-password'], ['new-password', 'new-password'],
  ['given-name', 'given-name'], ['family-name', 'family-name'], ['name', 'name'], ['honorific-prefix', ''],
  ['additional-name', ''], ['nickname', ''], ['organization', 'organization'], ['organization-title', ''],
  ['tel', 'tel'], ['tel-national', 'tel'], ['tel-local', 'tel'], ['street-address', 'address-line1'],
  ['address-line1', 'address-line1'], ['address-line2', 'address-line2'], ['address-line3', 'address-line2'],
  ['address-level2', 'address-level2'], ['address-level1', 'address-level1'], ['postal-code', 'postal-code'],
  ['country', 'country'], ['country-name', 'country'], ['cc-name', 'cc-name'], ['cc-given-name', 'cc-name'],
  ['cc-number', 'cc-number'], ['cc-exp', 'cc-exp'], ['cc-exp-month', 'cc-exp-month'], ['cc-exp-year', 'cc-exp-year'],
  ['cc-csc', 'cc-csc'], ['one-time-code', ''], ['bday', ''], ['bday-day', ''], ['bday-month', ''], ['bday-year', ''],
  ['sex', ''], ['url', ''], ['photo', ''], ['transaction-amount', ''], ['language', ''],
]);
// The first rule whose pattern matches the control's text wins; '' means "never fill this".
const BW_RULES = [
  [/one.?time|\botp\b|totp|\b2fa\b|\bmfa\b|verification.?code|auth\w*.?code|security.?(?:question|answer)|captcha|coupon|promo|voucher|gift.?card|search|query|\bq\b|filter/, ''],
  [/\bc(?:vc|vv|sc|vn)\d?\b|cvc|cvv|security.?code|card.?verif|card.?code|\bcid\b/, 'cc-csc'],
  [/card.?(?:num|no\b|nr|#)|cc.?(?:num|no\b)|credit.?card(?!.*(?:name|holder|exp))|debit.?card(?!.*(?:name|holder|exp))|kartennummer|n[uú]mero.?de.?tarjeta|num[eé]ro.?de.?carte|\bpan\b/, 'cc-number'],
  [/(?:card|cc).?(?:holder|owner|name)|name.?on.?(?:the.?)?card|nameoncard|cardholder|titular/, 'cc-name'],
  [/\bmm\s*\/?\s*yy|exp\w*.?date|valid.?(?:thru|through|until)/, 'cc-exp'],
  [/(?:exp|expir\w*|valid).{0,12}(?:month|\bmon\b|\bmm\b)|cc.?month|card.?month|exp.?m\b/, 'cc-exp-month'],
  [/(?:exp|expir\w*|valid).{0,12}(?:year|\byr\b|\byy)|cc.?year|card.?year|exp.?y\b/, 'cc-exp-year'],
  [/expir|exp.?date|\bexp\b|valid.?(?:thru|through|until)|mm.?\/.?yy/, 'cc-exp'],
  [/e.?mail|courriel|correo|\bmail\b/, 'email'],
  [/user.?name|user.?id|login.?(?:name|id)|\blogin\b|benutzer|identifiant|usuario|\buser\b|account.?(?:name|id)|member.?id|customer.?(?:id|number)/, 'username'],
  [/phone|mobile|\btel\b|telephone|cell|handy|t[eé]l[eé]phone|tel[eé]fono|whats.?app/, 'tel'],
  [/first.?name|given.?name|\bf.?name\b|forename|vorname|pr[eé]nom|nombre(?!.*apellido)/, 'given-name'],
  [/last.?name|sur.?name|family.?name|\bl.?name\b|nachname|apellido|nom.?de.?famille/, 'family-name'],
  [/company|organi[sz]ation|business.?name|\bfirma\b|empresa|soci[eé]t[eé]/, 'organization'],
  [/(?:address|addr|street).?(?:line)?.?[23]|apartment|\bapt\b|\bsuite\b|\bunit\b|\bflat\b|building|address.?2|adresszusatz/, 'address-line2'],
  [/(?:address|addr|street).?(?:line)?.?1|street|address|\baddr\b|line.?1|stra(?:ss|ß)e|calle|direcci[oó]n|adresse|house.?(?:number|no)/, 'address-line1'],
  [/city|town|suburb|locality|\bort\b|ville|ciudad|stadt|municipality/, 'address-level2'],
  [/\bstate\b|province|county|region|prefecture|bundesland|provincia|territory/, 'address-level1'],
  [/\bzip|postal|post.?code|postcode|\bplz\b|c[oó]digo.?postal|code.?postal|pin.?code|eircode/, 'postal-code'],
  [/country|nation\b|\bland\b|\bpa[ií]s\b|\bpays\b/, 'country'],
  [/full.?name|your.?name|\bname\b|\bnom\b|\bnombre\b/, 'name'],
];
const BW_FILLABLE_TYPES = new Set(['', 'text', 'email', 'tel', 'password', 'number']);
const bwVis = (e) => {
  if (!e || e.disabled || e.hidden || e.type === 'hidden') return false;
  const r = e.getBoundingClientRect();
  if (r.width < 2 || r.height < 2) return false;
  const s = getComputedStyle(e);
  return s.display !== 'none' && s.visibility !== 'hidden' && Number(s.opacity) > 0.05;
};
const bwText = (e) => {
  const parts = [e.getAttribute('name'), e.id, e.getAttribute('placeholder'), e.getAttribute('aria-label'), e.getAttribute('title'), e.getAttribute('data-testid')];
  try { for (const l of e.labels || []) parts.push(l.textContent); } catch {}
  const by = e.getAttribute('aria-labelledby');
  if (by) for (const id of by.split(/\s+/)) { const n = document.getElementById(id); if (n) parts.push(n.textContent); }
  if (!(e.labels && e.labels.length)) { const l = e.closest('label'); if (l) parts.push(l.textContent); }
  return parts.filter(Boolean).join(' ').replace(/[_\-.]+/g, ' ').toLowerCase().slice(0, 400);
};
const bwIsField = (e) => (e instanceof HTMLInputElement && BW_FILLABLE_TYPES.has((e.type || '').toLowerCase()))
  || e instanceof HTMLSelectElement || e instanceof HTMLTextAreaElement;
// The form a control belongs to, or the closest container that acts like one.
const bwScope = (e) => (e && e.form) || (e && e.closest && e.closest('form,[role=form],fieldset,dialog,[role=dialog],main,section,article')) || document.body || document.documentElement;
const bwPasswords = (scope) => [...scope.querySelectorAll('input[type=password]')].filter(bwVis);
const bwIsNew = (e, text) => {
  if (/new|confirm|repeat|retype|re.?enter|again|verify|create|choose|set.?password|sign.?up|register|join|neu|nouveau|nueva/.test(text)) return true;
  const scope = bwScope(e);
  if (bwPasswords(scope).length >= 2) return true;
  const formText = [scope.getAttribute && scope.getAttribute('action'), scope.id, scope.getAttribute && scope.getAttribute('name'), scope.className && String(scope.className)].filter(Boolean).join(' ').toLowerCase();
  return /sign.?up|register|registration|join|create.?account|new.?account|change.?password|reset.?password/.test(formText);
};
const bwKind = (e) => {
  if (!bwIsField(e)) return '';
  const type = (e.type || '').toLowerCase();
  const tokens = (e.getAttribute('autocomplete') || '').toLowerCase().trim().split(/\s+/);
  for (let i = tokens.length - 1; i >= 0; i--) if (BW_AC.has(tokens[i])) return BW_AC.get(tokens[i]);
  const text = bwText(e);
  if (type === 'password') {
    if (/one.?time|\botp\b|\b2fa\b|\bpin\b|verification.?code|security.?code|cvc|cvv/.test(text)) return '';
    return bwIsNew(e, text) ? 'new-password' : 'current-password';
  }
  for (const [pattern, kind] of BW_RULES) {
    if (!pattern.test(text)) continue;
    if (kind === '') return '';
    if (e instanceof HTMLTextAreaElement && kind !== 'address-line1') continue;
    if (type === 'number' && !/^cc-|postal-code|tel/.test(kind)) continue;
    return kind;
  }
  if (type === 'email') return 'email';
  if (type === 'tel') return 'tel';
  // A plain text box just before a password field is the username, whatever it is called.
  if ((type === 'text' || type === '') && e instanceof HTMLInputElement) {
    const after = bwPasswords(bwScope(e)).some((p) => e.compareDocumentPosition(p) & Node.DOCUMENT_POSITION_FOLLOWING);
    if (after) return 'username';
  }
  return '';
};
// What Bowser offers for a field: a login, an identity (name, address, contact) or a card.
const bwGroup = (e, kind) => {
  if (!kind) return '';
  if (kind === 'username' || kind === 'current-password') return 'login';
  if (kind === 'new-password') return 'new';
  if (kind.startsWith('cc-')) return 'card';
  if (kind === 'email' && bwPasswords(bwScope(e)).length) return 'login';
  return 'identity';
};
