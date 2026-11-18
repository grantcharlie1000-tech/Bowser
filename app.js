/* Bowser website: theme toggle, the compare slider and the tab switchers.
   Every switcher ([data-switch]) pairs tabs ([data-switch-item]) with scenes ([data-switch-scene]);
   with [data-autoplay] it advances on its own and cycles any [data-frame]s in a scene. */
(function () {
  "use strict";
  var reduce = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  /* ---------- Theme toggle ---------- */
  var themeBtn = document.getElementById("theme-toggle");
  var docEl = document.documentElement;
  function isDark() {
    var t = docEl.getAttribute("data-theme");
    if (t) return t === "dark";
    return window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches;
  }
  function syncThemeLabel() {
    if (themeBtn) themeBtn.setAttribute("aria-label", isDark() ? "Switch to light theme" : "Switch to dark theme");
  }
  if (themeBtn) {
    themeBtn.addEventListener("click", function () {
      var next = isDark() ? "light" : "dark";
      docEl.setAttribute("data-theme", next);
      try { localStorage.setItem("bowser-theme", next); } catch (e) {}
      syncThemeLabel();
    });
    syncThemeLabel();
  }

  /* ---------- Nav hairline once scrolled ---------- */
  var navEl = document.querySelector(".nav");
  if (navEl) {
    var onScroll = function () { navEl.classList.toggle("is-stuck", window.scrollY > 8); };
    window.addEventListener("scroll", onScroll, { passive: true });
    onScroll();
  }

  /* ---------- Compare slider ---------- */
  Array.prototype.forEach.call(document.querySelectorAll("[data-compare]"), function (cmp) {
    var range = cmp.querySelector(".cmp-range");
    var set = function () { cmp.style.setProperty("--x", range.value + "%"); };
    range.addEventListener("input", function () { cmp.dataset.touched = "1"; set(); });
    set();
    /* A short nudge the first time it scrolls into view, so it reads as draggable. */
    if (!reduce && "IntersectionObserver" in window) {
      var io = new IntersectionObserver(function (entries) {
        if (!entries[0].isIntersecting) return;
        io.disconnect();
        var start = null, from = 50;
        var step = function (now) {
          if (start === null) start = now;
          var t = Math.min(1, (now - start) / 1600);
          if (cmp.dataset.touched) return;
          range.value = (from + Math.sin(t * Math.PI * 2) * 16).toFixed(1);
          set();
          if (t < 1) requestAnimationFrame(step);
        };
        setTimeout(function () { requestAnimationFrame(step); }, 300);
      }, { threshold: 0.6 });
      io.observe(cmp);
    }
    range.addEventListener("pointerdown", function () { cmp.dataset.touched = "1"; });
    range.addEventListener("keydown", function () { cmp.dataset.touched = "1"; });
  });

  /* ---------- Switchers: the feature tour and the values pills ---------- */
  Array.prototype.forEach.call(document.querySelectorAll("[data-switch]"), function (root) {
    var items = [].slice.call(root.querySelectorAll("[data-switch-item]"));
    var scenes = [].slice.call(root.querySelectorAll("[data-switch-scene]"));
    var groups = {};
    scenes.forEach(function (s) {
      var key = s.classList.contains("scene-text") ? "text" : "plate";
      (groups[key] = groups[key] || []).push(s);
    });
    var auto = root.hasAttribute("data-autoplay");
    var pauseBtn = root.querySelector(".tour-pause");
    var frames = [], cycle = -1;
    (groups.plate || []).forEach(function (s, i) {
      var f = s.querySelectorAll("[data-frame]");
      if (f.length) { frames = [].slice.call(f); cycle = i; }
    });
    var index = 0, elapsed = 0, last = 0, frame = -1, paused = reduce, visible = true;

    function setFrame(n) {
      if (n === frame) return;
      frame = n;
      frames.forEach(function (f, i) { f.classList.toggle("is-on", i === n); });
    }
    function show(n) {
      index = (n + items.length) % items.length;
      elapsed = 0;
      items.forEach(function (it, i) {
        it.classList.toggle("is-on", i === index);
        it.setAttribute("aria-selected", i === index ? "true" : "false");
        it.tabIndex = i === index ? 0 : -1;
        var bar = it.querySelector(".tour-bar i");
        if (bar) bar.style.transform = "scaleX(0)";
      });
      Object.keys(groups).forEach(function (k) {
        groups[k].forEach(function (s, i) { s.classList.toggle("is-on", i === index); });
      });
      if (index === cycle) setFrame(0);
      /* Keep the active tab in view when the tab row scrolls (phones). */
      var row = items[index].parentElement;
      if (row.scrollWidth > row.clientWidth + 1) {
        row.scrollTo({ left: items[index].offsetLeft - 24, behavior: reduce ? "auto" : "smooth" });
      }
    }
    items.forEach(function (it, i) {
      it.addEventListener("click", function () { show(i); });
      it.addEventListener("keydown", function (e) {
        var d = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 }[e.key];
        if (!d) return;
        e.preventDefault();
        show(index + d);
        items[index].focus();
      });
    });
    show(0);
    if (!auto) return;

    function setPaused(p) {
      paused = p;
      if (!pauseBtn) return;
      pauseBtn.setAttribute("aria-pressed", p ? "true" : "false");
      pauseBtn.setAttribute("aria-label", p ? "Play the feature tour" : "Pause the feature tour");
    }
    function duration() { return +items[index].getAttribute("data-duration") || 7000; }
    function tick(now) {
      var dt = last ? Math.min(now - last, 100) : 0;
      last = now;
      if (!paused && visible) {
        elapsed += dt;
        var d = duration();
        if (elapsed >= d) show(index + 1);
        else {
          var bar = items[index].querySelector(".tour-bar i");
          if (bar) bar.style.transform = "scaleX(" + (elapsed / d).toFixed(4) + ")";
          if (index === cycle) setFrame(Math.min(frames.length - 1, Math.floor(elapsed / (d / frames.length))));
        }
      }
      requestAnimationFrame(tick);
    }
    if (pauseBtn) pauseBtn.addEventListener("click", function () { setPaused(!paused); });
    if ("IntersectionObserver" in window) {
      new IntersectionObserver(function (entries) { visible = entries[0].isIntersecting; }, { threshold: 0.3 }).observe(root);
    }
    setPaused(paused);
    requestAnimationFrame(tick);
  });
})();
