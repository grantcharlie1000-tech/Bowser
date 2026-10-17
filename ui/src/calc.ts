// The palette's calculator: the same grammar and output rules as src/commands/calc.rs, checked against
// the same cases (test/fixtures/calc-cases.json). Pure; nothing but numbers is ever evaluated.
//
// Grammar: + - * / % ^ ( ), unary + and -, × and ÷, decimals, the constants pi and e, and the
// functions sqrt abs round floor ceil sin cos tan ln log exp. ^ is right associative and binds
// tighter than unary minus (-2^2 is -4); % is the remainder. Text only counts as a calculation when
// it holds an operator or a function call, so 42 is not one.

const MAX_LEN = 120;
const MAX_DEPTH = 24;

type Token = { readonly k: 'num'; readonly v: number } | { readonly k: 'id'; readonly v: string } | { readonly k: 'op'; readonly v: string } | { readonly k: '(' } | { readonly k: ')' };

function tokenize(text: string): Token[] | null {
  const chars = Array.from(text);
  const tokens: Token[] = [];
  let at = 0;
  while (at < chars.length) {
    const c = chars[at]!;
    if (c === ' ' || c === '\t') { at += 1; continue; }
    if ((c >= '0' && c <= '9') || c === '.') {
      const start = at;
      let dots = 0;
      while (at < chars.length && ((chars[at]! >= '0' && chars[at]! <= '9') || chars[at] === '.')) {
        if (chars[at] === '.') dots += 1;
        at += 1;
      }
      const literal = chars.slice(start, at).join('');
      if (dots > 1 || literal === '.') return null;
      const v = Number(literal);
      if (!Number.isFinite(v)) return null;
      tokens.push({ k: 'num', v });
      continue;
    }
    if (/[A-Za-z]/.test(c)) {
      const start = at;
      while (at < chars.length && /[A-Za-z]/.test(chars[at]!)) at += 1;
      tokens.push({ k: 'id', v: chars.slice(start, at).join('').toLowerCase() });
      continue;
    }
    if ('+-*/%^'.includes(c)) { tokens.push({ k: 'op', v: c }); at += 1; continue; }
    if (c === '×') { tokens.push({ k: 'op', v: '*' }); at += 1; continue; }
    if (c === '÷') { tokens.push({ k: 'op', v: '/' }); at += 1; continue; }
    if (c === '(') { tokens.push({ k: '(' }); at += 1; continue; }
    if (c === ')') { tokens.push({ k: ')' }); at += 1; continue; }
    return null;
  }
  return tokens;
}

/** Half away from zero, as Rust's f64::round (Math.round rounds halves up). */
function roundAway(x: number): number {
  return Math.sign(x) * Math.round(Math.abs(x));
}

class Parser {
  at = 0;
  active = false;
  constructor(readonly tokens: Token[]) {}

  private peek(): Token | undefined { return this.tokens[this.at]; }
  private isOp(...ops: string[]): string | null {
    const t = this.peek();
    return t && t.k === 'op' && ops.includes(t.v) ? t.v : null;
  }

  expr(depth: number): number | null {
    if (depth > MAX_DEPTH) return null;
    let value = this.term(depth);
    if (value === null) return null;
    for (let op = this.isOp('+', '-'); op; op = this.isOp('+', '-')) {
      this.at += 1;
      this.active = true;
      const right = this.term(depth);
      if (right === null) return null;
      value = op === '+' ? value + right : value - right;
    }
    return value;
  }

  private term(depth: number): number | null {
    let value = this.unary(depth);
    if (value === null) return null;
    for (let op = this.isOp('*', '/', '%'); op; op = this.isOp('*', '/', '%')) {
      this.at += 1;
      this.active = true;
      const right = this.unary(depth);
      if (right === null) return null;
      value = op === '*' ? value * right : op === '/' ? value / right : value % right;
    }
    return value;
  }

  private unary(depth: number): number | null {
    if (depth > MAX_DEPTH) return null;
    const op = this.isOp('-', '+');
    if (op) {
      this.at += 1;
      this.active = true;
      const inner = this.unary(depth + 1);
      return inner === null ? null : op === '-' ? -inner : inner;
    }
    return this.power(depth);
  }

  private power(depth: number): number | null {
    const base = this.atom(depth);
    if (base === null) return null;
    if (this.isOp('^')) {
      this.at += 1;
      this.active = true;
      const exponent = this.unary(depth + 1);
      return exponent === null ? null : Math.pow(base, exponent);
    }
    return base;
  }

  private atom(depth: number): number | null {
    const token = this.peek();
    if (!token) return null;
    this.at += 1;
    if (token.k === 'num') return token.v;
    if (token.k === '(') {
      const value = this.expr(depth + 1);
      if (value === null || this.peek()?.k !== ')') return null;
      this.at += 1;
      return value;
    }
    if (token.k === 'id') {
      if (this.peek()?.k === '(') {
        this.at += 1;
        const argument = this.expr(depth + 1);
        if (argument === null || this.peek()?.k !== ')') return null;
        this.at += 1;
        this.active = true;
        switch (token.v) {
          case 'sqrt': return Math.sqrt(argument);
          case 'abs': return Math.abs(argument);
          case 'round': return roundAway(argument);
          case 'floor': return Math.floor(argument);
          case 'ceil': return Math.ceil(argument);
          case 'sin': return Math.sin(argument);
          case 'cos': return Math.cos(argument);
          case 'tan': return Math.tan(argument);
          case 'ln': return Math.log(argument);
          case 'log': return Math.log10(argument);
          case 'exp': return Math.exp(argument);
          default: return null;
        }
      }
      if (token.v === 'pi') return Math.PI;
      if (token.v === 'e') return Math.E;
    }
    return null;
  }
}

/** The value of `text` when it is a calculation, else `null`. */
export function evaluate(text: string): number | null {
  const trimmed = text.trim();
  if (trimmed === '' || Array.from(trimmed).length > MAX_LEN) return null;
  const tokens = tokenize(trimmed);
  if (!tokens) return null;
  const parser = new Parser(tokens);
  const value = parser.expr(0);
  return value !== null && parser.at === tokens.length && parser.active ? value : null;
}

/**
 * `text` evaluated and written the way the palette shows it: twelve significant digits, no
 * trailing zeros, no exponent. `null` for anything that is not a calculation or whose result is not
 * finite, is 1e15 or larger, or is a non-zero value below 1e-6.
 */
export function resultText(text: string): string | null {
  const value = evaluate(text);
  if (value === null || !Number.isFinite(value) || Math.abs(value) >= 1e15) return null;
  const rounded = Number(value.toPrecision(12));
  if (rounded !== 0 && Math.abs(rounded) < 1e-6) return null;
  return String(rounded === 0 ? 0 : rounded);
}
