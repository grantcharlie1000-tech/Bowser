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

  document.querySelectorAll(".faq details").forEach(function (item) {
    item.addEventListener("toggle", function () {
      if (!item.open) return;
      document.querySelectorAll(".faq details").forEach(function (other) {
        if (other !== item) other.open = false;
      });
    });
  });

  var motion = window.matchMedia("(prefers-reduced-motion: reduce)");
  var demo = document.querySelector("[data-rail-demo]");
  var rail = demo ? demo.querySelector("[data-rail]") : null;
  var railHeld = false;

  function setRail(open) {
    if (!demo) return;
    demo.classList.toggle("is-rail-open", open);
    if (rail) rail.setAttribute("aria-expanded", open ? "true" : "false");
  }

  if (rail) {
    rail.addEventListener("pointerenter", function () {
      railHeld = true;
      setRail(true);
    });
    rail.addEventListener("pointerdown", function () { railHeld = true; });
    rail.addEventListener("pointerleave", function () {
      railHeld = false;
      setRail(false);
    });
    rail.addEventListener("focusin", function () { setRail(true); });
    rail.addEventListener("focusout", function (event) {
      if (!rail.contains(event.relatedTarget)) setRail(false);
    });
    rail.addEventListener("click", function () {
      if (window.matchMedia("(hover: none)").matches) {
        setRail(!demo.classList.contains("is-rail-open"));
      }
    });
  }

  if (motion.matches || !("IntersectionObserver" in window)) return;

  var revealTargets = document.querySelectorAll(
    ".section-head, .shot, .card, .faq details, .cta .wrap"
  );
  revealTargets.forEach(function (el) {
    el.classList.add("reveal");
    var group = el.parentElement ? el.parentElement.children : [];
    var index = 0;
    for (var i = 0; i < group.length; i++) {
      if (group[i] === el) index = i;
    }
    if (index > 0) el.style.transitionDelay = Math.min(index, 5) * 70 + "ms";
  });
  var seen = new IntersectionObserver(function (entries) {
    entries.forEach(function (entry) {
      if (!entry.isIntersecting) return;
      entry.target.classList.add("is-in");
      seen.unobserve(entry.target);
    });
  }, { rootMargin: "0px 0px -10% 0px", threshold: 0.08 });
  revealTargets.forEach(function (el) {
    var top = el.getBoundingClientRect().top;
    if (top < window.innerHeight * 0.92) el.classList.add("is-in");
    else seen.observe(el);
  });

  if (!demo || !rail) return;

  var railDemoPlayed = false;
  var railDemo = new IntersectionObserver(function (entries) {
    entries.forEach(function (entry) {
      if (!entry.isIntersecting || railDemoPlayed) return;
      railDemoPlayed = true;
      railDemo.disconnect();
      setRail(true);
      window.setTimeout(function () {
        if (railHeld || rail.matches(":hover")) return;
        setRail(false);
      }, 2200);
    });
  }, { threshold: 0.45 });
  railDemo.observe(demo);
})();
