(function (o) {
  'use strict';
  /* Bowser reader view: on demand only, run in the page's world. No network, no
     page data leaves: the only output is the fixed-shape string built by res().
     o = {a: toggle|probe|exit, m: reader|calm, t: theme, s: px, d: dark, c: {bg,fg,ac,bd,fl},
          k: css (common part, marker, reader part), b: bar css}, built and validated by Rust. */
  var W = window, D = document, de = D.documentElement, E = Element.prototype, P = W.performance;
  var n = Object.freeze({
    q: E.querySelectorAll, qd: Document.prototype.querySelectorAll, ga: E.getAttribute, sa: E.setAttribute, ra: E.removeAttribute, rm: E.remove, as: E.attachShadow,
    tc: Object.getOwnPropertyDescriptor(Node.prototype, 'textContent').get, cs: W.getComputedStyle.bind(W),
    now: P.now.bind(P), mk: D.createElement.bind(D), st: W.scrollTo.bind(W), sl: Array.prototype.slice,
    pause: W.scheduler && W.scheduler.yield ? W.scheduler.yield.bind(W.scheduler) : null, defer: W.setTimeout.bind(W)
  });
  var KEY = Symbol.for('bowser.reader.v1'), TH = ['auto', 'light', 'sepia', 'dark'];
  var NEG = /(^|[^a-z])(comments?|sidebar|footer|nav|menu|share|social|related|promo|adverts?|ads?|sponsor|cookies?|consent|widget|popup|modal|newsletter|subscribe|breadcrumbs?|toc|recirc|taboola|outbrain|editsection)([^a-z]|$)/;
  var POS = /(^|[^a-z])(article|post|entry|story|content|body|text|main|prose)([^a-z]|$)/;
  var COOK = /(^|[^a-z])(cookies?|consent|gdpr|cmp|onetrust|didomi|cookiebot|truste)([^a-z]|$)/;
  var HEX = /^(#[0-9a-f]{3,8}|rgba?\([0-9., %]{1,50}\))$/i;

  // Slice by both time and work. The latter also bounds a slice on pages that replace the clock.
  var SCAN = Symbol.for('bowser.reader.scan'), STOP = {};
  var SLICE_UNITS = 512, SLICE_MS = 4, TOTAL_UNITS = 2000000, TOTAL_MS = 250, TEXT_CHUNK = 512;
  var sliceStart, units, checkAt, total = 0;
  function step(cost) {
    units += cost; total += cost;
    if (total > TOTAL_UNITS) throw STOP;
    if (units >= SLICE_UNITS) return true;
    if (units < checkAt) return false;
    checkAt = units + 32;
    return n.now() - sliceStart >= SLICE_MS;
  }
  // Walk the light DOM without live collections or an uninterruptible climb back up a deep subtree.
  function* walk(root, leave) {
    var x = root.firstChild, parent;
    while (x) {
      if (step(1)) {
        parent = x.parentNode;
        yield null;
        if (x.parentNode !== parent || !x.isConnected) throw STOP;
      }
      yield x;
      if (x.firstChild) { x = x.firstChild; continue; }
      while (x && x !== root) {
        if (leave) leave(x);
        if (x.nextSibling) { x = x.nextSibling; break; }
        x = x.parentNode;
        if (step(1)) yield null;
      }
      if (x === root) return;
    }
  }
  function q(e, s) { return (e === D ? n.qd : n.q).call(e, s); }
  function tx(e) { return n.tc.call(e); }
  function at(e, k) { return n.ga.call(e, k) || ''; }
  function set(e, k, v) { n.sa.call(e, k, v); }
  function key(e) { return (at(e, 'id') + ' ' + at(e, 'class')).toLowerCase(); }
  function* paragraph(p) {
    var len = 0, commas = 0, data, i, end;
    for (var x of walk(p)) {
      if (!x) { yield; continue; }
      if (x.nodeType !== 3 && x.nodeType !== 4) continue;
      data = x.data; len += data.length;
      for (i = 0; i < data.length;) {
        if (step(4)) yield;
        end = Math.min(i + TEXT_CHUNK, data.length);
        for (; i < end; i++) if (data.charCodeAt(i) === 44) commas++;
      }
    }
    if (!p.isConnected) throw STOP;
    return len < 50 ? 0 : 1 + commas + Math.min(len / 100 | 0, 3);
  }
  var EXCLUDED = /^(script|style|noscript|template)$/;
  // Stream lengths and words separately. Each text node contributes a trailing space to the
  // plain-text metric, as it did when we built one large string for the entire article.
  function* measure(root, withWords) {
    var skip = 0, anchors = new Set(), seen = 0, len = 0, links = 0, plain = 0, words = 0;
    var data, i, end, k, gap, word;
    function leave(x) {
      if (x.nodeType !== 1) return;
      if (EXCLUDED.test(x.localName)) skip--;
      if (x.localName === 'a') anchors.delete(x);
    }
    for (var x of walk(root, leave)) {
      if (!x) { yield; continue; }
      if (x.nodeType === 1) {
        if (EXCLUDED.test(x.localName)) skip++;
        if (x.localName === 'a' && seen++ < 3000) anchors.add(x);
        continue;
      }
      if (x.nodeType !== 3 && x.nodeType !== 4) continue;
      data = x.data;
      len += (1 - skip) * data.length;
      links += anchors.size * data.length;
      if (!withWords || x.nodeType !== 3 || /^(SCRIPT|STYLE|NOSCRIPT|TEMPLATE)$/.test(x.parentNode.nodeName)) continue;
      plain += data.length + 1; gap = true;
      for (i = 0; i < data.length;) {
        if (step(4)) yield;
        end = Math.min(i + TEXT_CHUNK, data.length);
        for (; i < end; i++) {
          k = data.charCodeAt(i);
          if ((k > 0x303f && k < 0xa000) || (k > 0xabff && k < 0xd7b0)) { words += 0.5; gap = true; continue; }
          word = k > 32;
          if (word && gap) words++;
          gap = !word;
        }
      }
    }
    if (!root.isConnected) throw STOP;
    return { len: len, links: links, w: Math.floor(words), l: Math.round(100 * Math.min(1, links / Math.max(plain, 1))) };
  }
  function* bad(p) {
    var a = p, d = 0;
    for (; a && a !== D.body; a = a.parentElement, d++) {
      if (step(1)) yield;
      // A paragraph inside a shielded unit never votes for the article.
      if (/(^| )shield( |$)/.test(at(a, 'data-sfw'))) return true;
      if (/^(NAV|ASIDE|FOOTER)$/.test(a.tagName) || /^(navigation|complementary|contentinfo|banner|search|dialog)$/.test(at(a, 'role')) || (d < 3 && NEG.test(key(a)))) return true;
    }
    return false;
  }
  function base(a) {
    var t = a.tagName, k = key(a), s = 0;
    if (t === 'ARTICLE' || at(a, 'itemprop') === 'articleBody') s += 10; else if (t === 'MAIN' || at(a, 'role') === 'main') s += 6;
    return s + (POS.test(k) ? 4 : 0) - (NEG.test(k) ? 10 : 0);
  }
  // Each long paragraph votes for its nearest ancestors. Keep the eight highest candidates in
  // insertion order for ties, without sorting a page-sized array in one turn.
  function* find() {
    var m = new Map(), c = [], count = 0, i, j, a, s, best = null, bs = 0, len, sc, e, tg, metrics;
    for (var x of walk(de)) {
      if (!x) { yield; continue; }
      if (x.nodeType !== 1 || x.localName !== 'p') continue;
      if (++count > 1500) break;
      s = yield* paragraph(x);
      if (!s || (yield* bad(x))) continue;
      for (a = x.parentElement, j = 0; a && a !== de && j < 3; a = a.parentElement, j++) m.set(a, (m.has(a) ? m.get(a) : base(a)) + s / (j + 1));
    }
    for (var candidate of m) {
      if (step(1)) yield;
      for (i = 0; i < c.length && c[i][1] >= candidate[1]; i++);
      if (i < 8) { c.splice(i, 0, candidate); if (c.length > 8) c.pop(); }
    }
    for (i = 0; i < c.length; i++) {
      e = c[i][0]; metrics = yield* measure(e, false);
      sc = c[i][1] * (1 - metrics.links / Math.max(metrics.len, 1));
      if (sc > bs) { bs = sc; best = e; len = metrics.len; }
    }
    if (!best) return null;
    tg = best.tagName === 'ARTICLE' || at(best, 'itemprop') === 'articleBody' ? 'ARTICLE' : '';
    for (a = best.parentElement, j = 0; a && a !== de && j < 4 && tg !== 'ARTICLE'; a = a.parentElement, j++) {
      tg = a.tagName;
      metrics = yield* measure(a, false);
      if (metrics.len >= len * 1.6 + 400) continue;
      if (tg === 'ARTICLE' || at(a, 'itemprop') === 'articleBody') { best = a; tg = 'ARTICLE'; } else if (tg === 'MAIN' || at(a, 'role') === 'main') best = a;
    }
    metrics = yield* measure(best, true);
    return { el: best, w: metrics.w, l: metrics.l };
  }
  // Marks fixed or sticky overlays, cookie banners and (strict) sidebar-like blocks under scope as hidden.
  function sweep(scope, keep, strict) {
    var all = q(scope, '*'), start = n.now(), i, e, k, p, hide, h = 0;
    for (i = 0; i < all.length && i < 4000; i++) {
      if (i >= 64 && (i & 31) === 0 && n.now() - start > 250) break;
      e = all[i];
      if (keep && e.contains(keep)) continue;
      k = key(e); hide = COOK.test(k) || (strict && NEG.test(k)) || at(e, 'role') === 'dialog' || at(e, 'aria-modal') === 'true';
      if (!hide) {
        p = style(e, 'position');
        hide = (p === 'fixed' || p === 'sticky') && (keep || tx(e).length < 2500);
      }
      if (hide) { set(e, 'data-bwr-x', 'h'); h++; }
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
  // Return only the 2–3 letter language hint and the fixed reader-result fields.
  var language = '';
  function* readLanguage() {
    var l = at(de, 'lang');
    if (!l) for (var x of walk(de)) {
      if (!x) { yield; continue; }
      if (x.nodeType === 1 && x.matches('meta[http-equiv=content-language]')) { l = at(x, 'content'); break; }
    }
    l = l.slice(0, 4).split(/[-_,\s]/)[0].toLowerCase();
    language = /^[a-z]{2,3}$/.test(l) ? l : '';
  }
  function res(code, st, w, l) { return 'r2|' + code + '|' + (st ? st.m : 'off') + '|' + (w | 0) + '|' + (l | 0) + '|' + th + '|' + fs + '|' + language; }
  function* read() {
    yield* readLanguage();
    if (a === 'probe') {
      if (cur) return res('ok', cur, 0, 0);
      f = yield* find();
      return res('ok', null, f ? f.w : 0, f ? f.l : 0);
    }
    if (cur) { exit(cur); if (a === 'exit' || cur.m === m) return res('ok', null, 0, 0); }
    if (a === 'exit') return res('ok', null, 0, 0);
    f = yield* find();
    if (m === 'reader' && (!f || f.w < 80)) return res('none', null, f ? f.w : 0, f ? f.l : 0);
    if (f && !f.el.isConnected) throw STOP;
    return res('ok', enter(m, f), f ? f.w : 0, f ? f.l : 0);
  }
  return (async function () {
    var href = W.location.href, body = D.body, reads = read(), cpu = 0, wake, canceled = false, slice;
    var events = new AbortController(), opts = { signal: events.signal }, navigation = W.navigation;
    function cancel() { canceled = true; if (wake) wake(); }
    function onVisibility() { if (a === 'probe' && D.hidden) cancel(); }
    try {
      if (typeof D[SCAN] === 'function') D[SCAN]();
      Object.defineProperty(D, SCAN, { value: cancel, configurable: true });
      W.addEventListener('pagehide', cancel, opts);
      W.addEventListener('popstate', cancel, opts);
      W.addEventListener('hashchange', cancel, opts);
      if (navigation) navigation.addEventListener('currententrychange', cancel, opts);
      D.addEventListener('visibilitychange', onVisibility, opts);
      while (!canceled && W.location.href === href && D.body === body && D.documentElement === de) {
        units = 0; checkAt = 32; sliceStart = n.now();
        slice = reads.next();
        if (slice.done) return slice.value;
        cpu += Math.max(0, n.now() - sliceStart);
        if (cpu > TOTAL_MS) break;
        await new Promise(function (resolve) {
          wake = resolve;
          if (n.pause) n.pause().then(resolve, resolve); else n.defer(resolve, 0);
        });
        wake = null;
      }
      return null;
    } catch (x) {
      if (x === STOP) return null;
      try { if (D[KEY]) exit(D[KEY]); } catch (y) { /* leave the page as it is */ }
      return res('err', null, 0, 0);
    } finally {
      events.abort();
      if (D[SCAN] === cancel) delete D[SCAN];
    }
  })();
})
