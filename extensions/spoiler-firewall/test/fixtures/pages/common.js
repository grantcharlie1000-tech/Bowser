// Shared helpers for fixture pages (runs AFTER the engine; used for dynamic content).
window.FX = {
  el(tag, attrs, html) { const e = document.createElement(tag); for (const k in attrs || {}) e.setAttribute(k, attrs[k]); if (html != null) e.innerHTML = html; return e; },
  sentinel(t) { return '<span class="sentinel">' + t + '</span>'; },
  SPOILER: 'GTA 6 ending spoiler: the hero dies in the final mission',
};
