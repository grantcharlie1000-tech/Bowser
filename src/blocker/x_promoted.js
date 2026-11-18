// X promoted posts. Runs only on x.com / twitter.com when the blocker is at the "trackers and ads"
// level for the site; the `bowser-cosmetic` style is the on switch (gone means off).
//
// 1. Timeline data (the main path). X marks every promoted entry in its own GraphQL timeline
//    responses (HomeTimeline, HomeLatestTimeline, TweetDetail, SearchTimeline, UserTweets, ...):
//    an entryId starting "promoted-" / "promotedTweet", or `promotedMetadata` on the entry's
//    itemContent. The script wraps the readers of those responses (the XMLHttpRequest response
//    getters and Response.prototype.text/json, patched on the prototypes, so a request X started
//    before this ran still goes through them) and drops those entries before X renders them. This
//    needs no markup and no UI language. It reads only responses from X's own /graphql/ API in
//    this page, keeps nothing and sends nothing; it never makes a request of its own.
// 2. Label (fallback). Mark the timeline cell of a post whose header row shows X's "Ad" label so
//    the cosmetic rule in `x_promoted.rs` hides it. No class names: only roles, data-testid and the
//    label text. If X changes its markup, edit the constants below.
// Debug: a console.debug line each time entries are dropped or the number of hidden cells changes,
// window.__bowserXPromotedNet.stats ({checked, removed}) and window.__bowserXPromoted.stats
// ({scans, posts, hidden}).
(() => {
  const on = () => !!document.getElementById('bowser-cosmetic');
  if (!window.__bowserXPromotedNet) {
    // Once per document: a later run (the style came back) must not wrap the readers twice.
    const net = { stats: { checked: 0, removed: 0 } };
    window.__bowserXPromotedNet = net;
    const GRAPHQL = /\/graphql\//;
    const PROMOTED_ID = /^promoted(?:-|tweet)/i;
    const isObject = (value) => value !== null && typeof value === 'object';
    const flagged = (content) => isObject(content) && (content.promotedMetadata != null || content.promoted_metadata != null);
    // A timeline entry (TimelineAddEntries.entries), a module item (TimelineTimelineModule.items,
    // TimelineAddToModule.moduleItems) or an instruction holding one (TimelinePinEntry,
    // TimelineReplaceEntry) that X itself marks as promoted.
    const promotedEntry = (entry) => {
      if (!isObject(entry) || Array.isArray(entry)) return false;
      if (typeof entry.entryId === 'string' && PROMOTED_ID.test(entry.entryId)) return true;
      const content = entry.content;
      if (flagged(entry.itemContent) || (isObject(content) && flagged(content.itemContent))) return true;
      if (isObject(entry.item) && flagged(entry.item.itemContent)) return true;
      if (isObject(content) && isObject(content.item) && isObject(content.item.content) && flagged(content.item.content.tweet)) return true;
      return isObject(entry.entry) && promotedEntry(entry.entry);
    };
    const moduleSize = (entry) => {
      if (!isObject(entry)) return -1;
      if (isObject(entry.content) && Array.isArray(entry.content.items)) return entry.content.items.length;
      return Array.isArray(entry.moduleItems) ? entry.moduleItems.length : -1;
    };
    // Removes promoted entries from every array in `node`, in place, and any module that only held
    // promoted items. Returns how many entries it removed.
    const prune = (node, depth) => {
      if (!isObject(node) || depth > 100) return 0;
      let removed = 0;
      if (Array.isArray(node)) {
        for (let i = node.length - 1; i >= 0; i--) {
          const item = node[i];
          if (promotedEntry(item)) {
            node.splice(i, 1);
            removed++;
            continue;
          }
          const size = moduleSize(item);
          const inner = prune(item, depth + 1);
          removed += inner;
          if (inner && size > 0 && moduleSize(item) === 0) node.splice(i, 1);
        }
      } else {
        for (const key of Object.keys(node)) removed += prune(node[key], depth + 1);
      }
      return removed;
    };
    const count = (removed) => {
      net.stats.checked++;
      if (!removed) return;
      net.stats.removed += removed;
      console.debug('[bowser] x promoted entries dropped from a timeline response: ' + removed);
    };
    // The cheap test first: a response with no promoted entry has no "romoted" anywhere in it.
    const pruneText = (text) => {
      if (typeof text !== 'string' || !text.includes('romoted')) return text;
      try {
        const data = JSON.parse(text);
        const removed = prune(data, 0);
        count(removed);
        return removed ? JSON.stringify(data) : text;
      } catch (_) {
        return text;
      }
    };
    const pruneData = (data) => {
      try {
        count(prune(data, 0));
      } catch (_) {}
      return data;
    };
    const XHR = window.XMLHttpRequest && window.XMLHttpRequest.prototype;
    if (XHR) {
      const texts = new WeakMap();
      const objects = new WeakSet();
      for (const name of ['responseText', 'response']) {
        const original = Object.getOwnPropertyDescriptor(XHR, name);
        if (!original || !original.get || !original.configurable) continue;
        Object.defineProperty(XHR, name, {
          configurable: true,
          enumerable: original.enumerable,
          get() {
            const raw = original.get.call(this);
            try {
              if (this.readyState !== 4 || !on() || !GRAPHQL.test(this.responseURL || '')) return raw;
              if (typeof raw === 'string') {
                // Both getters, and every read of the same response, give the same pruned text.
                const seen = texts.get(this);
                if (seen && seen.raw === raw) return seen.out;
                const out = pruneText(raw);
                texts.set(this, { raw, out });
                return out;
              }
              if (isObject(raw) && this.responseType === 'json' && !objects.has(raw)) {
                objects.add(raw);
                pruneData(raw);
              }
            } catch (_) {}
            return raw;
          },
        });
      }
    }
    const RESPONSE = window.Response && window.Response.prototype;
    if (RESPONSE && typeof RESPONSE.text === 'function' && typeof RESPONSE.json === 'function') {
      const text = RESPONSE.text;
      const json = RESPONSE.json;
      const wanted = (response) => {
        try {
          return on() && GRAPHQL.test(response.url || '');
        } catch (_) {
          return false;
        }
      };
      RESPONSE.text = function () {
        const body = text.call(this);
        return wanted(this) ? body.then(pruneText) : body;
      };
      RESPONSE.json = function () {
        if (!wanted(this)) return json.call(this);
        return text.call(this).then((body) => {
          const data = JSON.parse(body);
          return body.includes('romoted') ? pruneData(data) : data;
        });
      };
    }
  }

  if (window.__bowserXPromoted) return;
  const CELL = '[data-testid="cellInnerDiv"]';
  const POST = 'article[data-testid="tweet"],article[role="article"]';
  // The label X puts in a promoted post's header, per UI language, matched on a whole text node.
  const LABELS = [
    'ad', 'ads', 'promoted', 'sponsored', 'anzeige', 'gesponsert', 'annonce', 'sponsorisé', 'publicité',
    'publicidad', 'anuncio', 'promocionado', 'patrocinado', 'sponsorizzato', 'annuncio', 'promovido',
    'anúncio', 'advertentie', 'gesponsord', 'gepromoot', 'reklam', 'reklama', 'annons', 'mainos',
    'iklan', 'реклама', 'διαφήμιση', 'إعلان', 'מודעה', 'विज्ञापन', 'โฆษณา', 'quảng cáo',
    '広告', 'プロモーション', '광고', '广告', '廣告', '推广',
  ];
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
    // The cosmetic style is the on switch: gone means the site went back to a lower level. The
    // timeline readers above stay wrapped but check the switch on every response.
    if (!on()) {
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
