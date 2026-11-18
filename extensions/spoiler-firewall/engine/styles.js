// SFW.styles — CSS text only. Adopted through constructable stylesheets (never <style>, never innerHTML),
// so page CSP / Trusted Types cannot block it.
//
// Secrecy model (DESIGN §10): a shielded container is `visibility:hidden` together with all of its
// descendants (layout is untouched, nothing paints, nothing is focusable / selectable / findable / in the
// accessibility tree). The overlay is the single `visibility:visible` descendant. The blur / tint on the
// overlay is presentation; secrecy does not depend on it. Transitions and animations are disabled on
// shielded nodes so a page `transition: all 1s` can never delay the hide.
SFW.styles = (function () {
  const SHIELDED = '[data-sfw~="shield"]';
  const HOST_SHIELDED = ':host([data-sfw~="shield"])';

  const PAGE_CSS = `
/* --- shielded unit: hidden, inert-looking, no motion ------------------------------------------ */
${SHIELDED}{
  visibility:hidden!important;isolation:isolate!important;
  -webkit-text-fill-color:transparent!important;color:transparent!important;text-shadow:none!important;
  -webkit-user-select:none!important;user-select:none!important;
  transition:none!important;animation:none!important;
}
${SHIELDED} *,${SHIELDED} *::before,${SHIELDED} *::after{
  visibility:hidden!important;transition:none!important;animation:none!important;
  -webkit-user-select:none!important;user-select:none!important;
}
${HOST_SHIELDED},${HOST_SHIELDED} *,${HOST_SHIELDED} *::before,${HOST_SHIELDED} *::after{
  visibility:hidden!important;transition:none!important;animation:none!important;
  -webkit-user-select:none!important;user-select:none!important;
}
[data-sfw-pos]{position:relative!important}
/* region cover: work that did not fit in a time budget stays hidden (layout untouched) until it has been classified */
[data-sfw-defer],[data-sfw-defer] *,[data-sfw-defer] *::before,[data-sfw-defer] *::after{visibility:hidden!important}
/* the overlay is the only thing that stays visible */
${SHIELDED} > sfw-shield,${HOST_SHIELDED} > sfw-shield,sfw-shield{visibility:visible!important}
${SHIELDED} ${SHIELDED} > sfw-shield{visibility:hidden!important}
/* revealed: overlay turns into a click-through layer that only carries the "hide again" control */
[data-sfw~="revealed"]:hover,[data-sfw~="revealed"]:focus-within{--sfw-hide-o:1}
:host([data-sfw~="revealed"]:hover),:host([data-sfw~="revealed"]:focus-within){--sfw-hide-o:1}

/* --- attribute-only cover: pending state and the React-removal fallback ---------------------- */
${SHIELDED}[data-sfw-fb]{position:relative!important;background-clip:padding-box!important}
${SHIELDED}[data-sfw-fb]::after,${HOST_SHIELDED}[data-sfw-fb]::after{
  content:"Spoiler \\00b7  Click to reveal";
  visibility:visible!important;position:absolute!important;inset:0!important;z-index:2147483646!important;
  display:flex!important;align-items:center!important;justify-content:center!important;text-align:center!important;
  box-sizing:border-box!important;padding:6px 10px!important;border-radius:inherit!important;
  background:#12131b!important;color:#ff7b7b!important;-webkit-text-fill-color:#ff7b7b!important;
  font:800 13px/1.2 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif!important;letter-spacing:.18em!important;
  text-transform:uppercase!important;cursor:pointer!important;
  box-shadow:inset 0 0 0 2px rgba(255,123,123,.55)!important;
}
${SHIELDED}[data-sfw-fb="pending"]::after{content:"Checking for GTA VI spoilers\\2026"!important;letter-spacing:.04em!important;text-transform:none!important;color:#cfd3e0!important;-webkit-text-fill-color:#cfd3e0!important;box-shadow:none!important}
${SHIELDED}[data-sfw-fb="error"]::after{content:"Couldn\\2019 t verify this content \\00b7  Click to reveal"!important;letter-spacing:.02em!important;text-transform:none!important;color:#ffd27a!important;-webkit-text-fill-color:#ffd27a!important}
${SHIELDED}[data-sfw-fb="mediaerr"]::after{content:"Couldn\\2019 t check image \\00b7  Click to reveal"!important;letter-spacing:.02em!important;text-transform:none!important;color:#ffd27a!important;-webkit-text-fill-color:#ffd27a!important}
${SHIELDED}[data-sfw-fb="frameerr"]::after{content:"Embedded content couldn\\2019 t be checked \\00b7  Click to reveal"!important;letter-spacing:.02em!important;text-transform:none!important;color:#ffd27a!important;-webkit-text-fill-color:#ffd27a!important}
/* attribute-only covers on a light page (the engine sets data-sfw-theme from the background behind the container) */
${SHIELDED}[data-sfw-fb][data-sfw-theme="light"]::after{background:#f3f3f8!important;color:#c2182f!important;-webkit-text-fill-color:#c2182f!important;box-shadow:inset 0 0 0 2px rgba(194,24,47,.45)!important}
${SHIELDED}[data-sfw-fb="pending"][data-sfw-theme="light"]::after{color:#3a3d4d!important;-webkit-text-fill-color:#3a3d4d!important;box-shadow:none!important}
${SHIELDED}[data-sfw-fb="error"][data-sfw-theme="light"]::after,${SHIELDED}[data-sfw-fb="mediaerr"][data-sfw-theme="light"]::after,${SHIELDED}[data-sfw-fb="frameerr"][data-sfw-theme="light"]::after{color:#8a5a00!important;-webkit-text-fill-color:#8a5a00!important;box-shadow:inset 0 0 0 2px rgba(138,90,0,.4)!important}
@media (forced-colors:active){
  ${SHIELDED}[data-sfw-fb]::after{background:Canvas!important;color:CanvasText!important;-webkit-text-fill-color:CanvasText!important;box-shadow:inset 0 0 0 2px CanvasText!important}
}

/* --- the overlay host (shadow content lives in SHIELD_CSS) ----------------------------------- */
sfw-shield{
  all:initial!important;
  display:block!important;position:absolute!important;inset:0!important;z-index:2147483647!important;
  box-sizing:border-box!important;margin:0!important;padding:0!important;border:0!important;
  border-radius:inherit!important;overflow:hidden!important;visibility:visible!important;opacity:1!important;
  transform:none!important;filter:none!important;pointer-events:auto!important;
  container-type:size!important;-webkit-text-fill-color:currentcolor!important;
  contain:layout paint style!important;
}
sfw-shield[data-s="revealed"]{pointer-events:none!important;overflow:visible!important;contain:none!important}
sfw-shield[data-k="page"]{position:fixed!important;border-radius:0!important}
/* <body> itself shielded (text straight under it): its box can be 0 px tall (absolutely positioned app shells), so the
   cover takes the viewport, never an invisible strip over a hidden page */
body[data-sfw~="shield"] > sfw-shield{position:fixed!important;border-radius:0!important}
sfw-shield[data-k="notice"]{position:fixed!important;inset:auto 14px 14px auto!important;width:auto!important;height:auto!important;
  border-radius:12px!important;container-type:normal!important;contain:none!important;overflow:visible!important}
html[data-sfw-page~="shield"] > body,html[data-sfw-page~="shield"] > body *{visibility:hidden!important}
html[data-sfw-page~="shield"]{overflow:hidden!important}
`;

  // Media quarantine for high-risk hosts: hidden until the owning unit has been classified clear in a cold context.
  // While the OCR gate checks an image (host.ocr), a media-only wrapper gets a quiet "Checking image" veil (it fades in
  // after 350 ms, so a fast result never shows it); the media itself stays hidden by the rules below either way.
  const QUARANTINE_CSS = `
[data-sfw-mcpos]{position:relative!important}
[data-sfw-mc]::after{
  content:"Checking image\\2026"!important;position:absolute!important;inset:0!important;z-index:2147483645!important;
  display:flex!important;align-items:center!important;justify-content:center!important;box-sizing:border-box!important;padding:6px!important;
  text-align:center!important;pointer-events:none!important;background:rgba(22,23,32,.58)!important;color:#d6d9e6!important;-webkit-text-fill-color:#d6d9e6!important;
  font:600 12px/1.2 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif!important;letter-spacing:.02em!important;text-transform:none!important;
  animation:sfw-mc-in 220ms ease-out 350ms both!important;
}
[data-sfw-mc][data-sfw-theme="light"]::after{background:rgba(244,244,250,.72)!important;color:#2a2d3c!important;-webkit-text-fill-color:#2a2d3c!important}
@keyframes sfw-mc-in{from{opacity:0}to{opacity:1}}
[data-sfw-fade]{animation:sfw-media-in 180ms ease-out}
@keyframes sfw-media-in{from{opacity:0}to{opacity:1}}
@media (prefers-reduced-motion:reduce){[data-sfw-mc]::after{animation:none!important}[data-sfw-fade]{animation:none}}
@media (forced-colors:active){[data-sfw-mc]::after{background:Canvas!important;color:CanvasText!important;-webkit-text-fill-color:CanvasText!important}}
img:not([data-sfw-m="ok"]),picture:not([data-sfw-m="ok"]),video:not([data-sfw-m="ok"]),canvas:not([data-sfw-m="ok"]),
object:not([data-sfw-m="ok"]),embed:not([data-sfw-m="ok"]),svg image:not([data-sfw-m="ok"]),
input[type="image"]:not([data-sfw-m="ok"]),[poster]:not([data-sfw-m="ok"]){visibility:hidden!important}
[style*="background-image"]:not([data-sfw-m="ok"]),[style*="background:"]:not([data-sfw-m="ok"]){background-image:none!important}
`;

  // Rules toggled at runtime through CSSOM insertRule/deleteRule on the page sheet.
  const OPAQUE_RULE = "sfw-shield{--sfw-bf:none!important;--sfw-a:.985!important}";
  const REDUCED_RULE = "sfw-shield{--sfw-bf:none!important;--sfw-dur:0ms!important;--sfw-a:.985!important}";

  // Closed shadow root of every overlay.
  const SHIELD_CSS = `
:host{
  --sfw-bf:blur(22px) saturate(.55);--sfw-dur:180ms;--sfw-a:.94;--sfw-hide-o:0;
  --ink:#ff7a85;--ink-soft:#ffb3ba;--fg:#f4f2fa;--fg-soft:rgba(244,242,250,.78);--ring:#8ab4ff;
  --bg1:17,18,26;--bg2:30,21,40;--tint:132,62,160;
}
@media (prefers-color-scheme:light){
  :host{--ink:#c2182f;--ink-soft:#8d1e2d;--fg:#1c1e29;--fg-soft:rgba(28,30,41,.74);--ring:#1a56db;
    --bg1:246,246,250;--bg2:233,229,240;--tint:150,110,190}
}
/* the page decides the theme (data-sfw-theme, set by the engine from the background behind the cover); the OS scheme above is only the first guess */
:host([data-sfw-theme="dark"]){
  --ink:#ff7a85;--ink-soft:#ffb3ba;--fg:#f4f2fa;--fg-soft:rgba(244,242,250,.78);--ring:#8ab4ff;
  --bg1:17,18,26;--bg2:30,21,40;--tint:132,62,160;
}
:host([data-sfw-theme="light"]){
  --ink:#c2182f;--ink-soft:#8d1e2d;--fg:#1c1e29;--fg-soft:rgba(28,30,41,.74);--ring:#1a56db;
  --bg1:246,246,250;--bg2:233,229,240;--tint:150,110,190;
}
*{box-sizing:border-box;margin:0}
.cover{
  position:absolute;inset:0;display:grid;place-items:center;overflow:hidden;cursor:pointer;
  border-radius:inherit;color:var(--fg);-webkit-text-fill-color:currentcolor;text-align:center;
  font:13px/1.35 system-ui,-apple-system,"Segoe UI",Roboto,"Helvetica Neue",Arial,sans-serif;
  -webkit-user-select:none;user-select:none;opacity:1;
  background:
    radial-gradient(120% 140% at 12% 0%,rgba(var(--tint),.34),transparent 58%),
    repeating-linear-gradient(135deg,rgba(255,255,255,.05) 0 7px,rgba(255,255,255,0) 7px 14px),
    linear-gradient(160deg,rgba(var(--bg1),var(--sfw-a)),rgba(var(--bg2),var(--sfw-a)));
  -webkit-backdrop-filter:var(--sfw-bf);backdrop-filter:var(--sfw-bf);
  transition:opacity var(--sfw-dur) ease-out,-webkit-backdrop-filter var(--sfw-dur) ease-out,backdrop-filter var(--sfw-dur) ease-out;
}
:host(:focus-visible) .cover{outline:3px solid var(--ring);outline-offset:-3px}
.cover.out{opacity:0;-webkit-backdrop-filter:blur(0) saturate(1);backdrop-filter:blur(0) saturate(1);pointer-events:none}
.cover[hidden]{display:none}
.card{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:.9em;padding:.6em;max-width:100%}
.stamp{
  display:inline-block;transform:rotate(-5deg);padding:.32em .85em .28em;
  border:4px double var(--ink);border-radius:.3em;color:var(--ink);-webkit-text-fill-color:var(--ink);
  font-weight:900;letter-spacing:.34em;text-indent:.34em;text-transform:uppercase;white-space:nowrap;
  font-size:clamp(14px,min(5cqw,12cqh),34px);line-height:1.15;
  box-shadow:0 0 0 1px rgba(255,122,133,.14),inset 0 0 0 1px rgba(255,122,133,.12);
  -webkit-mask-image:radial-gradient(circle at 18% 28%,#000 0 62%,rgba(0,0,0,.82) 64% 100%);
          mask-image:radial-gradient(circle at 18% 28%,#000 0 62%,rgba(0,0,0,.82) 64% 100%);
}
.sub{font-size:clamp(11px,min(2.4cqw,6cqh),14px);color:var(--fg-soft);-webkit-text-fill-color:var(--fg-soft);letter-spacing:.015em;max-width:34em;text-wrap:balance}
.pill{display:none;align-items:center;gap:.5em;padding:.3em .8em;border-radius:999px;font-weight:700;font-size:12px;
  letter-spacing:.04em;border:1.5px solid var(--ink);color:var(--ink);-webkit-text-fill-color:var(--ink);white-space:nowrap}
.pill i{width:.55em;height:.55em;border-radius:50%;background:var(--ink);display:inline-block}
@container (max-height:63px){.card{display:none}.pill{display:inline-flex}}
@container (max-width:170px){.sub{display:none}.stamp{letter-spacing:.2em;text-indent:.2em}}
@container (max-width:110px){.pill span{display:none}}
@container (min-width:420px) and (min-height:240px){.stamp{font-size:clamp(24px,min(7cqw,15cqh),46px);border-width:5px}.sub{font-size:clamp(13px,2cqw,17px)}}

/* pending: a quiet, flat cover first; the "Checking" label fades in only if the check takes a while, so the many
   checks that finish within a frame or two never flash a label */
.cover[data-v="pending"]{background:rgba(var(--bg1),var(--sfw-a));-webkit-backdrop-filter:none;backdrop-filter:none}
.cover[data-v="pending"] .card,.cover[data-v="pending"] .pill{animation:sfw-late 200ms ease-out 400ms both}
@keyframes sfw-late{from{opacity:0}to{opacity:1}}
.cover[data-v="pending"] .stamp{border-color:var(--fg-soft);color:var(--fg-soft);-webkit-text-fill-color:var(--fg-soft);transform:none;border-style:solid;border-width:2px;
  letter-spacing:.06em;text-indent:.06em;text-transform:none;font-weight:700;font-size:clamp(12px,min(3.4cqw,9cqh),18px);-webkit-mask-image:none;mask-image:none;box-shadow:none}
.cover[data-v="pending"] .spin{display:inline-block;width:1.05em;height:1.05em;border-radius:50%;border:2px solid var(--fg-soft);border-top-color:transparent;
  margin-right:.55em;vertical-align:-.18em;animation:spin 900ms linear infinite}
@keyframes spin{to{transform:rotate(360deg)}}
/* error */
.cover[data-v="error"],.cover[data-v="mediaerr"],.cover[data-v="frameerr"]{--ink:#ffc15e;--tint:200,140,40}
.cover[data-v="error"] .stamp,.cover[data-v="mediaerr"] .stamp,.cover[data-v="frameerr"] .stamp{letter-spacing:.08em;text-indent:.08em;text-transform:none;font-size:clamp(12px,min(3.6cqw,9cqh),20px);transform:rotate(-2deg)}
/* error ink stays readable on a light cover */
:host([data-sfw-theme="light"]) .cover[data-v="error"],:host([data-sfw-theme="light"]) .cover[data-v="mediaerr"],:host([data-sfw-theme="light"]) .cover[data-v="frameerr"]{--ink:#8a5a00;--tint:230,170,60}
@media (prefers-color-scheme:light){:host(:not([data-sfw-theme="dark"])) .cover[data-v="error"],:host(:not([data-sfw-theme="dark"])) .cover[data-v="mediaerr"],:host(:not([data-sfw-theme="dark"])) .cover[data-v="frameerr"]{--ink:#8a5a00;--tint:230,170,60}}
/* page */
.cover[data-v="page"]{--sfw-a:.985;cursor:default}
.cover[data-v="page"] .card{gap:1.1em;max-width:min(92vw,520px)}
.cover[data-v="page"] .stamp{font-size:clamp(24px,6vw,44px);border-width:5px}
.cover[data-v="page"] .sub{font-size:clamp(14px,2.2vw,17px)}
.btn{all:unset;box-sizing:border-box;cursor:pointer;padding:.62em 1.3em;border-radius:999px;font:700 14px system-ui,-apple-system,"Segoe UI",sans-serif;
  background:var(--ink);color:#16060a;-webkit-text-fill-color:#16060a;letter-spacing:.03em}
.btn:hover{filter:brightness(1.08)}
.btn:focus-visible{outline:3px solid var(--ring);outline-offset:3px}
/* notice */
.cover[data-v="notice"]{position:static;display:block;padding:.65em .95em;border-radius:12px;--sfw-a:.97;cursor:default;text-align:left;
  box-shadow:0 6px 24px rgba(0,0,0,.35);font-weight:600;font-size:13px;-webkit-backdrop-filter:none;backdrop-filter:none}

/* hide again */
.hide{
  position:absolute;top:8px;right:8px;display:inline-flex;align-items:center;gap:.4em;padding:.3em .65em .3em .5em;
  font:600 12px/1 system-ui,-apple-system,"Segoe UI",sans-serif;letter-spacing:.01em;
  color:#f4f2fa;-webkit-text-fill-color:#f4f2fa;background:rgba(16,17,24,.82);border:1px solid rgba(255,255,255,.24);
  border-radius:999px;cursor:pointer;pointer-events:auto;z-index:2;
  opacity:var(--sfw-hide-o);transform:translateY(calc((1 - var(--sfw-hide-o)) * -3px));
  transition:opacity 140ms ease-out,transform 140ms ease-out;-webkit-backdrop-filter:blur(6px);backdrop-filter:blur(6px);
}
.hide:hover{background:rgba(16,17,24,.95)}
.hide:focus-visible{opacity:1;transform:none;outline:3px solid var(--ring);outline-offset:2px}
.hide svg{width:14px;height:14px;fill:none;stroke:currentColor;stroke-width:2;stroke-linecap:round;stroke-linejoin:round}

@media (prefers-reduced-motion:reduce){
  :host{--sfw-bf:none;--sfw-dur:0ms;--sfw-a:.985}
  .cover,.hide{transition:none}.cover[data-v="pending"] .spin{animation:none}
  .cover[data-v="pending"] .card,.cover[data-v="pending"] .pill{animation:none}
}
@media (prefers-reduced-transparency:reduce){:host{--sfw-bf:none;--sfw-a:.985}}
@media (forced-colors:active){
  .cover{background:Canvas;-webkit-backdrop-filter:none;backdrop-filter:none;color:CanvasText;-webkit-text-fill-color:CanvasText;forced-color-adjust:none}
  .stamp,.pill{border-color:CanvasText;color:CanvasText;-webkit-text-fill-color:CanvasText;-webkit-mask-image:none;mask-image:none}
  .sub{color:CanvasText;-webkit-text-fill-color:CanvasText}
  .btn{background:ButtonFace;color:ButtonText;-webkit-text-fill-color:ButtonText;border:2px solid ButtonText}
  .hide{background:ButtonFace;color:ButtonText;-webkit-text-fill-color:ButtonText;border-color:ButtonText;forced-color-adjust:none}
  :host(:focus-visible) .cover,.hide:focus-visible,.btn:focus-visible{outline-color:Highlight}
}
`;

  return { PAGE_CSS, QUARANTINE_CSS, SHIELD_CSS, OPAQUE_RULE, REDUCED_RULE };
})();
