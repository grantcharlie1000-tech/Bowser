// X promoted posts: mark the timeline cell of every promoted post so the cosmetic rule in
// `x_promoted.rs` hides it. Runs only on x.com / twitter.com when the blocker is at the
// "trackers and ads" level for the site. It reads nothing but the label text of timeline posts,
// keeps nothing and sends nothing. If X changes its markup, edit the constants below.
(() => {
  if (window.__bowserXPromoted) return;
  window.__bowserXPromoted = true;
  const CELL = '[data-testid="cellInnerDiv"]';
  const POST = 'article[data-testid="tweet"]';
  // The label X puts in a promoted post's header; never read inside the post text or names.
  const LABELS = ['Ad', 'Promoted', 'Sponsored'];
  const NOT_LABEL = '[data-testid="tweetText"],[data-testid="User-Name"],[data-testid="quoteTweet"]';
  const MARK = 'data-bowser-ad';
  const promoted = (post) => {
    for (const el of post.querySelectorAll('span,div')) {
      if (el.childElementCount !== 0 || !LABELS.includes(el.textContent.trim())) continue;
      if (el.closest(NOT_LABEL)) continue;
      if (el.closest(POST) === post) return true;
    }
    return false;
  };
  const scan = () => {
    // The cosmetic style is the on switch: gone means the site went back to a lower level.
    if (!document.getElementById('bowser-cosmetic')) {
      observer.disconnect();
      window.__bowserXPromoted = false;
      for (const cell of document.querySelectorAll('[' + MARK + ']')) cell.removeAttribute(MARK);
      return;
    }
    for (const cell of document.querySelectorAll(CELL)) {
      const post = cell.querySelector(POST);
      const ad = !!post && promoted(post);
      if (ad !== cell.hasAttribute(MARK)) ad ? cell.setAttribute(MARK, '') : cell.removeAttribute(MARK);
    }
  };
  let timer = 0;
  const later = () => {
    if (timer) return;
    timer = setTimeout(() => { timer = 0; scan(); }, 250);
  };
  const observer = new MutationObserver((records) => {
    for (const record of records) if (record.addedNodes.length) return later();
  });
  const start = () => {
    observer.observe(document.body, { childList: true, subtree: true });
    scan();
  };
  if (document.body) start();
  else document.addEventListener('DOMContentLoaded', start, { once: true });
})()
