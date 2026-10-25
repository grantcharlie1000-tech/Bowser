(function () {
  var root = document.documentElement;
  var themeButton = document.querySelector("[data-theme-toggle]");
  var mock = document.querySelector("[data-mock]");
  var win = mock.querySelector("[data-window]");
  var rail = mock.querySelector("[data-rail]");
  var pill = mock.querySelector("[data-pill]");
  var omni = pill.querySelector(".bw-omni");
  var note = mock.querySelector("[data-layout-note]");
  var addButton = mock.querySelector("[data-add]");
  var addResult = mock.querySelector("[data-add-result]");

  var pills = {
    search: "html.duckduckgo.com/html/?q=windows+browser&k1=-1",
    extensions: "chromewebstore.google.com/detail/…",
    start: "Search or enter address"
  };
  var notes = {
    horizontal: "Toolbar above the tabs. That is the default horizontal layout in the source.",
    vertical: "Collapsed rail is 48px. Rest on it and the titles open over the page. That rail is on main, not in the v0.0.23 download."
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
      button.classList.toggle("is-on", on && !button.classList.contains("bw-new"));
    });
    omni.textContent = pills[name] || "";
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
  }

  rail.addEventListener("pointerenter", function () { setRail(true); });
  rail.addEventListener("pointerleave", function () { setRail(false); });
  rail.addEventListener("focusin", function () { setRail(true); });
  rail.addEventListener("focusout", function (event) {
    if (!rail.contains(event.relatedTarget)) setRail(false);
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
