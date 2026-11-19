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
// ({scans, posts, hidden, checked}).
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
  const LABELS = new Set([
    'ad', 'ads', 'promoted', 'sponsored', 'anzeige', 'gesponsert', 'annonce', 'sponsorisé', 'publicité',
    'publicidad', 'anuncio', 'promocionado', 'patrocinado', 'sponsorizzato', 'annuncio', 'promovido',
    'anúncio', 'advertentie', 'gesponsord', 'gepromoot', 'reklam', 'reklama', 'annons', 'mainos',
    'iklan', 'реклама', 'διαφήμιση', 'إعلان', 'מודעה', 'विज्ञापन', 'โฆษณา', 'quảng cáo',
    '広告', 'プロモーション', '광고', '广告', '廣告', '推广',
  ]);
  // Text in these is the author's, not X's label: the post text, a quoted post.
  const NOT_LABEL = '[data-testid="tweetText"],[data-testid="quoteTweet"]';
  const CARET = '[data-testid="caret"]';
  // Post content below the header row: a label word in these (a poll option "Ads") is never X's label.
  const BODY = '[data-testid="tweetText"],[data-testid="cardPoll"],[data-testid="card.wrapper"],[data-testid="tweetPhoto"],[data-testid="quoteTweet"]';
  const MARK = 'data-bowser-ad';
  const SLICE_UNITS = 256, SLICE_MS = 4, MAX_PENDING_RECORDS = 4096;
  const stats = { scans: 0, posts: 0, hidden: 0, checked: 0 };
  window.__bowserXPromoted = { stats };

  // Yield one unit for each visit and each climb, including on deeply nested trees.
  function* walk(root) {
    let node = root;
    while (node) {
      yield node;
      if (node.firstChild) {
        node = node.firstChild;
      } else {
        while (node && node !== root && !node.nextSibling) {
          node = node.parentNode;
          yield null;
        }
        if (!node || node === root) return;
        node = node.nextSibling;
      }
    }
  }
  function* first(root, selector) {
    for (const node of walk(root)) {
      yield;
      if (node && node !== root && node.nodeType === 1 && node.matches(selector)) return node;
    }
    return null;
  }
  function* label(text) {
    if (text.length <= 256) {
      const trimmed = text.trim();
      return trimmed.length <= 12 && LABELS.has(trimmed.toLowerCase());
    }
    let start = 0, end = text.length;
    while (start < end && /\s/.test(text[start])) if ((++start & 255) === 0) yield;
    while (end > start && /\s/.test(text[end - 1])) if ((--end & 255) === 0) yield;
    return end - start <= 12 && LABELS.has(text.slice(start, end).toLowerCase());
  }
  function* promoted(post) {
    const caret = yield* first(post, CARET);
    if (!caret) return false;
    // The common ancestor of the label and caret must be below the post and contain no body
    // elements. Find the largest such ancestor, inspecting only siblings as we widen it.
    let header = null, branch = caret;
    for (let box = branch.parentElement; box && box !== post; box = box.parentElement) {
      if (branch.matches(BODY)) break;
      let body = false;
      for (let child = box.firstElementChild; child; child = child.nextElementSibling) {
        yield;
        if (child === branch && header) continue;
        if (child.matches(BODY) || (yield* first(child, BODY))) { body = true; break; }
      }
      if (body) break;
      header = box;
      branch = box;
    }
    if (!header) return false;
    for (const node of walk(header)) {
      yield;
      if (!node || node.nodeType !== 3 || !(yield* label(node.data))) continue;
      const el = node.parentElement;
      if (!el || el.closest(NOT_LABEL)) continue;
      const link = el.closest('a,[role="link"]');
      if (link && post.contains(link) && (!link.contains(caret) || link === caret) && !(yield* first(link, CARET))) continue;
      return true;
    }
    return false;
  }

  const results = new Map(); // post -> marked cell, or null for a normal post
  const marked = new Map(); // cell -> posts that currently mark it
  const dirty = new Set();
  const roots = new Set();
  const batches = [];
  let batch = 0, pendingRecords = 0, resync = false;
  let discover, records, check, timer = 0, stopped = false;
  const inBody = (node) => node && document.body && document.body.contains(node);
  function remember(post, cell) {
    const previous = results.get(post);
    if (previous && previous !== cell) {
      const owners = marked.get(previous);
      owners.delete(post);
      if (!owners.size) {
        marked.delete(previous);
        previous.removeAttribute(MARK);
      }
    }
    if (cell && previous !== cell) {
      if (!marked.has(cell)) marked.set(cell, new Set());
      marked.get(cell).add(post);
    }
    if (cell && !cell.hasAttribute(MARK)) cell.setAttribute(MARK, '');
    if (cell === undefined) results.delete(post);
    else results.set(post, cell);
  }
  // An inserted/replaced subtree is discovered once. A removed subtree is walked as well so
  // recycled cells lose their old mark even when they no longer hold a matching article.
  function* discoverPosts(root) {
    for (const node of walk(root)) {
      yield;
      if (!node || node.nodeType !== 1) continue;
      if (node.hasAttribute(MARK) && !marked.has(node)) node.removeAttribute(MARK);
      if (results.has(node) || (node.matches(POST) && inBody(node))) dirty.add(node);
    }
  }
  function* inspectRecords(list) {
    for (const record of list) {
      pendingRecords--;
      yield;
      const target = record.target.nodeType === 1 ? record.target : record.target.parentElement;
      if (target === document.documentElement && record.type === 'childList') {
        for (const nodes of [record.removedNodes, record.addedNodes]) {
          for (const node of nodes) {
            yield;
            if (node.nodeName === 'BODY') roots.add(node);
          }
        }
        continue;
      }
      for (let node = target, post; node && (post = node.closest(POST)); node = post.parentElement) {
        yield;
        dirty.add(post);
      }
      if (record.type === 'attributes') {
        if (results.has(target)) dirty.add(target);
        if (record.attributeName === 'data-testid' && (target.matches(CELL) || record.oldValue === 'cellInnerDiv')) roots.add(target);
      } else if (record.type === 'childList') {
        for (const nodes of [record.removedNodes, record.addedNodes]) {
          for (const node of nodes) {
            yield;
            if (node.nodeType === 1) roots.add(node);
          }
        }
      }
    }
  }
  function* recheckMounted() {
    for (const post of results.keys()) {
      yield;
      if (!inBody(post) || !post.matches(POST)) dirty.add(post);
    }
    if (document.body) yield* discoverPosts(document.body);
  }
  function* checkPost(post) {
    if (!inBody(post) || !post.matches(POST)) {
      remember(post, undefined);
      return;
    }
    stats.checked++;
    const ad = yield* promoted(post);
    if (!dirty.has(post) && inBody(post) && post.matches(POST)) remember(post, ad ? post.closest(CELL) || post : null);
  }
  const take = (queue) => {
    const item = queue.values().next().value;
    queue.delete(item);
    return item;
  };
  const pending = () => stopped ? marked.size : records || batch < batches.length || resync || discover || roots.size || check || dirty.size;
  const later = () => { if (!timer) timer = setTimeout(run, 0); };
  function run() {
    timer = 0;
    if (!stopped && !on()) {
      stopped = true;
      observer.disconnect();
      roots.clear(); dirty.clear(); results.clear(); batches.length = 0;
      records = discover = check = null;
    }
    const deadline = performance.now() + SLICE_MS;
    stats.scans++;
    for (let work = 0; pending() && work < SLICE_UNITS && performance.now() < deadline; work++) {
      if (stopped) {
        const cell = marked.keys().next().value;
        marked.delete(cell);
        cell.removeAttribute(MARK);
        continue;
      }
      if (!records && batch < batches.length) {
        records = inspectRecords(batches[batch]);
        batches[batch++] = null;
        if (batch === batches.length) { batches.length = 0; batch = 0; }
      }
      if (records && records.next().done) records = null;
      if (!discover && resync) { resync = false; discover = recheckMounted(); }
      if (!discover && roots.size) discover = discoverPosts(take(roots));
      if (discover && discover.next().done) discover = null;
      if (!check && dirty.size) check = checkPost(take(dirty));
      if (check && check.next().done) check = null;
    }
    stats.posts = results.size;
    if (marked.size !== stats.hidden) {
      stats.hidden = marked.size;
      console.debug('[bowser] x promoted posts hidden: ' + stats.hidden + ' of ' + stats.posts);
    }
    if (pending()) later();
    else if (stopped) {
      window.__bowserXPromoted = null;
      // The engine may have restored the style while we were removing old marks.
      if (on()) start();
    }
  }
  let body;
  const observeBody = () => {
    body = document.body;
    if (body) observer.observe(body, {
      childList: true, subtree: true, characterData: true,
      attributes: true, attributeOldValue: true, attributeFilter: ['data-testid', 'role'],
    });
  };
  const observer = new MutationObserver((changes) => {
    if (!on()) { later(); return; }
    if (body !== document.body) {
      observer.disconnect();
      observer.observe(document.documentElement, { childList: true });
      observeBody();
    }
    // A huge React replacement can deliver thousands of mutations in one callback. Coalesce
    // overflow into one cooperative discovery pass instead of retaining every old mutation.
    if (pendingRecords + changes.length > MAX_PENDING_RECORDS || batch >= MAX_PENDING_RECORDS) {
      records = null; batches.length = 0; batch = pendingRecords = 0;
      roots.clear();
      resync = true;
    } else {
      batches.push(changes);
      pendingRecords += changes.length;
    }
    later();
  });
  function start() {
    stopped = false;
    batch = pendingRecords = 0;
    resync = false;
    window.__bowserXPromoted = { stats };
    observer.observe(document.documentElement, { childList: true });
    observeBody();
    if (document.body) roots.add(document.body);
    later();
  }
  if (document.body) start();
  else document.addEventListener('DOMContentLoaded', start, { once: true });
})()
