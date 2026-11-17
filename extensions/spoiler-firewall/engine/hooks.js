// SFW.hooks — main-world hooks. install(win) is safe to run inside a hostile page: originals are captured
// first, replacements are non-enumerable and keep name/length, installation is idempotent per window, and every
// hook body swallows its own errors so a failure can never break the page.
//
//  * Element.prototype.attachShadow  -> remember the root (same-world consumers) and mark the host with
//    data-sfw-sr (cross-world consumers: the Chrome isolated-world runtime then asks
//    chrome.dom.openOrClosedShadowRoot). The attribute is set in a microtask, NOT synchronously: custom element
//    constructors commonly call attachShadow, and adding an attribute inside a constructor makes
//    document.createElement() throw NotSupportedError.
//  * HTMLMediaElement.prototype.play -> media held by a shield (data-sfw-hold on itself or an ancestor, shadow
//    boundaries included) refuses to play: rejected promise, stays paused and muted. No audible blip.
SFW.hooks = (function () {
  const installedOn = new WeakSet();
  const roots = new WeakMap(); // host element -> ShadowRoot
  const listeners = [];
  let origAttachShadow = null;
  let origPlay = null;

  function rootOf(host) {
    return roots.get(host) || null;
  }

  // Subscribers are notified synchronously from inside attachShadow so the root can be observed before any
  // content is appended to it. Callback errors are swallowed.
  function subscribe(cb) {
    listeners.push(cb);
    return function () {
      const i = listeners.indexOf(cb);
      if (i >= 0) listeners.splice(i, 1);
    };
  }

  // attachShadow through the captured original (used by the overlay so our own roots are never reported).
  function attachClosed(el) {
    const f = origAttachShadow || Element.prototype.attachShadow;
    return f.call(el, { mode: "closed" });
  }

  function holdsMedia(el) {
    for (let n = el, hops = 0; n && hops < 400; hops++) {
      if (n.nodeType === 1 && n.hasAttribute("data-sfw-hold")) return true;
      const p = n.parentNode;
      n = p && p.nodeType === 11 && p.host ? p.host : p; // step over shadow boundaries
    }
    return false;
  }

  function define(obj, name, fn) {
    Object.defineProperty(obj, name, { value: fn, writable: true, configurable: true, enumerable: false });
  }

  function rename(fn, name, length) {
    try {
      Object.defineProperty(fn, "name", { value: name, configurable: true });
      Object.defineProperty(fn, "length", { value: length, configurable: true });
    } catch (e) { /* ignore */ }
    return fn;
  }

  function install(win) {
    win = win || globalThis;
    if (!win || installedOn.has(win)) return false;
    installedOn.add(win);
    let ok = false;
    try {
      const Elem = win.Element && win.Element.prototype;
      const setAttr = Elem && Elem.setAttribute;
      const orig = Elem && Elem.attachShadow;
      if (typeof orig === "function") {
        origAttachShadow = origAttachShadow || orig;
        const queue = win.queueMicrotask ? win.queueMicrotask.bind(win) : function (f) { Promise.resolve().then(f); };
        define(Elem, "attachShadow", rename(function attachShadow(init) {
          const root = orig.apply(this, arguments);
          try {
            if (this.localName !== "sfw-shield") {
              roots.set(this, root);
              const host = this;
              queue(function () { try { setAttr.call(host, "data-sfw-sr", "1"); } catch (e) { /* ignore */ } });
              for (let i = 0; i < listeners.length; i++) {
                try { listeners[i](host, root); } catch (e) { /* ignore */ }
              }
            }
          } catch (e) { /* ignore */ }
          return root;
        }, "attachShadow", 1));
        ok = true;
      }
    } catch (e) { /* ignore */ }
    try {
      const Media = win.HTMLMediaElement && win.HTMLMediaElement.prototype;
      const orig = Media && Media.play;
      if (typeof orig === "function") {
        origPlay = origPlay || orig;
        define(Media, "play", rename(function play() {
          try {
            if (holdsMedia(this)) {
              try { this.pause(); this.muted = true; } catch (e) { /* ignore */ }
              return Promise.reject(new win.DOMException("Playback is held by Spoiler Firewall", "NotAllowedError"));
            }
          } catch (e) { /* ignore */ }
          return orig.apply(this, arguments);
        }, "play", 0));
        ok = true;
      }
    } catch (e) { /* ignore */ }
    try {
      const Hist = win.History && win.History.prototype;
      for (const name of ["pushState", "replaceState"]) {
        const orig = Hist && Hist[name];
        if (typeof orig !== "function") continue;
        define(Hist, name, rename(function () {
          const r = orig.apply(this, arguments);
          try { win.dispatchEvent(new win.Event("sfw:nav")); } catch (e) { /* ignore */ }
          return r;
        }, name, 2));
        ok = true;
      }
    } catch (e) { /* ignore */ }
    return ok;
  }

  // Document swap watch (initial about:blank of a popup, see runtime.rebind): the opener's first touch of the NEW document goes
  // through Document.prototype, so a thin wrapper around its commonest members notifies synchronously, before the opener's
  // first write or append lands. Installed only on windows where it can happen. cb(newDoc) runs at most once per new document.
  const watching = new WeakSet();
  const DOC_METHODS = ["write", "writeln", "open", "close", "createElement", "createElementNS", "createTextNode", "createDocumentFragment", "createComment", "getElementById", "querySelector", "querySelectorAll", "getElementsByTagName", "getElementsByClassName", "importNode", "adoptNode", "execCommand"];
  const DOC_ACCESSORS = ["body", "head", "documentElement", "title", "scrollingElement", "forms", "images", "scripts", "links"];
  function watchDocument(win, current, cb) {
    win = win || globalThis;
    if (watching.has(win)) return false;
    watching.add(win);
    const DocP = win.Document && win.Document.prototype;
    if (!DocP) return false;
    const check = function (d) {
      try {
        if (d && d.nodeType === 9 && d !== current() && d.defaultView === win) cb(d);
      } catch (e) { /* never break the page */ }
    };
    for (const name of DOC_METHODS) {
      try {
        const desc = Object.getOwnPropertyDescriptor(DocP, name);
        if (!desc || typeof desc.value !== "function") continue;
        const orig = desc.value;
        define(DocP, name, rename(function () { check(this); return orig.apply(this, arguments); }, name, orig.length));
      } catch (e) { /* ignore */ }
    }
    for (const name of DOC_ACCESSORS) {
      try {
        const desc = Object.getOwnPropertyDescriptor(DocP, name);
        if (!desc || (!desc.get && !desc.set)) continue;
        const g = desc.get, s = desc.set;
        Object.defineProperty(DocP, name, {
          get: g ? rename(function () { check(this); return g.call(this); }, "get " + name, 0) : undefined,
          set: s ? rename(function (v) { check(this); return s.call(this, v); }, "set " + name, 1) : undefined,
          enumerable: desc.enumerable, configurable: true,
        });
      } catch (e) { /* ignore */ }
    }
    return true;
  }

  return { install, rootOf, subscribe, attachClosed, holdsMedia, watchDocument };
})();
