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
  const MARK = 'data-bowser-ad';
  const stats = { scans: 0, posts: 0, hidden: 0 };
  window.__bowserXPromoted = { stats };
  // A label text node: a whole node that is just the word, in the post but not in its text, not
  // in a link (display names and handles are links) unless that link is the post's own header row
  // (it holds the "..." menu), and not inside a quoted post (a link that holds no menu).
  const promoted = (post) => {
    const walker = document.createTreeWalker(post, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const text = node.data.trim();
      if (text.length === 0 || text.length > 12 || !LABELS.includes(text.toLowerCase())) continue;
      const el = node.parentElement;
      if (!el || el.closest(NOT_LABEL)) continue;
      const link = el.closest('a,[role="link"]');
      if (link && post.contains(link) && !link.querySelector(CARET)) continue;
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
    for (const post of posts) {
      const cell = post.closest(CELL) || post;
      const ad = promoted(post);
      if (ad) hidden++;
      if (ad !== cell.hasAttribute(MARK)) ad ? cell.setAttribute(MARK, '') : cell.removeAttribute(MARK);
    }
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
