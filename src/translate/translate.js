(function (o) {
  'use strict';
  /* Bowser page translation: on demand only, run through ExecuteScript in the page's world. It makes no
     request and keeps nothing outside this document: Rust fetches the text, a local worker translates
     it, and this script only reads text nodes and writes translated strings back.
     o = {a: collect|apply|restore, k: token, i: first index, s: [translated strings]}, built and validated
     by Rust. The result is a plain object, validated again by Rust (translate::parse_reply). */
  var W = window, D = document, E = Element.prototype, P = W.performance;
  var cd = Object.getOwnPropertyDescriptor(CharacterData.prototype, 'data');
  var n = Object.freeze({
    ga: E.getAttribute, sa: E.setAttribute, ra: E.removeAttribute, cl: E.closest, qs: E.querySelectorAll,
    tw: D.createTreeWalker.bind(D), nx: TreeWalker.prototype.nextNode, rect: E.getBoundingClientRect,
    dg: cd.get, ds: cd.set, now: P.now.bind(P), slice: Array.prototype.slice, push: Array.prototype.push
  });
  var KEY = Symbol.for('bowser.translate.v1');
  var MAX_ITEMS = 6000, MAX_ATTRS = 400, BATCH_ITEMS = 300, BATCH_CHARS = 16000, ITEM_MAX = 2000, BUDGET_MS = 400;
  var SKIP = /^(script|style|noscript|template|code|pre|kbd|samp|var|textarea|input|select|option|svg|math|iframe|object|embed|canvas|audio|video|head|title)$/i;
  var ATTR_SKIP = /^(script|style|noscript|template|head|svg|math)$/i;
  var NOTR = /(^|\s)notranslate(\s|$)/;
  var LETTER = /\p{L}/u, URLISH = /^(https?:\/\/|www\.)\S+$/i;
  var ATTRS = ['title', 'alt', 'placeholder', 'aria-label'];

  function at(e, k) { return n.ga.call(e, k) || ''; }
  function res(c, st, i, s) {
    return { v: 1, c: c, k: st ? st.tok : '', i: i | 0, n: st ? st.items.length : 0, s: s || [] };
  }
  function ok(text) {
    var t = text.trim();
    return t.length > 0 && t.length <= ITEM_MAX && LETTER.test(t) && !URLISH.test(t);
  }
  function skipEl(e) {
    var ce;
    if (SKIP.test(e.nodeName)) return true;
    if (at(e, 'translate') === 'no' || NOTR.test(at(e, 'class'))) return true;
    ce = n.ga.call(e, 'contenteditable'); // null when absent, '' when present without a value
    return ce === '' || ce === 'true' || ce === 'plaintext-only';
  }
  function viewportFirst(items) {
    var seen = new Map(), vh = W.innerHeight || 800, a = [], b = [], i, it, e, r, v;
    for (i = 0; i < items.length; i++) {
      it = items[i]; e = it.el;
      v = seen.get(e);
      if (v === undefined) {
        r = n.rect.call(e);
        v = (r.width > 0 || r.height > 0) && r.bottom > -200 && r.top < vh + 1200 ? 0 : (r.width > 0 || r.height > 0 ? 1 : 2);
        seen.set(e, v);
      }
      (v === 0 ? a : b).push(it);
    }
    return a.concat(b);
  }
  function gather() {
    var items = [], w, x, e, i, k, a, t0 = n.now();
    w = n.tw(D.body, 1 | 4, function (node) {
      if (node.nodeType === 1) return skipEl(node) ? 2 : 3;
      return 1;
    });
    while ((x = n.nx.call(w)) && items.length < MAX_ITEMS) {
      if (x.nodeType !== 3) continue;
      e = x.parentNode;
      if (!e || e.nodeType !== 1) continue;
      k = n.dg.call(x);
      if (!ok(k)) continue;
      items.push({ t: 0, el: e, node: x, orig: k, now: null, lead: k.slice(0, k.length - k.trimStart().length), tail: k.slice(k.trimEnd().length) });
      if (n.now() - t0 > BUDGET_MS) break;
    }
    a = n.qs.call(D.body, '[title],[alt],[placeholder],[aria-label]');
    for (i = 0, k = 0; i < a.length && k < MAX_ATTRS && items.length < MAX_ITEMS; i++) {
      if (n.cl.call(a[i], '[translate="no"],.notranslate,[contenteditable="true"],[contenteditable=""]')) continue;
      if (ATTR_SKIP.test(a[i].nodeName)) continue;
      ATTRS.forEach(function (name) { // eslint-disable-line no-loop-func
        var v = n.ga.call(a[i], name);
        if (v && ok(v) && k < MAX_ATTRS) { items.push({ t: 1, el: a[i], name: name, orig: v, now: null, lead: '', tail: '' }); k++; }
      });
    }
    return viewportFirst(items);
  }
  // The strings of the batch that starts at index i: each one trimmed of surrounding space (the space
  // is put back around the translation), within the item and character budgets.
  function batch(st, i) {
    var out = [], chars = 0, it, s;
    while (i + out.length < st.items.length && out.length < BATCH_ITEMS) {
      it = st.items[i + out.length];
      s = it.orig.trim();
      if (out.length && chars + s.length > BATCH_CHARS) break;
      out.push(s); chars += s.length;
    }
    return out;
  }
  function write(it, text) {
    var s = it.lead + text + it.tail;
    if (it.t === 0) {
      if (!it.node.isConnected || n.dg.call(it.node) !== it.orig) return;
      n.ds.call(it.node, s);
    } else {
      if (n.ga.call(it.el, it.name) !== it.orig) return;
      n.sa.call(it.el, it.name, s);
    }
    it.now = s;
  }
  function undo(it) {
    if (it.now === null) return;
    if (it.t === 0) {
      if (it.node.isConnected && n.dg.call(it.node) === it.now) n.ds.call(it.node, it.orig);
    } else if (n.ga.call(it.el, it.name) === it.now) n.sa.call(it.el, it.name, it.orig);
    it.now = null;
  }
  function tok() { return Math.random().toString(36).slice(2, 10) || 'x'; }
  function state() {
    var st = D[KEY];
    return st && typeof st.tok === 'string' && Array.isArray(st.items) ? st : null;
  }

  var cur = state(), a = o.a, st, i, list, j;
  try {
    if (a === 'test') return { gather: gather, ok: ok };
    if (a === 'restore') {
      if (cur) {
        for (j = 0; j < cur.items.length; j++) undo(cur.items[j]);
        delete D[KEY];
      }
      return res('ok', null, 0, []);
    }
    if (a === 'collect') {
      if (cur) return res('on', cur, cur.items.length, []);
      if (!D.body) return res('none', null, 0, []);
      list = gather();
      if (!list.length) return res('none', null, 0, []);
      st = { tok: tok(), items: list };
      Object.defineProperty(D, KEY, { value: st, configurable: true });
      return res('ok', st, 0, batch(st, 0));
    }
    if (a === 'apply') {
      if (!cur || cur.tok !== o.k || !Array.isArray(o.s)) return res('stale', null, 0, []);
      i = o.i | 0;
      if (i < 0 || i + o.s.length > cur.items.length) return res('err', cur, 0, []);
      for (j = 0; j < o.s.length; j++) if (typeof o.s[j] === 'string' && o.s[j].length) write(cur.items[i + j], o.s[j]);
      i += o.s.length;
      return res('ok', cur, i, batch(cur, i));
    }
    return res('err', null, 0, []);
  } catch (x) {
    try { if (D[KEY]) { for (j = 0; j < D[KEY].items.length; j++) undo(D[KEY].items[j]); delete D[KEY]; } } catch (y) { /* leave the page as it is */ }
    return res('err', null, 0, []);
  }
})
