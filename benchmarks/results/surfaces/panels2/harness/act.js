// Scripted interaction for screenshots: ?act=click:<css>|key:<Key>|focus:<css>|wait:<ms> (steps run in order after load).
(() => {
  const act = new URLSearchParams(location.search).get('act');
  if (!act) return;
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const waitFor = async (sel) => { for (let n = 0; n < 100; n += 1) { const e = document.querySelector(sel); if (e) return e; await sleep(50); } return null; };
  window.addEventListener('load', async () => {
    await sleep(400);
    for (const step of act.split('|')) {
      const i = step.indexOf(':');
      const kind = i < 0 ? step : step.slice(0, i);
      const arg = i < 0 ? '' : step.slice(i + 1);
      if (kind === 'click') (await waitFor(arg))?.click();
      else if (kind === 'focus') (await waitFor(arg))?.focus();
      else if (kind === 'key') {
        const t = document.activeElement || document.body;
        t.dispatchEvent(new KeyboardEvent('keydown', { key: arg === 'Space' ? ' ' : arg, bubbles: true, cancelable: true }));
      } else if (kind === 'wait') await sleep(Number(arg));
      await sleep(120);
    }
    const a = document.activeElement;
    if (new URLSearchParams(location.search).get('dbg')) { const d = document.createElement('div'); d.style.cssText='position:fixed;bottom:0;left:0;background:#fff;color:#000;font:10px monospace;z-index:99'; d.textContent = `app=${document.getElementById('app')?.className}|${document.getElementById('app')?.children.length} tabs=${document.querySelectorAll('.side-tab').length} act=${document.querySelectorAll('.side-tab.is-active').length} hasFocus=${document.hasFocus()} `+ (a ? `${a.tagName}.${a.className}|fv=${a.matches(':focus-visible')}|tab=${a.getAttribute('tabindex')}` : 'none'); document.body.append(d); }
  });
})();
