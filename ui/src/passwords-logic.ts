// Words, checks and commands for the settings page's "Passwords and autofill" section. Pure: no DOM,
// so ui/test/passwords.mjs runs it directly. Rust's side of the wire is
// src/chrome_host/passwords_page.rs; the shared fixture is ui/test/fixtures/passwords-snapshot.json.
import type { PanelFields, SnapshotMsg } from './protocol';

export interface Login { readonly id: number; readonly host: string; readonly origin: string; readonly user: string; readonly score: number; readonly reused: boolean }
export interface Address {
  readonly id: number; readonly name: string; readonly organization: string; readonly email: string; readonly tel: string;
  readonly line1: string; readonly line2: string; readonly city: string; readonly region: string; readonly postal: string; readonly country: string;
}
export interface SavedCard { readonly id: number; readonly name: string; readonly brand: string; readonly last4: string; readonly month: number; readonly year: number; readonly nick: string }
export type ImportState =
  | { readonly stage: 'preview'; readonly provider: string; readonly fresh: number; readonly dup: number; readonly conflict: number; readonly bad: number }
  | { readonly stage: 'done'; readonly added: number; readonly skipped: number; readonly failed: number };
export type Ack = 'none' | 'saved' | 'deleted' | 'copied' | 'imported' | 'exported' | 'failed';
export interface Prefs { readonly save: boolean; readonly addresses: boolean; readonly cards: boolean }

export interface PasswordsSnapshot {
  readonly available: boolean;
  readonly guest: boolean;
  readonly prefs: Prefs;
  readonly total: number;
  readonly query: string;
  readonly logins: readonly Login[];
  readonly never: readonly string[];
  readonly addresses: readonly Address[];
  readonly cards: readonly SavedCard[];
  readonly importState: ImportState | null;
  readonly ack: Ack;
  readonly error: string;
  readonly reveal: { readonly id: number; readonly value: string } | null;
  readonly generated: string | null;
}

export const ADDRESS_KEYS = ['name', 'organization', 'email', 'tel', 'line1', 'line2', 'city', 'region', 'postal', 'country'] as const;
export type AddressKey = typeof ADDRESS_KEYS[number];
export const ADDRESS_LABELS: Readonly<Record<AddressKey, string>> = {
  name: 'Full name', organization: 'Company', email: 'Email', tel: 'Phone', line1: 'Street address', line2: 'Flat, suite or unit',
  city: 'Town or city', region: 'County, state or region', postal: 'Postcode or ZIP', country: 'Country',
};
const ACKS: readonly Ack[] = ['none', 'saved', 'deleted', 'copied', 'imported', 'exported', 'failed'];

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const str = (v: unknown): string => (typeof v === 'string' ? v : '');
const num = (v: unknown): number => (typeof v === 'number' && Number.isFinite(v) ? v : 0);

/** Reads a `passwords` snapshot, or null when it is not one. Unknown or malformed rows are dropped. */
export function parsePasswordsSnapshot(message: SnapshotMsg): PasswordsSnapshot | null {
  if (message.p !== 'passwords') return null;
  const d = message.d as Record<string, unknown>;
  if (typeof d.available !== 'boolean' || !Array.isArray(d.l) || !isObj(d.prefs)) return null;
  const rows = (v: unknown): unknown[][] => (Array.isArray(v) ? v.filter((r): r is unknown[] => Array.isArray(r)) : []);
  const logins = rows(d.l).filter((r) => r.length === 6).map((r): Login => ({
    id: num(r[0]), host: str(r[1]), origin: str(r[2]), user: str(r[3]), score: Math.max(0, Math.min(4, num(r[4]))), reused: r[5] === true,
  }));
  const addresses = rows(d.addr).filter((r) => r.length === 11).map((r): Address => {
    const [name, organization, email, tel, line1, line2, city, region, postal, country] = r.slice(1).map(str) as [string, string, string, string, string, string, string, string, string, string];
    return { id: num(r[0]), name, organization, email, tel, line1, line2, city, region, postal, country };
  });
  const cards = rows(d.cards).filter((r) => r.length === 7).map((r): SavedCard => ({
    id: num(r[0]), name: str(r[1]), brand: str(r[2]), last4: str(r[3]), month: num(r[4]), year: num(r[5]), nick: str(r[6]),
  }));
  let importState: ImportState | null = null;
  if (isObj(d.import)) {
    const i = d.import;
    if (i.stage === 'preview') importState = { stage: 'preview', provider: str(i.provider), fresh: num(i.new), dup: num(i.dup), conflict: num(i.conflict), bad: num(i.bad) };
    else if (i.stage === 'done') importState = { stage: 'done', added: num(i.added), skipped: num(i.skipped), failed: num(i.failed) };
  }
  const prefs = d.prefs;
  const ack = ACKS.includes(d.ack as Ack) ? (d.ack as Ack) : 'none';
  const reveal = isObj(d.reveal) && typeof d.reveal.value === 'string' ? { id: num(d.reveal.id), value: d.reveal.value } : null;
  return {
    available: d.available, guest: d.guest === true,
    prefs: { save: prefs.save !== false, addresses: prefs.addresses !== false, cards: prefs.cards !== false },
    total: num(d.total), query: str(d.q), logins,
    never: Array.isArray(d.never) ? d.never.filter((s): s is string => typeof s === 'string') : [],
    addresses, cards, importState, ack, error: str(d.error), reveal,
    generated: typeof d.generated === 'string' ? d.generated : null,
  };
}

// ---- commands (the page's `set` family; Rust's map_command takes exactly these fields) ----

export const cmd = {
  refresh: (): PanelFields => ({ a: 'pw-refresh' }),
  search: (text: string): PanelFields => ({ a: 'pw-search', text: text.slice(0, 200) }),
  reveal: (id: number): PanelFields => ({ a: 'pw-reveal', id }),
  copy: (id: number, password: boolean): PanelFields => ({ a: 'pw-copy', id, n: password ? 1 : 0 }),
  remove: (id: number): PanelFields => ({ a: 'pw-delete', id }),
  generate: (options: GeneratorOptions): PanelFields => ({ a: 'pw-generate', text: generatorText(options) }),
  copyGenerated: (): PanelFields => ({ a: 'pw-copy-generated' }),
  importFile: (): PanelFields => ({ a: 'pw-import' }),
  importConfirm: (): PanelFields => ({ a: 'pw-import-confirm' }),
  importCancel: (): PanelFields => ({ a: 'pw-import-cancel' }),
  exportFile: (): PanelFields => ({ a: 'pw-export' }),
  forgetNever: (origin: string): PanelFields => ({ a: 'pw-never-remove', text: origin }),
  pref: (key: keyof Prefs, on: boolean): PanelFields => ({ a: 'pw-pref', text: key, on }),
  deleteAddress: (id: number): PanelFields => ({ a: 'af-address-delete', id }),
  deleteCard: (id: number): PanelFields => ({ a: 'af-card-delete', id }),
};

/** Saves a login: a new one without `id`. The password goes last, so it may hold line breaks. */
export function saveLogin(id: number | null, site: string, user: string, password: string): PanelFields | null {
  const s = site.trim();
  const u = user.trim();
  if (!s || !u || !password || /[\n\r]/.test(s + u)) return null;
  const text = `${s}\n${u}\n${password}`;
  if (text.length > 4096) return null;
  return id === null ? { a: 'pw-save', text } : { a: 'pw-save', id, text };
}

export function saveAddress(id: number | null, values: Partial<Record<AddressKey, string>>): PanelFields | null {
  const lines = ADDRESS_KEYS.map((k) => [k, (values[k] ?? '').replace(/[\r\n=]+/g, ' ').replace(/\s+/g, ' ').trim()] as const).filter(([, v]) => v !== '').map(([k, v]) => `${k}=${v}`);
  if (lines.length === 0) return null;
  const text = lines.join('\n');
  return id === null ? { a: 'af-address-save', text } : { a: 'af-address-save', id, text };
}

/** Saves a card. An edit may leave `number` blank to keep the saved number. */
export function saveCard(id: number | null, card: { name: string; number: string; month: string; year: string; nick: string }): PanelFields | null {
  const digits = card.number.replace(/\D/g, '');
  if (id === null && !luhnValid(digits)) return null;
  if (digits !== '' && !luhnValid(digits)) return null;
  const clean = (s: string): string => s.replace(/[\r\n=]+/g, ' ').replace(/\s+/g, ' ').trim();
  const text = [`name=${clean(card.name)}`, `number=${digits}`, `month=${clean(card.month)}`, `year=${clean(card.year)}`, `nick=${clean(card.nick)}`].join('\n');
  return id === null ? { a: 'af-card-save', text } : { a: 'af-card-save', id, text };
}

// ---- generator ----

export interface GeneratorOptions { length: number; lower: boolean; upper: boolean; digits: boolean; symbols: boolean; avoidAmbiguous: boolean }
export const DEFAULT_GENERATOR: Readonly<GeneratorOptions> = { length: 20, lower: true, upper: true, digits: true, symbols: true, avoidAmbiguous: true };
export const GENERATOR_MIN = 8;
export const GENERATOR_MAX = 64;

/** The compact form Rust parses (`GeneratorOptions::parse`): `20:luds:a`. */
export function generatorText(o: GeneratorOptions): string {
  const classes = `${o.lower ? 'l' : ''}${o.upper ? 'u' : ''}${o.digits ? 'd' : ''}${o.symbols ? 's' : ''}` || 'l';
  const length = Math.max(GENERATOR_MIN, Math.min(GENERATOR_MAX, Math.round(o.length)));
  return `${length}:${classes}${o.avoidAmbiguous ? ':a' : ''}`;
}

/** Which kind each character is, for the generator's coloured output. */
export function charKinds(password: string): Array<'lower' | 'upper' | 'digit' | 'symbol'> {
  return [...password].map((c) => (/[a-z]/.test(c) ? 'lower' : /[A-Z]/.test(c) ? 'upper' : /[0-9]/.test(c) ? 'digit' : 'symbol'));
}

// ---- strength (the same scale and rules as src/passwords/generator.rs, for live feedback) ----

const COMMON = ['password', 'passw0rd', '123456', '12345678', 'qwerty', 'abc123', 'letmein', 'welcome', 'monkey', 'dragon', 'football',
  'baseball', 'iloveyou', 'admin', 'login', 'master', 'sunshine', 'princess', 'shadow', 'trustno1', 'superman', 'michael', 'starwars',
  'whatever', 'freedom', 'hello', 'secret', 'charlie', 'computer', 'qwertyuiop', 'asdfgh', 'zxcvbn', 'changeme', 'default', 'summer',
  'winter', 'spring', 'autumn', 'pokemon', 'batman', 'liverpool', 'chelsea', 'arsenal'];
const ROWS = ['qwertyuiop', 'asdfghjkl', 'zxcvbnm', '1234567890'];
const LEET: Readonly<Record<string, string>> = { 0: 'o', 1: 'i', '!': 'i', 3: 'e', 4: 'a', '@': 'a', 5: 's', $: 's', 7: 't' };

export function strength(password: string): number {
  const chars = [...password];
  if (chars.length === 0) return 0;
  const lower = password.toLowerCase();
  const stem = lower.replace(/[\d\p{P}\p{S}]+$/u, '');
  const leet = [...stem].map((c) => LEET[c] ?? c).join('');
  if (chars.length < 6 || COMMON.some((w) => lower === w || stem === w || leet === w)) return 0;
  let pool = 0;
  if (/[a-z]/.test(password)) pool += 26;
  if (/[A-Z]/.test(password)) pool += 26;
  if (/[0-9]/.test(password)) pool += 10;
  if (/[\x20-\x2f\x3a-\x40\x5b-\x60\x7b-\x7e]/.test(password)) pool += 33;
  if (/[^\x00-\x7f]/.test(password)) pool += 64;
  let effective = 1;
  for (let i = 1; i < chars.length; i++) {
    const a = chars[i - 1]!.toLowerCase();
    const b = chars[i]!.toLowerCase();
    const step = (b.codePointAt(0) ?? 0) - (a.codePointAt(0) ?? 0);
    const keyboard = ROWS.some((row) => { const x = row.indexOf(a); const y = row.indexOf(b); return x >= 0 && y >= 0 && Math.abs(x - y) === 1; });
    effective += a === b ? 0.1 : Math.abs(step) === 1 || keyboard ? 0.25 : 1;
  }
  for (const word of COMMON) if (word.length >= 4 && (leet.includes(word) || lower.includes(word))) effective -= word.length * 0.75;
  const unique = new Set(chars).size;
  effective = Math.min(Math.max(effective, 1), unique * 2.5);
  const bits = effective * Math.log2(Math.max(pool, 1));
  return bits < 28 ? 0 : bits < 40 ? 1 : bits < 60 ? 2 : bits < 80 ? 3 : 4;
}
export const STRENGTH_LABELS = ['Very weak', 'Weak', 'Fair', 'Strong', 'Very strong'] as const;

// ---- cards ----

export function luhnValid(digits: string): boolean {
  if (!/^\d{12,19}$/.test(digits)) return false;
  let sum = 0;
  for (let i = 0; i < digits.length; i++) {
    let d = Number(digits[digits.length - 1 - i]);
    if (i % 2 === 1) { d *= 2; if (d > 9) d -= 9; }
    sum += d;
  }
  return sum % 10 === 0;
}

export function cardBrand(digits: string): string {
  const p = (n: number): number => Number(digits.slice(0, n)) || 0;
  if (digits.startsWith('4')) return 'Visa';
  if ((p(2) >= 51 && p(2) <= 55) || (p(4) >= 2221 && p(4) <= 2720)) return 'Mastercard';
  if (p(2) === 34 || p(2) === 37) return 'American Express';
  if (digits.startsWith('6011') || digits.startsWith('65') || (p(3) >= 644 && p(3) <= 649)) return 'Discover';
  if (p(4) >= 3528 && p(4) <= 3589) return 'JCB';
  if ([36, 38, 39].includes(p(2)) || (p(3) >= 300 && p(3) <= 305)) return 'Diners Club';
  if (digits.startsWith('62')) return 'UnionPay';
  if ([50, 56, 57, 58, 63, 67].includes(p(2))) return 'Maestro';
  return digits.length ? 'Card' : '';
}

/** "4242 4242 4242 4242" (Amex: 4-6-5). */
export function formatCardNumber(input: string): string {
  const digits = input.replace(/\D/g, '').slice(0, 19);
  const groups = cardBrand(digits) === 'American Express' ? [4, 6, 5] : [4, 4, 4, 4, 3];
  const out: string[] = [];
  let at = 0;
  for (const g of groups) { if (at >= digits.length) break; out.push(digits.slice(at, at + g)); at += g; }
  return out.join(' ');
}

export function cardExpiry(card: SavedCard): string {
  return card.month && card.year ? `${String(card.month).padStart(2, '0')}/${String(card.year % 100).padStart(2, '0')}` : '';
}

/** Whether a saved card's expiry is in the past at `now`. */
export function cardExpired(card: SavedCard, now: Date): boolean {
  if (!card.month || !card.year) return false;
  return card.year < now.getFullYear() || (card.year === now.getFullYear() && card.month < now.getMonth() + 1);
}

// ---- list ----

export type LoginFilter = 'all' | 'weak' | 'reused';

export function filterLogins(logins: readonly Login[], query: string, filter: LoginFilter): Login[] {
  const q = query.trim().toLowerCase();
  return logins
    .filter((l) => (filter === 'weak' ? l.score < 2 : filter === 'reused' ? l.reused : true))
    .filter((l) => !q || l.host.toLowerCase().includes(q) || l.user.toLowerCase().includes(q))
    .sort((a, b) => a.host.localeCompare(b.host) || a.user.localeCompare(b.user));
}

export function healthText(logins: readonly Login[]): string {
  const weak = logins.filter((l) => l.score < 2).length;
  const reused = logins.filter((l) => l.reused).length;
  if (logins.length === 0) return 'No passwords saved yet';
  const parts = [`${logins.length} saved`];
  if (weak) parts.push(`${weak} weak`);
  if (reused) parts.push(`${reused} reused`);
  if (!weak && !reused) parts.push('all strong and unique');
  return parts.join(' · ');
}

/** The tile letter for a site. */
export function monogram(host: string): string {
  const name = host.replace(/^www\d?\./, '');
  return (name[0] ?? '?').toUpperCase();
}

export function ackToast(ack: Ack, error: string): { text: string; ok: boolean } | null {
  switch (ack) {
    case 'saved': return { text: 'Saved', ok: true };
    case 'deleted': return { text: 'Deleted', ok: true };
    case 'copied': return { text: 'Copied. The clipboard clears in 30 seconds', ok: true };
    case 'imported': return { text: 'Import finished', ok: true };
    case 'exported': return { text: 'Exported. Delete the file when you are done with it', ok: true };
    case 'failed': return { text: error || 'That did not work. Nothing was changed.', ok: false };
    default: return null;
  }
}

// ---- moving from another app ----

export interface Provider { readonly id: string; readonly name: string; readonly steps: readonly string[] }
export const PROVIDERS: readonly Provider[] = [
  { id: 'chrome', name: 'Chrome', steps: ['Open chrome://password-manager/settings', 'Under Export passwords, choose Download file', 'Choose the .csv file below'] },
  { id: 'edge', name: 'Edge', steps: ['Open edge://wallet/passwords', 'Select More actions, then Export passwords', 'Choose the .csv file below'] },
  { id: 'firefox', name: 'Firefox', steps: ['Open about:logins', 'Select the ••• menu, then Export passwords', 'Choose the .csv file below'] },
  { id: 'apple', name: 'Safari / Apple', steps: ['Open the Passwords app (or Safari Settings, Passwords)', 'Choose File, then Export All Passwords', 'Copy the file to this PC and choose it below'] },
  { id: '1password', name: '1Password', steps: ['In the 1Password app, choose File, then Export', 'Pick your account and the CSV format', 'Choose the .csv file below'] },
  { id: 'bitwarden', name: 'Bitwarden', steps: ['In the web vault, open Tools, then Export vault', 'Choose the .csv file format', 'Choose the .csv file below'] },
  { id: 'lastpass', name: 'LastPass', steps: ['Open Advanced options, then Export', 'Confirm with your master password', 'Choose the .csv file below'] },
  { id: 'dashlane', name: 'Dashlane', steps: ['Open Settings, then Export data', 'Choose Export to CSV and unzip the download', 'Choose credentials.csv below'] },
  { id: 'nordpass', name: 'NordPass', steps: ['Open Settings, then Export items', 'Confirm with your master password', 'Choose the .csv file below'] },
  { id: 'proton', name: 'Proton Pass', steps: ['Open Settings, then Export', 'Choose the CSV format', 'Choose the .csv file below'] },
  { id: 'keepass', name: 'KeePass', steps: ['In KeePassXC choose Database, Export, CSV file (KeePass 2: File, Export, KeePass CSV)', 'Save the file on this PC', 'Choose the .csv file below'] },
  { id: 'other', name: 'Other', steps: ['Export your passwords as a CSV file', 'Bowser reads the site, username and password columns', 'Choose the .csv file below'] },
];

export function importSummary(state: ImportState): string {
  if (state.stage === 'done') {
    const extra = [state.skipped ? `${state.skipped} skipped` : '', state.failed ? `${state.failed} could not be read` : ''].filter(Boolean).join(', ');
    return `${plural(state.added, 'password')} imported${extra ? ` (${extra})` : ''}`;
  }
  const parts = [`${plural(state.fresh, 'new password')}`];
  if (state.dup) parts.push(`${state.dup} already saved`);
  if (state.conflict) parts.push(`${state.conflict} with a different saved password (kept as they are)`);
  if (state.bad) parts.push(`${state.bad} without a usable web address (skipped)`);
  return parts.join(', ');
}

export function plural(n: number, word: string): string {
  return `${n.toLocaleString('en-GB')} ${word}${n === 1 ? '' : 's'}`;
}
