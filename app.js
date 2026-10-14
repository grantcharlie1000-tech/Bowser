/* Bowser landing page.
   1. bowserWindow(): a drawn Bowser window, rendered into every [data-bw] mount.
   2. The highlight row: auto-advance, progress fill, pause, and the layout crossfade in whichever
      beat holds [data-frame]s. */
(function () {
  "use strict";

  /* ---------- Icons (24px grid, stroked) ---------- */
  var P = {
    back: '<path d="M19 12H5m6-6-6 6 6 6"/>',
    fwd: '<path d="M5 12h14m-6-6 6 6-6 6"/>',
    reload: '<path d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3M19.5 4.5v4h-4"/>',
    lock: '<rect x="5.5" y="10.5" width="13" height="9" rx="2"/><path d="M8.5 10.5V8a3.5 3.5 0 0 1 7 0v2.5"/>',
    shield: '<path d="M12 3.5 5.5 6v5.5c0 4 2.8 7.4 6.5 8.8 3.7-1.4 6.5-4.8 6.5-8.8V6Z"/><path d="m9.2 12 2 2 3.8-3.8"/>',
    dots: '<circle cx="5.5" cy="12" r="1.1"/><circle cx="12" cy="12" r="1.1"/><circle cx="18.5" cy="12" r="1.1"/>',
    home: '<path d="M4.5 10.5 12 4.5l7.5 6V19a.5.5 0 0 1-.5.5h-4.5v-5h-5v5H5a.5.5 0 0 1-.5-.5Z"/>',
    sliders: '<path d="M4.5 8h9m4 0h2m-15 8h2m4 0h9"/><circle cx="15.5" cy="8" r="2"/><circle cx="8.5" cy="16" r="2"/>',
    case: '<rect x="4" y="8" width="16" height="11" rx="2"/><path d="M9 8V6.5A1.5 1.5 0 0 1 10.5 5h3A1.5 1.5 0 0 1 15 6.5V8M4 13h16"/>',
    plusbox: '<rect x="4.5" y="4.5" width="15" height="15" rx="3"/><path d="M12 8.5v7M8.5 12h7"/>',
    clock: '<circle cx="12" cy="12" r="7.5"/><path d="M12 8v4.2l2.8 1.8"/>',
    heart: '<path d="M12 19s-7-4.4-7-9.2A3.8 3.8 0 0 1 12 8a3.8 3.8 0 0 1 7 1.8C19 14.6 12 19 12 19Z"/>',
    plus: '<path d="M12 5.5v13M5.5 12h13"/>',
    x: '<path d="m7 7 10 10M17 7 7 17"/>',
    chev: '<path d="m7 10 5 5 5-5"/>',
    panel: '<rect x="4.5" y="5.5" width="15" height="13" rx="2"/><path d="M10 5.5v13"/>',
    tab: '<rect x="4" y="6" width="16" height="13" rx="2"/><path d="M4 10h16M9 6v4"/>',
    priv: '<path d="M3.5 11h17"/><path d="M6 11c0-2.5.7-6 6-6s6 3.5 6 6"/><rect x="4.5" y="13" width="6" height="4.5" rx="2.2"/><rect x="13.5" y="13" width="6" height="4.5" rx="2.2"/><path d="M10.5 15h3"/>',
    window: '<rect x="4" y="5" width="16" height="14" rx="2"/><path d="M4 9h16"/>',
    star: '<path d="m12 4.8 2.2 4.5 5 .7-3.6 3.5.9 5-4.5-2.4-4.5 2.4.9-5L4.8 10l5-.7Z"/>',
    puzzle: '<path d="M9 5.5h3.5V4a1.8 1.8 0 0 1 3.6 0v1.5h2.4v4.4H20a1.8 1.8 0 0 1 0 3.6h-1.5v5H14v-1.4a1.8 1.8 0 0 0-3.6 0v1.4H5.5v-4.4H7a1.8 1.8 0 0 0 0-3.6H5.5V5.5Z"/>',
    brush: '<path d="M14.5 5.5 18.5 9.5 11 17l-4-4Z"/><path d="M7 13c-2 .5-2.5 2-2.5 3.5 0 1-.5 2-1 2.5 3 .5 6-.5 7.5-2"/>',
    gear: '<circle cx="12" cy="12" r="3"/><path d="M12 3.5v2.2m0 12.6v2.2M3.5 12h2.2m12.6 0h2.2M6 6l1.6 1.6m8.8 8.8L18 18M6 18l1.6-1.6m8.8-8.8L18 6"/>',
    exit: '<path d="M10 5H6a1 1 0 0 0-1 1v12a1 1 0 0 0 1 1h4m4-11 4 4-4 4m4-4H9"/>',
    cookie: '<path d="M19.5 12.5A7.5 7.5 0 1 1 11.5 4.5a3 3 0 0 0 3.5 3.5 3 3 0 0 0 4.5 4.5Z"/><circle cx="9" cy="10" r=".6"/><circle cx="14" cy="15" r=".6"/><circle cx="9.5" cy="15.5" r=".6"/>',
    eyeoff: '<path d="M4 4l16 16M10 6.3A8.6 8.6 0 0 1 12 6c5 0 8 6 8 6a14 14 0 0 1-2.4 3.1M14.1 14.2A3 3 0 0 1 9.8 9.9M6.4 7.9A14 14 0 0 0 4 12s3 6 8 6a8 8 0 0 0 3.5-.8"/>',
    folder: '<path d="M4 7.5A1.5 1.5 0 0 1 5.5 6h4l2 2h7a1.5 1.5 0 0 1 1.5 1.5v8a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 17.5Z"/>',
    file: '<path d="M7 4h6.5L18 8.5V19a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1Z"/><path d="M13 4v5h5"/>',
    min: '<path d="M6 12h12"/>',
    max: '<rect x="6.5" y="6.5" width="11" height="11" rx="1"/>'
  };
  function ico(name, cls) {
    return '<svg class="bw-i' + (cls ? " " + cls : "") + '" viewBox="0 0 24 24" aria-hidden="true">' + P[name] + "</svg>";
  }
  function bone(cls) {
    return '<svg class="' + cls + '" viewBox="0 0 100 100" aria-hidden="true"><use href="#bone"/></svg>';
  }

  /* ---------- Tab sets ---------- */
  var TABS = {
    reading: [
      { t: "A slower morning", f: "f-sky", u: "reading.example/slow-mornings" },
      { t: "Notes", f: "f-amber", u: "notes.example" },
      { t: "Weather", f: "f-mint", u: "weather.example" }
    ],
    source: [
      { t: "grantcharlie1000-tech/Bowser", f: "f-ink", u: "github.com/grantcharlie1000-tech/Bowser" },
      { t: "A slower morning", f: "f-sky", u: "reading.example/slow-mornings" },
      { t: "Notes", f: "f-amber", u: "notes.example" }
    ],
    "new": [
      { t: "New tab", f: "f-bone", u: "" }
    ],
    "private": [
      { t: "A slower morning", f: "f-sky", u: "reading.example/slow-mornings" },
      { t: "Private tab", f: "f-priv", u: "", priv: true }
    ]
  };

  function fav(tab) {
    if (tab.f === "f-priv") return '<span class="bw-fav f-priv">' + ico("priv") + "</span>";
    if (tab.f === "f-bone") return '<span class="bw-fav f-bone">' + bone("") + "</span>";
    return '<span class="bw-fav ' + tab.f + '"></span>';
  }

  function tabIsland(tab, on) {
    return '<span class="bw-tab' + (on ? " is-on" : "") + (tab.priv ? " is-priv" : "") + '">' +
      fav(tab) + '<span class="bw-tt">' + tab.t + "</span>" + (on ? ico("x", "bw-x") : "") + "</span>";
  }

  function strip(tabs, active) {
    var html = '<div class="bw-strip">';
    tabs.forEach(function (t, i) { html += tabIsland(t, i === active); });
    return html + '<span class="bw-newtab">' + ico("plus") + "</span></div>";
  }

  function nav() {
    return '<div class="bw-nav"><span class="bw-btn is-dim">' + ico("back") + '</span><span class="bw-btn is-dim">' +
      ico("fwd") + '</span><span class="bw-btn">' + ico("reload") + "</span></div>";
  }

  function omni(tab, opts) {
    var text = tab.u
      ? '<span class="bw-url">' + tab.u.replace(/^([^/]+)/, "<b>$1</b>") + "</span>"
      : '<span class="bw-url is-hint">Search or enter address</span>';
    var lead = tab.priv
      ? '<span class="bw-chip">' + ico("priv") + "Private</span>"
      : tab.u ? ico("lock", "bw-lock") : "";
    return '<div class="bw-omni' + (opts.narrow ? " is-narrow" : "") + '">' + lead + text +
      '<span class="bw-omni-end">' + ico("star") + "</span></div>";
  }

  function actions(opts) {
    return '<div class="bw-acts"><span class="bw-btn is-shield">' + ico("shield") + '</span><span class="bw-btn' +
      (opts.extensions ? " is-pressed" : "") + '">' + ico("puzzle") + '</span><span class="bw-btn' +
      (opts.menu ? " is-pressed" : "") + '">' + ico("dots") + "</span></div>";
  }

  function controls() {
    return '<div class="bw-ctl"><span>' + ico("min") + "</span><span>" + ico("max") + '</span><span class="is-close">' + ico("x") + "</span></div>";
  }

  function rail() {
    return '<aside class="bw-rail">' +
      '<span class="bw-ri">' + ico("home") + "</span>" +
      '<span class="bw-ri">' + ico("sliders") + "</span>" +
      '<span class="bw-ri is-on">' + ico("case") + "</span>" +
      '<span class="bw-ri">' + ico("plusbox") + "</span>" +
      '<span class="bw-ri">' + ico("clock") + "</span>" +
      '<span class="bw-rail-gap"></span>' +
      '<span class="bw-ri">' + ico("heart") + "</span>" +
      '<span class="bw-ri">' + ico("dots") + "</span>" +
      "</aside>";
  }

  function sidebar(tabs, active) {
    var html = '<aside class="bw-side"><div class="bw-ws">' + ico("case") + "<b>Reading</b>" + ico("chev", "bw-ws-chev") +
      '<span class="bw-ws-end">' + ico("panel") + '</span></div><div class="bw-row-new">' + ico("plus") + "New tab</div>";
    tabs.forEach(function (t, i) {
      html += '<div class="bw-vtab' + (i === active ? " is-on" : "") + '">' + fav(t) + '<span class="bw-tt">' + t.t + "</span>" +
        (i === active ? ico("x", "bw-x") : "") + "</div>";
    });
    html += '<div class="bw-vtab is-settings">' + ico("gear", "bw-fav-i") + '<span class="bw-tt">Settings</span></div>';
    return html + "</aside>";
  }

  /* ---------- Pages ---------- */
  var LANDSCAPE =
    '<svg class="pg-art" viewBox="0 0 400 180" preserveAspectRatio="xMidYMid slice" aria-hidden="true">' +
    '<defs><linearGradient id="sky" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#c9d8fb"/><stop offset="1" stop-color="#f3ecf4"/></linearGradient></defs>' +
    '<rect width="400" height="180" fill="url(#sky)"/>' +
    '<circle cx="282" cy="74" r="26" fill="#fff6e4"/>' +
    '<path d="M0 120c60-30 110-34 170-12s120 10 160-16 70-8 70-8v96H0Z" fill="#9fb4ea"/>' +
    '<path d="M0 142c70-22 130-18 190 0s150 8 210-14v52H0Z" fill="#7189d6"/>' +
    '<path d="M0 162c90-14 170-10 250 2s110 0 150-8v24H0Z" fill="#4c66c4"/></svg>';

  function pageArticle() {
    return '<div class="pg pg-article">' +
      '<div class="pg-top"><span class="pg-brand"></span><i></i><i></i><i></i><span class="pg-pill"></span></div>' +
      '<div class="pg-cols"><div class="pg-main">' +
      '<i class="sk kicker"></i><i class="sk h"></i><i class="sk h short"></i>' +
      '<i class="sk l"></i><i class="sk l mid"></i>' +
      '<div class="pg-fig">' + LANDSCAPE + "</div>" +
      '<i class="sk l"></i><i class="sk l"></i><i class="sk l mid"></i></div>' +
      '<div class="pg-aside"><i class="sk cap"></i><div class="pg-card"></div><i class="sk l"></i><i class="sk l mid"></i>' +
      '<i class="sk rule"></i><i class="sk cap"></i><i class="sk b"></i><i class="sk b mid"></i><i class="sk l short"></i></div></div></div>';
  }

  /* An article with two ad slots. Each slot holds the ad and its blocked mark; the window
     shows the mark (.is-clean), and a .is-reveal window plays the ad giving way to it. */
  function adSlot(cls) {
    return '<div class="ad ' + cls + '"><div class="ad-live"><span class="ad-tag">Ad</span><i></i><i></i><b></b></div>' +
      '<div class="ad-gone">' + ico("shield") + "<span>Ad blocked</span></div></div>";
  }

  function pageAds() {
    return '<div class="pg pg-article pg-ads">' +
      '<div class="pg-top"><span class="pg-brand"></span><i></i><i></i><i></i><span class="pg-pill"></span></div>' +
      '<div class="pg-cols"><div class="pg-main">' + adSlot("ad-banner") +
      '<i class="sk kicker"></i><i class="sk h"></i><i class="sk h short"></i>' + adSlot("ad-inline") +
      '<i class="sk l"></i><i class="sk l mid"></i><i class="sk l"></i><i class="sk l"></i><i class="sk l mid"></i></div>' +
      '<div class="pg-aside"><i class="sk cap"></i>' + adSlot("ad-box") + '<i class="sk l"></i><i class="sk l mid"></i>' +
      '<i class="sk rule"></i><i class="sk cap"></i><i class="sk b"></i><i class="sk b mid"></i></div></div></div>';
  }

  function pageBlank() {
    return '<div class="pg pg-blank">' + bone("pg-blank-mark") + '<div class="pg-blank-field">' +
      ico("star") + "<span>Search or enter address</span></div>" +
      '<div class="pg-blank-tiles"><i></i><i></i><i></i><i></i></div></div>';
  }

  function pageSource() {
    var rows = [["folder", "blocker"], ["file", "blocker.rs"], ["file", "blocklist.txt", true], ["file", "extensions.rs"],
      ["file", "private.rs"], ["file", "shield.rs"]];
    var list = rows.map(function (r) {
      return '<div class="src-row' + (r[2] ? " is-hit" : "") + '">' + ico(r[0], r[0] === "folder" ? "is-dir" : "") + "<span>" + r[1] +
        '</span><i class="sk l"></i><i class="sk t"></i></div>';
    }).join("");
    return '<div class="pg pg-source">' +
      '<div class="src-head"><span class="src-avatar"></span><span class="src-name">grantcharlie1000-tech / <b>Bowser</b></span><span class="src-badge">Public</span></div>' +
      '<div class="src-nav"><span class="is-on">' + ico("panel") + 'Code</span><i class="sk"></i><i class="sk"></i><i class="sk"></i></div>' +
      '<div class="src-cols"><div class="src-files"><div class="src-files-top">' + ico("folder", "is-dir") + '<span class="src-path">Bowser / <b>src</b></span><i class="sk t"></i></div>' +
      list + '</div><div class="src-about"><i class="sk cap"></i><i class="sk l"></i><i class="sk l mid"></i><i class="sk l short"></i><i class="sk rule"></i><i class="sk cap"></i><i class="sk l mid"></i></div></div></div>';
  }

  function pagePrivate() {
    var rows = [
      ["shield", "Ads and trackers", "Blocked, with strict tracking prevention"],
      ["clock", "Browsing", "Not recorded"],
      ["cookie", "Cookies", "In memory until the last private tab closes"]
    ];
    return '<div class="pg pg-private"><div class="pv-glow"></div><div class="pv-mark">' + ico("priv") + "</div>" +
      '<p class="pv-title">You’re browsing privately</p>' +
      '<div class="pv-card">' + rows.map(function (r) {
        return '<div class="pv-row' + (r[0] === "shield" ? " is-lead" : "") + '"><span class="pv-ico">' + ico(r[0]) + "</span><b>" + r[1] + "</b><span>" + r[2] + "</span></div>";
      }).join("") + "</div></div>";
  }

  function menu() {
    function item(icon, label) { return '<div class="mn-row">' + ico(icon) + "<span>" + label + "</span></div>"; }
    return '<div class="bw-menu" role="presentation">' +
      item("tab", "New tab") + item("priv", "New private tab") + item("window", "New window") +
      '<i class="mn-sep"></i>' +
      item("clock", "History") + item("star", "Bookmarks") + item("puzzle", "Extensions") +
      '<i class="mn-sep"></i>' +
      '<div class="mn-row mn-toggle">' + ico("shield") + "<span>Block ads and trackers</span>" +
      '<span class="mn-switch" aria-label="On"><i></i></span></div>' +
      '<i class="mn-sep"></i>' +
      item("brush", "Customize Bowser") + item("gear", "Settings") +
      '<i class="mn-sep"></i>' + item("exit", "Exit") + "</div>";
  }

  /* The extensions menu: unpacked folders, each with its manifest version and a switch. */
  function extMenu() {
    var exts = [["Reading ruler", "MV3", "x-a"], ["Quiet tabs", "MV3", "x-b"], ["Word count", "MV2", "x-c"]];
    return '<div class="bw-menu bw-ext" role="presentation"><div class="ex-head">' + ico("puzzle") + "<span>Extensions</span></div>" +
      exts.map(function (e) {
        return '<div class="ex-row"><span class="ex-ico ' + e[2] + '"></span><span class="ex-name"><b>' + e[0] +
          '</b><small>' + ico("folder") + "Unpacked folder</small></span>" + '<span class="ex-mv">' + e[1] + "</span>" +
          '<span class="mn-switch"><i></i></span></div>';
      }).join("") +
      '<i class="mn-sep"></i><div class="mn-row ex-load">' + ico("folder") + "<span>Load unpacked folder</span></div></div>";
  }

  /* ---------- The window ---------- */
  function bowserWindow(o) {
    var tabs = TABS[o["private"] ? "private" : o.tabs || (o.page === "source" ? "source" : "reading")];
    var active = o["private"] ? 1 : (o.tab || 0);
    var cur = tabs[active];
    var page = { article: pageArticle, ads: pageAds, blank: pageBlank, source: pageSource, "private": pagePrivate }[o.page]();
    var main;

    if (o.layout === "vertical") {
      main = '<div class="bw-row bw-row-tool">' + nav() + omni(cur, o) + actions(o) + controls() + "</div>";
    } else if (o.layout === "tabs-above") {
      main = '<div class="bw-row bw-row-tabs">' + strip(tabs, active) + controls() + "</div>" +
        '<div class="bw-row bw-row-tool">' + nav() + omni(cur, o) + actions(o) + "</div>";
    } else if (o.layout === "toolbar-above") {
      main = '<div class="bw-row bw-row-tool">' + nav() + omni(cur, o) + actions(o) + controls() + "</div>" +
        '<div class="bw-row bw-row-tabs is-under">' + strip(tabs, active) + "</div>";
    } else {
      main = '<div class="bw-row bw-row-tool is-single">' + nav() + omni(cur, { narrow: true }) + actions(o) + strip(tabs, active) + controls() + "</div>";
    }

    return '<div class="bw bw--' + o.layout + (o["private"] ? " is-private" : "") + (o.reveal ? " is-reveal" : " is-clean") + '">' + rail() +
      (o.layout === "vertical" ? sidebar(tabs, active) : "") +
      '<div class="bw-main">' + main + '<div class="bw-page">' + page + "</div></div>" +
      (o.menu ? menu() : "") + (o.extensions ? extMenu() : "") + "</div>";
  }

  Array.prototype.forEach.call(document.querySelectorAll("[data-bw]"), function (mount) {
    var opts = {};
    try { opts = JSON.parse(mount.getAttribute("data-bw")); } catch (e) { return; }
    mount.outerHTML = bowserWindow(opts);
  });

  /* ---------- Highlight row ---------- */
  var root = document.querySelector("[data-highlights]");
  if (!root) return;
  var panels = [].slice.call(root.querySelectorAll("[data-panel]"));
  var scenes = [].slice.call(root.querySelectorAll("[data-scene]"));
  var dots = [].slice.call(root.querySelectorAll(".hl-dot"));
  var cycle = 0;
  scenes.forEach(function (s, i) { if (s.querySelector("[data-frame]")) cycle = i; });
  var frames = [].slice.call(scenes[cycle].querySelectorAll("[data-frame]"));
  var legend = [].slice.call(root.querySelectorAll("[data-legend]"));
  var pauseBtn = root.querySelector(".hl-pause");
  var reduce = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  var index = 0, elapsed = 0, last = 0, frame = -1;
  var paused = reduce, visible = true;

  function duration() { return +panels[index].getAttribute("data-duration") || 8000; }

  function setFrame(n) {
    if (n === frame) return;
    frame = n;
    frames.forEach(function (f, i) { f.classList.toggle("is-on", i === n); });
    legend.forEach(function (l, i) { l.classList.toggle("is-on", i === n); });
  }

  function show(n) {
    index = (n + panels.length) % panels.length;
    elapsed = 0;
    panels.forEach(function (p, i) { p.classList.toggle("is-on", i === index); });
    scenes.forEach(function (s, i) { s.classList.toggle("is-on", i === index); });
    dots.forEach(function (d, i) {
      d.classList.toggle("is-on", i === index);
      d.classList.toggle("is-done", i < index);
      if (i === index) d.setAttribute("aria-current", "true"); else d.removeAttribute("aria-current");
      d.firstChild.style.transform = "scaleX(0)";
    });
    if (index === cycle) setFrame(0);
  }

  function tick(now) {
    var dt = last ? Math.min(now - last, 100) : 0;
    last = now;
    if (!paused && visible) {
      elapsed += dt;
      var d = duration();
      if (elapsed >= d) {
        show(index + 1);
      } else {
        dots[index].firstChild.style.transform = "scaleX(" + (elapsed / d).toFixed(4) + ")";
        if (index === cycle) setFrame(Math.min(frames.length - 1, Math.floor(elapsed / (d / frames.length))));
      }
    }
    requestAnimationFrame(tick);
  }

  function setPaused(p) {
    paused = p;
    root.classList.toggle("is-paused", p);
    pauseBtn.setAttribute("aria-pressed", p ? "true" : "false");
    pauseBtn.setAttribute("aria-label", p ? "Play highlights" : "Pause highlights");
  }

  dots.forEach(function (d, i) { d.addEventListener("click", function () { show(i); }); });
  pauseBtn.addEventListener("click", function () { setPaused(!paused); });
  legend.forEach(function (l, i) {
    l.addEventListener("click", function () { show(cycle); setPaused(true); setFrame(i); });
  });

  if ("IntersectionObserver" in window) {
    new IntersectionObserver(function (entries) { visible = entries[0].isIntersecting; }, { threshold: 0.35 }).observe(root);
  }

  setPaused(paused);
  show(0);
  requestAnimationFrame(tick);
})();
