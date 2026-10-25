// X promoted posts: mark the timeline cell of every promoted post so the cosmetic rule in
// `x_promoted.rs` hides it. Runs only on x.com / twitter.com when the blocker is at the
// "trackers and ads" level for the site. It reads only timeline badge text/accessibility labels,
// keeps nothing and sends nothing. No class names: only roles, data-testid and the label text.
// If X changes its markup, edit the constants below. Debug: a console.debug line each time the
// number of hidden cells changes, and window.__bowserXPromoted.stats ({scans, posts, hidden}).
(() => {
  if (window.__bowserXPromoted) return;
  const CELL = '[data-testid="cellInnerDiv"]';
  const POST = 'article[data-testid="tweet"],article[role="article"]';
  // Exact badge labels, including badges split over several child text nodes.
  const LABELS = ['ad', 'ads', 'promoted', 'sponsored', 'anzeige', 'annonce', 'publicidad', 'promocionado', 'sponsorizzato', 'promovido', 'reklam'];
  // Text in these is the author's, not X's label: the post text, a quoted post.
  const NOT_LABEL = '[data-testid="User-Name"],[data-testid="tweetText"],[data-testid="quoteTweet"],button,[role="button"],time';
  const CARET = '[data-testid="caret"]';
  // Post content below the header row: a label word in these (a poll option "Ads") is never X's label.
  const BODY = '[data-testid="tweetText"],[data-testid="cardPoll"],[data-testid="card.wrapper"],[data-testid="tweetPhoto"],[data-testid="quoteTweet"]';
  const MARK = 'data-bowser-ad';
  const stats = { scans: 0, posts: 0, hidden: 0 };
  const stop = () => {
    observer.disconnect();
    clearTimeout(timer);
    timer = 0;
    document.removeEventListener('DOMContentLoaded', start);
    for (const cell of document.querySelectorAll('[' + MARK + ']')) cell.removeAttribute(MARK);
    stats.hidden = 0;
    window.__bowserXPromoted = null;
  };
  window.__bowserXPromoted = { stats, stop };
  // The smallest box around the author/menu and badge must be a header, below the article and
  // without post content. The author anchor also handles headers whose menu has no caret test id.
  const inHeader = (post, anchor, el) => {
    let box = anchor.parentElement;
    while (box && box !== post && !box.contains(el)) box = box.parentElement;
    return !!box && box !== post && post.contains(box) && !box.querySelector(BODY);
  };
  const isLabel = (text) => LABELS.includes(text.trim().toLowerCase());
  const promoted = (post) => {
    const anchor = post.querySelector('[data-testid="User-Name"]') || post.querySelector(CARET);
    if (!anchor) return false;
    for (const el of post.querySelectorAll('span,div,[aria-label],[title]')) {
      const text = el.textContent.trim();
      if (!isLabel(text) && (text || ![el.getAttribute('aria-label'), el.getAttribute('title')].some((value) => value && isLabel(value)))) continue;
      if (el.contains(anchor) || el.closest(NOT_LABEL) || el.closest(POST) !== post) continue;
      const link = el.closest('a,[role="link"]');
      if (link && post.contains(link) && !link.contains(anchor)) continue;
      if (!inHeader(post, anchor, el)) continue;
      return true;
    }
    return false;
  };
  const scan = () => {
    // Style removal is a fallback; the engine explicitly calls stop even when cookie CSS stays.
    if (!document.getElementById('bowser-cosmetic')) {
      stop();
      return;
    }
    stats.scans++;
    let hidden = 0;
    const posts = document.querySelectorAll(POST);
    const ads = new Set();
    for (const post of posts) if (promoted(post)) ads.add(post.closest(CELL) || post);
    for (const cell of ads) {
      hidden++;
      if (!cell.hasAttribute(MARK)) cell.setAttribute(MARK, '');
    }
    // X recycles timeline cells: a marked cell whose post changed, or that no longer holds a
    // promoted post at all, shows again.
    for (const cell of document.querySelectorAll('[' + MARK + ']')) if (!ads.has(cell)) cell.removeAttribute(MARK);
    stats.posts = posts.length;
    if (hidden !== stats.hidden) {
      stats.hidden = hidden;
      console.debug('[bowser] x promoted posts hidden: ' + hidden + ' of ' + posts.length);
    }
  };
  let timer = 0;
  const later = () => {
    if (timer) return;
    timer = setTimeout(() => { timer = 0; scan(); }, 250);
  };
  // Added or removed nodes and changed text all count: a recycled cell can change either way.
  const observer = new MutationObserver((records) => {
    for (const record of records) if (record.type === 'attributes' || record.type === 'characterData' || record.addedNodes.length || record.removedNodes.length) return later();
  });
  const start = () => {
    observer.observe(document.documentElement, { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ['aria-label', 'title', 'data-testid', 'role'] });
    scan();
  };
  if (document.body) start();
  else document.addEventListener('DOMContentLoaded', start, { once: true });
})()
