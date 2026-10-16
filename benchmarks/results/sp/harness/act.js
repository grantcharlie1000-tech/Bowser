// Scripted interaction for screenshots: window.__runAct("click:<css>|focus:<css>|wait:<ms>") runs the steps in order.
(() => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const waitFor = async (selector) => {
    for (let n = 0; n < 100; n += 1) {
      const node = document.querySelector(selector);
      if (node) return node;
      await sleep(50);
    }
    return null;
  };
  window.__runAct = async (act) => {
    for (const step of act.split('|')) {
      const i = step.indexOf(':');
      const kind = i < 0 ? step : step.slice(0, i);
      const arg = i < 0 ? '' : step.slice(i + 1);
      if (kind === 'click') (await waitFor(arg))?.click();
      else if (kind === 'focus') (await waitFor(arg))?.focus();
      else if (kind === 'wait') await sleep(Number(arg));
      await sleep(120);
    }
  };
})();
