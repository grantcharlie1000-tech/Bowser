// After the replayed messages land, puts the Extensions section in the state a shot needs: one
// row's declarations open (`details`), or a removal being confirmed (`confirm`).
(() => {
  'use strict';
  const scn = new URLSearchParams(location.search).get('scn');
  if (scn !== 'details' && scn !== 'confirm') return;
  const timer = setInterval(() => {
    const rows = document.querySelectorAll('.sp-ext-item');
    if (rows.length === 0) return;
    clearInterval(timer);
    if (scn === 'details') rows[0].querySelector('.sp-ext-toggle-details')?.click();
    if (scn === 'confirm') document.querySelector('.sp-ext-item[data-id="ext-a81c55e0f9d2b304"] .sp-ext-remove')?.click();
    document.activeElement?.blur?.();
  }, 20);
})();
