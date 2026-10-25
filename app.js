(function () {
  var root = document.documentElement;
  var themeButton = document.querySelector("[data-theme-toggle]");

  function appliedTheme() {
    var chosen = root.getAttribute("data-theme");
    if (chosen === "dark" || chosen === "light") return chosen;
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }

  function paintTheme() {
    var dark = appliedTheme() === "dark";
    themeButton.setAttribute("aria-pressed", dark ? "true" : "false");
    themeButton.textContent = dark ? "Light" : "Dark";
    var color = document.querySelector('meta[name="theme-color"]');
    if (color) color.setAttribute("content", dark ? "#111111" : "#ffffff");
  }

  themeButton.addEventListener("click", function () {
    var next = appliedTheme() === "dark" ? "light" : "dark";
    root.setAttribute("data-theme", next);
    try { localStorage.setItem("bowser-theme", next); } catch (e) {}
    paintTheme();
  });
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", function () {
    if (!root.hasAttribute("data-theme")) paintTheme();
  });
  paintTheme();

  document.querySelectorAll(".faq details").forEach(function (item) {
    item.addEventListener("toggle", function () {
      if (!item.open) return;
      document.querySelectorAll(".faq details").forEach(function (other) {
        if (other !== item) other.open = false;
      });
    });
  });

  var demo = document.querySelector("[data-rail-demo]");
  var rail = demo ? demo.querySelector("[data-rail]") : null;
  var railHeld = false;
  var railTimer = 0;

  function setRail(open) {
    if (!demo) return;
    demo.classList.toggle("is-rail-open", open);
    if (rail) rail.setAttribute("aria-expanded", open ? "true" : "false");
  }

  if (rail) {
    rail.addEventListener("pointerenter", function () { railHeld = true; setRail(true); });
    rail.addEventListener("pointerdown", function () { railHeld = true; });
    rail.addEventListener("pointerleave", function () { railHeld = false; setRail(false); });
    rail.addEventListener("focusin", function () { setRail(true); });
    rail.addEventListener("focusout", function (event) {
      if (!rail.contains(event.relatedTarget)) setRail(false);
    });
  }

  function show(name) {
    document.querySelectorAll("[data-panel]").forEach(function (panel) {
      panel.hidden = panel.getAttribute("data-panel") !== name;
    });
    document.querySelectorAll("[data-tab]").forEach(function (tab) {
      var on = tab.getAttribute("data-tab") === name;
      tab.setAttribute("aria-selected", on ? "true" : "false");
    });
    window.clearTimeout(railTimer);
    if (name === "vertical") {
      setRail(true);
      railTimer = window.setTimeout(function () {
        if (railHeld || (rail && rail.matches(":hover"))) return;
        setRail(false);
      }, 2200);
    } else {
      setRail(false);
    }
  }

  document.querySelectorAll("[data-tab]").forEach(function (tab) {
    tab.addEventListener("click", function () {
      show(tab.getAttribute("data-tab"));
    });
  });
  show("tabs");
})();
