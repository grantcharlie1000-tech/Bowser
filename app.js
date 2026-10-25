(function () {
  var root = document.documentElement;
  var themeButton = document.querySelector("[data-theme-toggle]");
  var mock = document.querySelector("[data-mock]");
  var win = mock.querySelector("[data-window]");
  var rail = mock.querySelector("[data-rail]");
  var flyout = mock.querySelector("[data-flyout]");
  var pill = mock.querySelector("[data-pill]");
  var note = mock.querySelector("[data-layout-note]");
  var addButton = mock.querySelector("[data-add]");
  var addResult = mock.querySelector("[data-add-result]");

  var pills = {
    search: "html.duckduckgo.com/html/?q=windows+browser&k1=-1",
    extensions: "chromewebstore.google.com/detail/…",
    start: "Search or enter an address"
  };
  var notes = {
    horizontal: "Rounded tabs and a centred address pill. This is the quiet, Safari-style bar.",
    vertical: "Hover the icon rail. Titles open over the page. That hover rail is pull request 96, not the current release."
  };

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
    if (color) color.setAttribute("content", dark ? "#121211" : "#f3f2ee");
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

  function show(name) {
    mock.querySelectorAll("[data-view]").forEach(function (panel) {
      panel.hidden = panel.getAttribute("data-view") !== name;
    });
    mock.querySelectorAll("[data-show]").forEach(function (button) {
      var on = button.getAttribute("data-show") === name;
      if (button.getAttribute("role") === "tab") button.setAttribute("aria-selected", on ? "true" : "false");
      button.classList.toggle("is-on", on);
    });
    pill.textContent = pills[name] || "";
    pill.classList.toggle("is-empty", name === "start");
  }

  mock.querySelectorAll("[data-show]").forEach(function (button) {
    button.addEventListener("click", function () {
      show(button.getAttribute("data-show"));
    });
  });

  function setLayout(name) {
    win.setAttribute("data-layout", name);
    mock.querySelectorAll("[data-layout-btn]").forEach(function (button) {
      var on = button.getAttribute("data-layout-btn") === name;
      button.setAttribute("aria-pressed", on ? "true" : "false");
      button.classList.toggle("is-on", on);
    });
    note.textContent = notes[name] || "";
    if (name !== "vertical") setRail(false);
  }

  mock.querySelectorAll("[data-layout-btn]").forEach(function (button) {
    button.addEventListener("click", function () {
      setLayout(button.getAttribute("data-layout-btn"));
    });
  });

  function setRail(open) {
    if (win.getAttribute("data-layout") !== "vertical") open = false;
    win.classList.toggle("is-rail-open", open);
    rail.setAttribute("aria-expanded", open ? "true" : "false");
    flyout.inert = !open;
  }

  function insideRail(node) {
    return node && (rail.contains(node) || flyout.contains(node));
  }

  rail.addEventListener("pointerenter", function () { setRail(true); });
  flyout.addEventListener("pointerenter", function () { setRail(true); });
  rail.addEventListener("pointerleave", function (event) {
    if (!insideRail(event.relatedTarget)) setRail(false);
  });
  flyout.addEventListener("pointerleave", function (event) {
    if (!insideRail(event.relatedTarget)) setRail(false);
  });
  rail.addEventListener("focusin", function () { setRail(true); });
  flyout.addEventListener("focusin", function () { setRail(true); });
  rail.addEventListener("focusout", function (event) {
    if (!insideRail(event.relatedTarget)) setRail(false);
  });
  flyout.addEventListener("focusout", function (event) {
    if (!insideRail(event.relatedTarget)) setRail(false);
  });
  rail.addEventListener("click", function () {
    if (window.matchMedia("(hover: none)").matches) {
      setRail(!win.classList.contains("is-rail-open"));
    }
  });

  addButton.addEventListener("click", function () {
    addResult.hidden = false;
  });

  show("search");

  document.querySelectorAll(".faq details").forEach(function (item) {
    item.addEventListener("toggle", function () {
      if (!item.open) return;
      document.querySelectorAll(".faq details").forEach(function (other) {
        if (other !== item) other.open = false;
      });
    });
  });
})();
