(function (o) {
  'use strict';
  /* Bowser reader view: on demand only, run through ExecuteScript in the page's world. No network, no
     page data leaves: the only output is the fixed-shape string built by res().
     o = {a: toggle|probe|exit, m: reader|calm, t: theme, s: px, d: dark, c: {bg,fg,ac,bd,fl},
          k: css (common part, marker, reader part), b: bar css}, built and validated by Rust. */
  var W = window, D = document, de = D.documentElement, E = Element.prototype, P = W.performance;
  var n = Object.freeze({
    q: E.querySelectorAll, qd: Document.prototype.querySelectorAll, ga: E.getAttribute, sa: E.setAttribute, ra: E.removeAttribute, rm: E.remove, as: E.attachShadow,
    tc: Object.getOwnPropertyDescriptor(Node.prototype, 'textContent').get, cs: W.getComputedStyle.bind(W),
    now: P.now.bind(P), mk: D.createElement.bind(D), tw: D.createTreeWalker.bind(D), st: W.scrollTo.bind(W), sl: Array.prototype.slice
  });
  var KEY = Symbol.for('bowser.reader.v1'), T0 = n.now(), TH = ['auto', 'light', 'sepia', 'dark'];
  var NEG = /(^|[^a-z])(comments?|sidebar|footer|nav|menu|share|social|related|promo|adverts?|ads?|sponsor|cookies?|consent|widget|popup|modal|newsletter|subscribe|breadcrumbs?|toc|recirc|taboola|outbrain|editsection)([^a-z]|$)/;
  var POS = /(^|[^a-z])(article|post|entry|story|content|body|text|main|prose)([^a-z]|$)/;
  var COOK = /(^|[^a-z])(cookies?|consent|gdpr|cmp|onetrust|didomi|cookiebot|truste)([^a-z]|$)/;
  var HEX = /^(#[0-9a-f]{3,8}|rgba?\([0-9., %]{1,50}\))$/i;

  function late() { return n.now() - T0 > 250; }
  function q(e, s) { return (e === D ? n.qd : n.q).call(e, s); }
  function tx(e) { return n.tc.call(e); }
  function at(e, k) { return n.ga.call(e, k) || ''; }
  function set(e, k, v) { n.sa.call(e, k, v); }
  // Text length and words ignore script and style text, which textContent includes.
  function tl(e) {
    var x = q(e, 'script,style,noscript,template'), s = tx(e).length, i;
    for (i = 0; i < x.length; i++) s -= tx(x[i]).length;
    return s;
  }
  function plain(e) {
    var w = n.tw(e, 4), s = '', x;
    while ((x = w.nextNode())) if (!/^(SCRIPT|STYLE|NOSCRIPT|TEMPLATE)$/.test(x.parentNode.nodeName)) s += x.data + ' ';
    return s;
  }
  function key(e) { return (at(e, 'id') + ' ' + at(e, 'class')).toLowerCase(); }
  function words(t) {
    var c = 0, sp = true, i, k, w;
    for (i = 0; i < t.length; i++) {
      k = t.charCodeAt(i);
      if ((k > 0x303f && k < 0xa000) || (k > 0xabff && k < 0xd7b0)) { c += 0.5; sp = true; continue; } // CJK, Hangul
      w = k > 32;
      if (w && sp) c++;
      sp = !w;
    }
    return Math.floor(c);
  }
  function linkLen(e) {
    var a = q(e, 'a'), s = 0, i;
    for (i = 0; i < a.length && i < 3000; i++) s += tx(a[i]).length;
    return s;
  }
  function bad(p) {
    var a = p, d = 0;
    for (; a && a !== D.body; a = a.parentElement, d++) {
      if (/^(NAV|ASIDE|FOOTER)$/.test(a.tagName) || /^(navigation|complementary|contentinfo|banner|search|dialog)$/.test(at(a, 'role')) || (d < 3 && NEG.test(key(a)))) return true;
    }
    return false;
  }
  function base(a) {
    var t = a.tagName, k = key(a), s = 0;
    if (t === 'ARTICLE' || at(a, 'itemprop') === 'articleBody') s += 10; else if (t === 'MAIN' || at(a, 'role') === 'main') s += 6;
    return s + (POS.test(k) ? 4 : 0) - (NEG.test(k) ? 10 : 0);
  }
  // Readability-style scoring, written from scratch: each long paragraph votes for its nearest ancestors.
  function find() {
    var ps = D.getElementsByTagName('p'), m = new Map(), c = [], i, j, a, s, t, best = null, bs = 0, len, ld, sc, e, tg;
    for (i = 0; i < ps.length && i < 1500 && !late(); i++) {
      t = tx(ps[i]);
      if (t.length < 50 || bad(ps[i])) continue;
      s = t.split(',').length + Math.min(t.length / 100 | 0, 3);
      for (a = ps[i].parentElement, j = 0; a && a !== de && j < 3; a = a.parentElement, j++) m.set(a, (m.has(a) ? m.get(a) : base(a)) + s / (j + 1));
    }
    m.forEach(function (v, k) { c.push([k, v]); });
    c.sort(function (x, y) { return y[1] - x[1]; });
    for (i = 0; i < c.length && i < 8; i++) {
      e = c[i][0]; sc = c[i][1] * (1 - linkLen(e) / Math.max(tl(e), 1));
      if (sc > bs) { bs = sc; best = e; }
    }
    if (!best) return null;
    len = tl(best);
    tg = best.tagName === 'ARTICLE' || at(best, 'itemprop') === 'articleBody' ? 'ARTICLE' : '';
    for (a = best.parentElement, j = 0; a && a !== de && j < 4 && tg !== 'ARTICLE'; a = a.parentElement, j++) { // widen to the title and lead
      tg = a.tagName;
      if (tl(a) >= len * 1.6 + 400) continue;
      if (tg === 'ARTICLE' || at(a, 'itemprop') === 'articleBody') { best = a; tg = 'ARTICLE'; } else if (tg === 'MAIN' || at(a, 'role') === 'main') best = a;
    }
    t = plain(best);
    return { el: best, w: words(t), l: Math.round(100 * Math.min(1, linkLen(best) / Math.max(t.length, 1))) };
  }
  // Marks fixed or sticky overlays, cookie banners and (strict) sidebar-like blocks under scope as hidden.
  function sweep(scope, keep, strict) {
    var all = q(scope, '*'), i, e, k, p, h = 0;
    for (i = 0; i < all.length && i < 4000 && !late(); i++) {
      e = all[i];
      if (keep && e.contains(keep)) continue;
      k = key(e); p = style(e, 'position');
      if (((p === 'fixed' || p === 'sticky') && (keep || tx(e).length < 2500)) || COOK.test(k) || (strict && NEG.test(k)) || at(e, 'role') === 'dialog' || at(e, 'aria-modal') === 'true') { set(e, 'data-bwr-x', 'h'); h++; }
    }
    return h;
  }
  function style(e, k) { try { return n.cs(e)[k]; } catch (x) { return ''; } } // a page can make this throw
  function ttl() { try { return D.title || ''; } catch (x) { return ''; } }
  function sheet(root, css) {
    var s, a;
    try { // a constructed sheet is not blocked by the page's style-src
      s = new CSSStyleSheet(); s.replaceSync(css);
      a = n.sl.call(root.adoptedStyleSheets); a.push(s); root.adoptedStyleSheets = a;
    } catch (x) { s = n.mk('style'); s.textContent = css; (root === D ? D.head || de : root).appendChild(s); }
    return s;
  }
  function paint(st) {
    var c = o.c || {}, s = de.style, k;
    set(de, 'data-bwr-t', st.th);
    for (k in { bg: 1, fg: 1, ac: 1, bd: 1, fl: 1 }) {
      if (st.th === 'auto' && typeof c[k] === 'string' && c[k].length < 64 && HEX.test(c[k])) s.setProperty('--bwr-' + k, c[k]); else s.removeProperty('--bwr-' + k);
    }
    s.setProperty('--bwr-ft', st.fs + 'px');
    s.setProperty('--bwr-ff', FONTS[tf.f]);
    s.setProperty('--bwr-lh', String(tf.l / 10));
    s.setProperty('--bwr-mw', tf.w + 'ch');
    if (tf.v) set(de, 'data-bwr-v', 'on'); else n.ra.call(de, 'data-bwr-v');
    if (st.th === 'auto') s.setProperty('color-scheme', o.d ? 'dark' : 'light'); else s.removeProperty('color-scheme');
    for (k in st.tb) set(st.tb[k], 'aria-pressed', String(k === st.th));
  }
  function bar(st) {
    st.tb = {};
    if (!tf.g && !tf.r) return; // no controls and no reading time: nothing to put on the page
    var h = n.mk('div'), s = n.as.call(h, { mode: 'closed' }), b = n.mk('div'), i, t;
    function btn(t, l, f) { var e = n.mk('button'); e.type = 'button'; e.textContent = t; set(e, 'aria-label', l); e.addEventListener('click', f); b.appendChild(e); return e; }
    b.className = 'b'; set(h, 'data-bwr-x', 'b'); set(b, 'role', tf.g ? 'toolbar' : 'status'); set(b, 'aria-label', tf.g ? 'Reader controls' : 'Reading time');
    function theme(t) { st.tb[t] = btn(t.charAt(0).toUpperCase() + t.slice(1), t + ' theme', function () { st.th = t; paint(st); }); }
    if (tf.g) {
      btn('A−', 'Smaller text', function () { st.fs = Math.max(14, st.fs - 2); paint(st); });
      btn('A+', 'Larger text', function () { st.fs = Math.min(30, st.fs + 2); paint(st); });
      b.appendChild(n.mk('i'));
      for (i = 0; i < TH.length; i++) theme(TH[i]);
    }
    if (tf.r) {
      if (tf.g) b.appendChild(n.mk('i'));
      t = n.mk('span'); t.className = 't'; t.textContent = Math.max(1, Math.ceil(st.w / 230)) + ' min read'; b.appendChild(t);
    }
    sheet(s, String(o.b)); s.appendChild(b); D.body.appendChild(h);
  }
  function enter(m, f) {
    var st = { m: m, y: W.scrollY || 0, th: th, fs: fs, sh: null, tb: null, w: f ? f.w : 0 }, css = String(o.k).split('/*R*/'), root, e, p, c, i, h, t;
    Object.defineProperty(D, KEY, { value: st, configurable: true });
    set(de, 'data-bwr', m);
    st.sh = sheet(D, m === 'reader' ? css.join('') : css[0]);
    if (m === 'calm') {
      if (sweep(D.body, f && f.el, false)) { if (style(de, 'overflowY') === 'hidden') set(de, 'data-bwr-x', 'u'); if (style(D.body, 'overflowY') === 'hidden') set(D.body, 'data-bwr-x', 'u'); }
      return st;
    }
    root = f.el;
    for (e = root; e && e !== D.body; e = p) {
      p = e.parentElement; set(e, 'data-bwr-k', e === root ? 'r' : 'a');
      if (p) for (c = p.children, i = 0; i < c.length; i++) if (c[i] !== e) set(c[i], 'data-bwr-x', 'h');
    }
    if (root === D.body) set(root, 'data-bwr-k', 'r');
    if (!q(root, 'h1').length) { // the title may sit outside the content block
      h = n.mk('h1'); t = q(D, 'h1')[0]; h.textContent = ((t && tx(t).trim()) || ttl()).slice(0, 300);
      set(h, 'data-bwr-x', 't'); root.insertBefore(h, root.firstChild);
    }
    sweep(root, null, true); bar(st); paint(st); n.st(0, 0);
    return st;
  }
  function exit(st) {
    var a = q(D, '[data-bwr-x],[data-bwr-k]'), i, x, s = st.sh, keep = [], y, v;
    for (i = 0; i < a.length; i++) {
      x = at(a[i], 'data-bwr-x');
      if (x === 't' || x === 'b') n.rm.call(a[i]); else { n.ra.call(a[i], 'data-bwr-x'); n.ra.call(a[i], 'data-bwr-k'); }
    }
    v = ['data-bwr', 'data-bwr-t', 'data-bwr-x', 'data-bwr-v'];
    for (i = 0; i < 4; i++) n.ra.call(de, v[i]);
    v = ['bg', 'fg', 'ac', 'bd', 'fl', 'ft', 'ff', 'lh', 'mw'];
    for (i = 0; i < 9; i++) de.style.removeProperty('--bwr-' + v[i]);
    de.style.removeProperty('color-scheme');
    if (!at(de, 'style')) n.ra.call(de, 'style');
    if (s instanceof CSSStyleSheet) {
      a = n.sl.call(D.adoptedStyleSheets);
      for (i = 0; i < a.length; i++) if (a[i] !== s) keep.push(a[i]);
      D.adoptedStyleSheets = keep;
    } else if (s) n.rm.call(s);
    delete D[KEY];
    if (st.m === 'reader') n.st(0, st.y);
  }

  var cur = D[KEY], a = o.a, m = o.m === 'calm' ? 'calm' : 'reader', f;
  var th = TH.indexOf(o.t) < 0 ? 'auto' : o.t, fs = Math.min(30, Math.max(14, (o.s | 0) || 19));
  // Typography from Settings > Page tools: font key, line height in tenths, column width in ch, vertical text,
  // the control bar and the reading time. Anything out of range falls back to the default.
  var FONTS = { serif: 'Georgia,"Iowan Old Style","Times New Roman",serif', sans: '"Segoe UI Variable","Segoe UI",system-ui,sans-serif', mono: 'Consolas,"Cascadia Mono",monospace' };
  var tf = {
    f: FONTS[o.f] && Object.prototype.hasOwnProperty.call(FONTS, o.f) ? o.f : 'serif',
    l: (o.l | 0) >= 12 && (o.l | 0) <= 24 ? o.l | 0 : 16,
    w: (o.w | 0) >= 40 && (o.w | 0) <= 120 ? o.w | 0 : 68,
    v: o.v === true, g: o.g !== false, r: o.r === true
  };
  if (!(cur && (cur.m === 'reader' || cur.m === 'calm') && typeof cur.fs === 'number')) cur = null;
  if (cur) { th = cur.th; fs = cur.fs; }
  // The page's language, for offering translation: lang (or the content-language meta) cut to 2-3 letters.
  function lang() {
    var l = at(de, 'lang'), m = !l && q(D, 'meta[http-equiv=content-language]')[0];
    l = (l || (m ? at(m, 'content') : '')).split(/[-_,\s]/)[0].toLowerCase();
    return /^[a-z]{2,3}$/.test(l) ? l : '';
  }
  function res(code, st, w, l) { return 'r2|' + code + '|' + (st ? st.m : 'off') + '|' + (w | 0) + '|' + (l | 0) + '|' + th + '|' + fs + '|' + lang(); }
  try {
    if (a === 'test') return { find: find, words: words, bad: bad };
    if (a === 'probe') {
      if (cur) return res('ok', cur, 0, 0);
      f = find();
      return res('ok', null, f ? f.w : 0, f ? f.l : 0);
    }
    if (cur) { exit(cur); if (a === 'exit' || cur.m === m) return res('ok', null, 0, 0); }
    if (a === 'exit') return res('ok', null, 0, 0);
    f = find();
    if (m === 'reader' && (!f || f.w < 80)) return res('none', null, f ? f.w : 0, f ? f.l : 0);
    return res('ok', enter(m, f), f ? f.w : 0, f ? f.l : 0);
  } catch (x) {
    try { if (D[KEY]) exit(D[KEY]); } catch (y) { /* leave the page as it is */ }
    return res('err', null, 0, 0);
  }
})
