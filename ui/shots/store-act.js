// After the replayed messages land, puts the installation card in the state a shot needs:
// `?act=tech` opens the failure's "Technical details".
(() => {
  'use strict';
  const act = new URLSearchParams(location.search).get('act');
  if (act !== 'tech') return;
  const timer = setInterval(() => {
    const details = document.querySelector('.bs-tech');
    if (!details) return;
    clearInterval(timer);
    details.open = true;
  }, 20);
})();
