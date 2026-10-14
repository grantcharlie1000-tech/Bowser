// Bowser Shield keyboard chord helpers: parse "Alt+Shift+S", match a keydown against it, and tell
// whether the focused element takes typing.
//
// Split out of ui/panel.js so the content script, which runs in every frame, carries only these few
// lines; the panel itself (ui/panel.js) is injected into the top frame when the chord is pressed. A
// classic script with no chrome.* and no timers. It is loaded like shield-core.js: require() from the
// Node tests, a <script> tag in the popup and options pages, a content-script entry in the manifest.

const ShieldChord = (function () {
  "use strict";

  var MODIFIERS = { alt: "alt", shift: "shift", ctrl: "ctrl", control: "ctrl", meta: "meta" };

  function parseChord(text) {
    if (typeof text !== "string") return null;
    var parts = text.split("+");
    if (parts.length < 2) return null;
    var chord = { alt: false, shift: false, ctrl: false, meta: false, code: null };
    for (var i = 0; i < parts.length; i++) {
      var word = parts[i].trim();
      var low = word.toLowerCase();
      if (Object.prototype.hasOwnProperty.call(MODIFIERS, low)) {
        if (chord[MODIFIERS[low]]) return null;
        chord[MODIFIERS[low]] = true;
      } else if (chord.code !== null) {
        return null;
      } else if (/^[a-z]$/i.test(word)) {
        chord.code = "Key" + word.toUpperCase();
      } else if (/^[0-9]$/.test(word)) {
        chord.code = "Digit" + word;
      } else if (/^f([1-9]|1[0-2])$/i.test(word)) {
        chord.code = "F" + word.slice(1);
      } else {
        return null;
      }
    }
    if (chord.code === null) return null;
    if (!chord.alt && !chord.ctrl && !chord.meta) return null;
    return chord;
  }

  function matchChord(chord, event) {
    return (
      !!chord &&
      event.code === chord.code &&
      !!event.altKey === chord.alt &&
      !!event.shiftKey === chord.shift &&
      !!event.ctrlKey === chord.ctrl &&
      !!event.metaKey === chord.meta
    );
  }

  function isEditable(el) {
    if (!el) return false;
    var tag = String(el.tagName || "").toUpperCase();
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
    if (el.isContentEditable) return true;
    if (typeof el.getAttribute === "function") {
      var role = el.getAttribute("role");
      if (role && role.toLowerCase() === "textbox") return true;
    }
    return false;
  }

  return { parseChord: parseChord, matchChord: matchChord, isEditable: isEditable };
})();

if (typeof module === "object" && module.exports) module.exports = ShieldChord; // Node tests
if (typeof self === "object") self.ShieldChord = self.ShieldChord || ShieldChord; // content script
if (typeof window === "object") window.ShieldChord = ShieldChord; // popup/options
