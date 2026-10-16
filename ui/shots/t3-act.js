// After the replayed messages land, puts the page in the state a privacy shot needs: scrolled to a
// block, or with the Delete browsing data question open.
(() => {
  'use strict';
  const params = new URLSearchParams(location.search);
  const scn = params.get('scn');
  const before = params.get('mode') === 'before';
  const scrollTo = (row, offset = 8) => {
    const target = document.querySelector(`[data-row="${row}"]`);
    const main = document.querySelector('.sp-main');
    if (target && main) main.scrollTop = Math.max(0, target.offsetTop - offset);
  };
  const timer = setInterval(() => {
    if (!window.__delivered) return;
    const ready = before ? document.querySelector('.sp-dash[data-row="dashboard"] .sp-card') : document.querySelector('.pg-protect-delete .pg-row');
    if (!ready && !scn.startsWith('appearance') && !scn.startsWith('shield')) return;
    clearInterval(timer);
    setTimeout(() => {
      if (scn === 'privacy-sites') scrollTo('siteRules');
      if (scn === 'privacy-lists') scrollTo('blockLists');
      if (scn === 'privacy-delete' || scn === 'privacy-delete-armed') {
        if (before) scrollTo('dashboard', -9999);
        else {
          scrollTo('deleteData');
          if (scn === 'privacy-delete-armed') {
            const row = (label) => [...document.querySelectorAll('.pg-protect-delete .pg-row')].find((r) => r.querySelector('.pg-label')?.textContent === label);
            row('Saved sign-ins')?.querySelector('[role="switch"]')?.click();
            row('Confirm deleting saved sign-ins')?.querySelector('[role="switch"]')?.click();
            [...document.querySelectorAll('.pg-protect-delete button')].find((b) => b.textContent === 'Delete')?.click();
          }
        }
      }
      if (scn === 'appearance') scrollTo(before ? 'themeAccent' : 'websiteAppearance', before ? 200 : 120);
      document.activeElement?.blur?.();
    }, 60);
  }, 25);
})();
