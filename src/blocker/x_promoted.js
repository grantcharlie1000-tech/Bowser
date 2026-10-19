// X promoted posts: mark the timeline cell of every promoted post so the cosmetic rule in
// `x_promoted.rs` hides it. Runs only on x.com / twitter.com when the blocker is at the
// "trackers and ads" level for the site. It reads nothing but the label text of timeline posts,
// keeps nothing and sends nothing. No class names: only roles, data-testid and the label text.
// If X changes its markup, edit the constants below. Debug: a console.debug line each time the
// number of hidden cells changes, and window.__bowserXPromoted.stats ({scans, posts, hidden}).
(() => {
  if (window.__bowserXPromoted) return;
  const CELL = '[data-testid="cellInnerDiv"]';
  const POST = 'article[data-testid="tweet"],article[role="article"]';
  // The label X puts in a promoted post's header, per UI language, matched on a whole text node.
  const LABELS = ['ad', 'ads', 'promoted', 'sponsored', 'anzeige', 'annonce', 'publicidad', 'promocionado', 'sponsorizzato', 'promovido', 'reklam'];
  // Text in these is the author's, not X's label: the post text, a quoted post.
  const NOT_LABEL = '[data-testid="tweetText"],[data-testid="quoteTweet"]';
  const CARET = '[data-testid="caret"]';
  // Post content below the header row: a label word in these (a poll option "Ads") is never X's label.
  const BODY = '[data-testid="tweetText"],[data-testid="cardPoll"],[data-testid="card.wrapper"],[data-testid="tweetPhoto"],[data-testid="quoteTweet"]';
  const MARK = 'data-bowser-ad';
  const stats = { scans: 0, posts: 0, hidden: 0 };
  window.__bowserXPromoted = { stats };
  // A label text node: a whole node that is just the word, in the post but not in its text, not
  // in a link (display names and handles are links) unless that link is the post's own header row
  // (it holds the "..." menu), and not inside a quoted post (a link that holds no menu). It must
  // sit in the header row next to the "..." menu: the smallest box around both the menu and the
  // word is below the article and holds no post content (text, poll, card, media).
  const inHeader = (post, caret, el) => {
    let box = caret.parentElement;
    while (box && box !== post && !box.contains(el)) box = box.parentElement;
    return !!box && box !== post && post.contains(box) && !box.querySelector(BODY);
  };
  const promoted = (post) => {
    const caret = post.querySelector(CARET);
    if (!caret) return false;
    const walker = document.createTreeWalker(post, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const text = node.data.trim();
      if (text.length === 0 || text.length > 12 || !LABELS.includes(text.toLowerCase())) continue;
      const el = node.parentElement;
      if (!el || el.closest(NOT_LABEL)) continue;
      const link = el.closest('a,[role="link"]');
      if (link && post.contains(link) && !link.querySelector(CARET)) continue;
      if (!inHeader(post, caret, el)) continue;
      return true;
    }
    return false;
  };
  const scan = () => {
    // The cosmetic style is the on switch: gone means the site went back to a lower level.
    if (!document.getElementById('bowser-cosmetic')) {
      observer.disconnect();
      window.__bowserXPromoted = null;
      for (const cell of document.querySelectorAll('[' + MARK + ']')) cell.removeAttribute(MARK);
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
    for (const record of records) if (record.type === 'characterData' || record.addedNodes.length || record.removedNodes.length) return later();
  });
  const start = () => {
    observer.observe(document.body, { childList: true, subtree: true, characterData: true });
    scan();
  };
  if (document.body) start();
  else document.addEventListener('DOMContentLoaded', start, { once: true });
})()
