/* Spoiler Firewall for GTA VI 0.0.0-probe — LIVE-TEST PROBE, not the detection engine. Verifies that Bowser injects a
   script with the config substituted into every frame; draws an unmistakable red shield where the page text matches. */
(function () {
"use strict";
const CFG = /*__SFW_CONFIG__*/null;
const probe = { cfg: CFG, href: String(location.href), top: window === window.top, shielded: false, titleRewritten: false, at: 'start' };
const BEACON = 'http://127.0.0.1:18081/__report?d=';
function beacon(phase) {
  try { new Image().src = BEACON + encodeURIComponent(JSON.stringify({ phase, href: String(location.href).slice(0, 80), top: probe.top, cfg: CFG, shielded: probe.shielded, titleRewritten: probe.titleRewritten, title: document.title.slice(0, 60), t: Date.now() })); } catch (e) {}
}
try { Object.defineProperty(window, '__sfwProbe', { value: probe, configurable: true }); } catch (e) { window.__sfwProbe = probe; }
const SPOILER = /gta\s*(?:6|vi)[\s\S]{0,60}(?:ending|betray|dies)|(?:ending|betray|dies)[\s\S]{0,60}gta\s*(?:6|vi)/i;
function shield() {
  if (probe.shielded || !document.documentElement) return;
  const div = document.createElement('div');
  div.setAttribute('data-sfw-probe-shield', '1');
  div.style.cssText = 'position:fixed;inset:0;z-index:2147483647;background:#c00018;color:#fff;font:700 18px sans-serif;display:flex;align-items:center;justify-content:center;text-align:center';
  div.textContent = 'PROBE SHIELD (' + (probe.top ? 'top' : 'frame') + ')';
  document.documentElement.appendChild(div);
  probe.shielded = true;
  beacon('shield');
}
function check() {
  probe.at = document.readyState;
  const text = (document.body && document.body.textContent) || '';
  if (SPOILER.test(text)) shield();
  if (SPOILER.test(document.title) && document.title !== 'Spoiler-protected page') { document.title = 'Spoiler-protected page'; probe.titleRewritten = true; beacon('title'); }
}
beacon('inject');
new MutationObserver(check).observe(document, { subtree: true, childList: true, characterData: true });
document.addEventListener('DOMContentLoaded', check);
addEventListener('load', () => { check(); beacon('load'); });
})();
