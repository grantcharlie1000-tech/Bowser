(function () {
  'use strict';
  /* Bowser spatial navigation: Shift+arrow keys move the focus between the links of a page.
     Added at document creation only while Settings > Page tools > Spatial navigation is on. It runs
     in the top frame only, reads layout, focuses a link and scrolls. It makes no request and sends
     nothing out of the page. Enter on the focused link opens it and Ctrl+Enter opens it in a new tab,
     as the browser does for any focused link. */
  var W = window, D = document, KEY = Symbol.for('bowser.spatial.v1');
  if (W.top !== W || W[KEY]) return;
  var MARK = 'data-bw-spatial', MAX = 1500, SHEET = null;

  function editable(e) {
    var t = e && e.tagName;
    if (!t) return false;
    if (t === 'INPUT' || t === 'TEXTAREA' || t === 'SELECT') return true;
    return !!(e.isContentEditable);
  }
  function usable(a) {
    var r, s;
    if (!a.getAttribute('href') || a.hasAttribute('disabled') || a.getAttribute('aria-hidden') === 'true') return null;
    r = a.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) return null;
    s = W.getComputedStyle(a);
    if (s.visibility === 'hidden' || s.display === 'none' || Number(s.opacity) === 0) return null;
    if (r.bottom < -4000 || r.top > W.innerHeight + 4000 || r.right < -4000 || r.left > W.innerWidth + 4000) return null;
    return r;
  }
  // Direction d: 0 up, 1 down, 2 left, 3 right. Lower is better; null is not in that direction.
  function score(from, r, d) {
    var fx = (from.left + from.right) / 2, fy = (from.top + from.bottom) / 2, x = (r.left + r.right) / 2, y = (r.top + r.bottom) / 2, p, o;
    if (d === 0) { if (r.bottom > from.top + 2 && y >= fy) return null; p = fy - y; o = Math.abs(fx - x); }
    else if (d === 1) { if (r.top < from.bottom - 2 && y <= fy) return null; p = y - fy; o = Math.abs(fx - x); }
    else if (d === 2) { if (r.right > from.left + 2 && x >= fx) return null; p = fx - x; o = Math.abs(fy - y); }
    else { if (r.left < from.right - 2 && x <= fx) return null; p = x - fx; o = Math.abs(fy - y); }
    if (p <= 0) return null;
    return p + 2.5 * o;
  }
  function shown(e) {
    var r = e.getBoundingClientRect();
    if (r.top < 0 || r.bottom > W.innerHeight || r.left < 0 || r.right > W.innerWidth) e.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }
  function clear() {
    var l = D.querySelectorAll('[' + MARK + ']'), i;
    for (i = 0; i < l.length; i++) l[i].removeAttribute(MARK);
  }
  function mark(e) {
    clear();
    e.setAttribute(MARK, '');
    if (!SHEET) {
      try {
        SHEET = new CSSStyleSheet();
        SHEET.replaceSync('[' + MARK + ']{outline:2px solid Highlight!important;outline-offset:2px!important}');
        D.adoptedStyleSheets = D.adoptedStyleSheets.concat([SHEET]);
      } catch (x) { SHEET = null; }
    }
  }
  function move(d) {
    var links = D.querySelectorAll('a[href]'), cur = D.activeElement, from, i, r, s, best = null, bs = Infinity, vr;
    if (cur && cur.tagName !== 'A') cur = null;
    if (cur) from = cur.getBoundingClientRect();
    else {
      // Nothing focused: start from the middle of the edge the key points away from.
      vr = { left: 0, right: W.innerWidth, top: 0, bottom: W.innerHeight };
      from = d === 0 ? { left: vr.right / 2, right: vr.right / 2, top: vr.bottom, bottom: vr.bottom }
        : d === 1 ? { left: vr.right / 2, right: vr.right / 2, top: 0, bottom: 0 }
        : d === 2 ? { left: vr.right, right: vr.right, top: vr.bottom / 2, bottom: vr.bottom / 2 }
        : { left: 0, right: 0, top: vr.bottom / 2, bottom: vr.bottom / 2 };
    }
    for (i = 0; i < links.length && i < MAX; i++) {
      if (links[i] === cur) continue;
      r = usable(links[i]);
      if (!r) continue;
      s = score(from, r, d);
      if (s !== null && s < bs) { bs = s; best = links[i]; }
    }
    return best;
  }
  function onKey(e) {
    var d, best;
    if (!e.shiftKey || e.ctrlKey || e.altKey || e.metaKey || e.isComposing || e.defaultPrevented) return;
    d = e.key === 'ArrowUp' ? 0 : e.key === 'ArrowDown' ? 1 : e.key === 'ArrowLeft' ? 2 : e.key === 'ArrowRight' ? 3 : -1;
    if (d < 0 || editable(D.activeElement)) return;
    best = move(d);
    e.preventDefault();
    e.stopPropagation();
    if (best) { best.focus({ preventScroll: true }); mark(best); shown(best); return; }
    if (d < 2) W.scrollBy(0, (d === 0 ? -1 : 1) * Math.round(W.innerHeight * 0.5));
  }
  function onBlur() { clear(); }
  W.addEventListener('keydown', onKey, true);
  W.addEventListener('blur', onBlur, true);
  W[KEY] = Object.freeze({
    off: function () {
      W.removeEventListener('keydown', onKey, true);
      W.removeEventListener('blur', onBlur, true);
      clear();
      if (SHEET) { D.adoptedStyleSheets = D.adoptedStyleSheets.filter(function (s) { return s !== SHEET; }); SHEET = null; }
      try { delete W[KEY]; } catch (x) { /* frozen page object */ }
    }
  });
})()
