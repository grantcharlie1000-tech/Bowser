// SFW.adapters — data-driven platform adapters plus the generic resolver (DESIGN §5.5).
//
// Adapter = { id, highRisk, item, own, generic, thread, context, media, attrs, hotUrl(url) -> 0|1|2 }
//   item     selector list of units (adapter selectors merged with the generic fallback)
//   own      adapter-specific item selectors only (priority when resolving a unit)
//   thread   selector for the thread / conversation root whose context defines hotness for everything inside
//   context  selectors (searched inside the thread root, else document-wide) whose text/value defines hotness:
//            subreddit name, video title, search box value ...
//   media    media selector (adapter specific + generic)
//   attrs    extra attribute names whose text is part of a unit's text (custom-element data attributes)
//   hotUrl   0 cold | 1 warm | 2 hot, from the URL alone (query / path slug / subreddit) via SFW.classify
// Selector drift degrades to the generic resolver: `own` simply stops matching and `generic` keeps applying.
SFW.adapters = (function () {
  const GENERIC_ITEM = ["article", "[role=article]", "li", "[role=listitem]", "[role=option]", "blockquote", "figure", "tr", "dd"];
  const GENERIC_MEDIA = ["img", "picture", "video", "canvas", "object", "embed", "svg image"];
  const GENERIC_CONTEXT = [];
  const SEARCH_ROWS = ["[role=option]", "li[role=presentation]", "[role=listbox] li"];

  function classifyUrl(url) {
    try {
      const r = SFW.classify.url(url);
      if (!r) return 0;
      if (r.flags && (r.flags.franchise || r.flags.entity)) return 2;
      if (r.level === "spoiler" || r.level === "mention") return 2;
      if (r.level === "risk") return 1;
    } catch (e) { /* ignore */ }
    return 0;
  }

  function hotTextWord(s) {
    try { return !!SFW.classify.isHotText(s); } catch (e) { return false; }
  }

  function parse(url) {
    try { return new URL(url, "https://example.invalid/"); } catch (e) { return null; }
  }

  // Subreddits that are not GTA VI specific but are where GTA VI talk concentrates.
  const REDDIT_WARM = /^(gta|gtav|gtaonline|gtaleaks|rockstar|rockstargames|rockstargamessupport|gamingleaksandrumours|gamingleaks|gtaonlinegirls|nextgengaming)$/i;

  function redditHot(url) {
    const u = parse(url);
    let best = classifyUrl(url);
    if (!u) return best;
    const m = /^\/r\/([^/]+)/i.exec(u.pathname);
    if (m) {
      const name = decodeURIComponent(m[1]).replace(/[_-]+/g, " ");
      if (hotTextWord(name) || hotTextWord(m[1])) best = 2;
      else if (REDDIT_WARM.test(m[1]) && best < 1) best = 1;
    }
    return best;
  }

  const A = [];
  function def(a) { A.push(a); }

  // ---- X / Twitter --------------------------------------------------------------------------------------------
  def({
    id: "x", threadUrl: /\/status\/\d+/, hosts: /(^|\.)(x|twitter)\.com$/i, highRisk: true,
    item: ["article[data-testid=tweet]", "[data-testid=cellInnerDiv]", "[data-testid=typeaheadResult]", "[data-testid=trend]", "[data-testid=UserCell]", "[data-testid='card.wrapper']", "[data-testid=conversation]", "[data-testid=DMDrawer] [role=listitem]", "[role=listbox] [role=option]"],
    thread: ["[aria-label^='Timeline: Conversation']", "[data-testid=primaryColumn] section[role=region]"],
    context: ["[data-testid=SearchBox_Search_Input]", "[data-testid=tweetText]"],
    media: ["[data-testid=tweetPhoto]", "[data-testid=videoPlayer]", "[data-testid='card.layoutLarge.media']", "[data-testid='card.layoutSmall.media']"],
    attrs: [],
    hotUrl: classifyUrl,
  });
  // ---- Reddit (shreddit, sh.reddit, old.reddit) ---------------------------------------------------------------
  def({
    id: "reddit", threadUrl: /\/comments\//, hosts: /(^|\.)(reddit\.com|redd\.it|redditmedia\.com)$/i, highRisk: true,
    item: ["shreddit-post", "shreddit-comment", "faceplate-tracker", "search-telemetry-tracker", "reddit-search-large", "shreddit-search-suggestion",
      ".thing", "div.search-result", "li.search-result", "li[role=presentation]", "[data-testid=post-container]", "[data-testid=comment]", "div.Comment", "div.Post"],
    thread: ["shreddit-post", "div.thing.link", "#siteTable > .thing", "[data-testid=post-container]", "main"],
    context: ["shreddit-post[post-title]", "shreddit-subreddit-header", "h1", "[data-testid=subreddit-name]", "#header-bottom-left .redditname", "input[name=q]", "faceplate-search-input"],
    media: ["shreddit-player", "shreddit-aspect-ratio", "gallery-carousel", "[slot=post-media-container]", "a.thumbnail", "img.preview"],
    attrs: ["post-title", "subreddit-prefixed-name", "subreddit-name", "permalink", "content-href", "author", "aria-label", "title", "alt"],
    hotUrl: redditHot,
  });
  // ---- YouTube -----------------------------------------------------------------------------------------------
  def({
    id: "youtube", threadUrl: /^\/(watch|shorts\/|live\/)/, hosts: /(^|\.)(youtube\.com|youtu\.be|youtube-nocookie\.com)$/i, highRisk: true,
    item: ["ytd-rich-item-renderer", "ytd-video-renderer", "ytd-compact-video-renderer", "ytd-reel-item-renderer", "ytd-comment-thread-renderer", "ytd-comment-view-model", "ytd-playlist-renderer",
      "ytd-grid-video-renderer", "ytd-channel-renderer", "ytd-shelf-renderer", "ytm-shorts-lockup-view-model", "yt-lockup-view-model", "ytd-watch-metadata", "ytd-watch-info-text", "#movie_player", "ytd-player",
      ".ytp-caption-window-container", ".captions-text", "ytd-engagement-panel-section-list-renderer", "li.sbsb_c", "[role=option]", "ytd-notification-renderer"],
    thread: ["ytd-watch-flexy", "ytd-watch-grid", "ytd-search", "ytd-browse"],
    context: ["ytd-watch-metadata h1", "h1.ytd-watch-metadata", "#title h1", "ytd-watch-metadata #description", "#description-inner", "input#search", "input[name=search_query]", "yt-searchbox input", "ytd-channel-name"],
    media: ["ytd-thumbnail", "yt-image", "yt-img-shadow", "#movie_player", ".ytp-cued-thumbnail-overlay-image"],
    attrs: ["title", "aria-label", "alt"],
    hotUrl: classifyUrl,
  });
  // ---- Search engines -----------------------------------------------------------------------------------------
  def({
    id: "google", hosts: /(^|\.)google\.[a-z.]{2,6}$/i, highRisk: true,
    item: ["div.g", "div.MjjYud", "div.tF2Cxc", "div[data-hveid]", "div[data-attrid]", "g-card", "g-inner-card", "div.vt6azd", "div.PmEWq", "div.related-question-pair", "[jsname=yEVEwb]", "div.IsZvec"].concat(SEARCH_ROWS, ["li.sbct", "li.sbsb_c", "div[data-ved] a"]),
    thread: ["#search", "#rso", "#main"],
    context: ["textarea[name=q]", "input[name=q]", "#APjFqb", "#searchform input"],
    media: ["g-img", "img", "div[data-lpage]", "#imagesContainer img", "[data-docid]"],
    attrs: ["aria-label", "title", "alt"],
    hotUrl: classifyUrl,
  });
  def({
    id: "bing", hosts: /(^|\.)bing\.com$/i, highRisk: true,
    item: ["li.b_algo", "li.b_ans", "li.b_ad", "div.b_ans", "li.b_algoGroup", "div.b_rich", "li.sa_sg", "li[role=option]", "div.mc_vtvc", "div.b_imgcap_altitle", "div.iusc", "div.richrsrailsugwrapper", "li.b_ans .b_vlist2col li"].concat(SEARCH_ROWS),
    thread: ["#b_results", "#b_content"],
    context: ["#sb_form_q", "textarea[name=q]", "input[name=q]"],
    media: ["div.cico", "div.mc_vtvc_con", "div.vthumb", "img.rms_img"],
    attrs: ["aria-label", "title", "alt"],
    hotUrl: classifyUrl,
  });
  def({
    id: "ddg", hosts: /(^|\.)duckduckgo\.com$/i, highRisk: true,
    item: ["article[data-testid=result]", "li[data-layout]", "div.result", "div.results_links", "li.sa", "div.tile", "div.module", ".acp"].concat(SEARCH_ROWS),
    thread: ["#links", "#web_content_wrapper", "section[data-testid=mainline]"],
    context: ["input[name=q]", "#search_form_input", "#searchbox_input"],
    media: [".tile--img", ".result__icon", "img"],
    attrs: ["aria-label", "title", "alt"],
    hotUrl: classifyUrl,
  });
  def({
    id: "yahoo", hosts: /(^|\.)(search\.)?yahoo\.[a-z.]{2,6}$/i, highRisk: true,
    item: ["div.algo", "div.dd.algo", "li .dd", "div.compTitle", "div.relsrch", "li[role=presentation]", "div.sw-CardBase", "div.NewsArticle", "li.first"].concat(SEARCH_ROWS),
    thread: ["#web", "#results", "#main"],
    context: ["input[name=p]", "#yschsp", "input[name=q]"],
    media: ["div.thmb", "img"],
    attrs: ["aria-label", "title", "alt"],
    hotUrl: function (url) {
      const u = parse(url);
      let best = classifyUrl(url);
      if (u && u.searchParams.has("p") && hotTextWord(u.searchParams.get("p"))) best = 2;
      return best;
    },
  });
  def({
    id: "brave", hosts: /(^|\.)search\.brave\.com$/i, highRisk: true,
    item: ["div.snippet", "#results .snippet", ".fdb", ".card", "div.video-snippet", "div[data-type=web]", "div[data-type=news]", "div.infobox", "div.qa-snippet", "[data-type=suggestion]"].concat(SEARCH_ROWS),
    thread: ["#results", "#main"],
    context: ["input#searchbox", "input[name=q]", "textarea[name=q]"],
    media: [".thumb", "img"],
    attrs: ["aria-label", "title", "alt"],
    hotUrl: classifyUrl,
  });
  def({
    id: "startpage", hosts: /(^|\.)startpage\.com$/i, highRisk: true,
    item: ["div.result", "div.w-gl__result", "div.wgl-result", "div.w-gl__result-wrapper", "div.w-gl--default", "div.suggestions li"].concat(SEARCH_ROWS),
    thread: ["#main", ".mainline-results"],
    context: ["input#q", "input[name=query]", "input[name=q]"],
    media: ["img", "div.image-result"],
    attrs: ["aria-label", "title", "alt"],
    hotUrl: function (url) {
      const u = parse(url);
      let best = classifyUrl(url);
      if (u && u.searchParams.has("query") && hotTextWord(u.searchParams.get("query"))) best = 2;
      return best;
    },
  });
  // ---- Social / video / chat ------------------------------------------------------------------------------------
  def({
    id: "tiktok", hosts: /(^|\.)tiktok\.com$/i, highRisk: true,
    item: ["[data-e2e=recommend-list-item-container]", "[data-e2e=search-card-item]", "[data-e2e=search_top-item]", "[data-e2e=user-post-item]", "[data-e2e^=comment-level]", "[data-e2e=comment-item]", "[data-e2e=search-user-container]", "div[class*=DivItemContainer]", "div[class*=DivCommentItemContainer]", "[data-e2e=browse-video-desc]", "[data-e2e=video-desc]"],
    thread: ["[data-e2e=browse-video]", "[data-e2e=video-detail]", "[data-e2e=search-card-list]"],
    context: ["[data-e2e=browse-video-desc]", "[data-e2e=video-desc]", "input[type=search]", "input[data-e2e=search-user-input]"],
    media: ["video", "img", "[data-e2e=video-player]"], attrs: ["aria-label", "alt", "title"],
    hotUrl: classifyUrl,
  });
  def({
    id: "instagram", hosts: /(^|\.)instagram\.com$/i, highRisk: true,
    item: ["article", "div[role=dialog] ul li", "a[href*='/p/']", "a[href*='/reel/']", "li[role=menuitem]", "div[role=button] div._ac7v", "[role=listbox] [role=option]"],
    thread: ["article", "div[role=dialog]"],
    context: ["article h1", "input[type=text][placeholder]", "input[aria-label*=Search]"],
    media: ["video", "img", "div[role=button] img"], attrs: ["alt", "aria-label", "title"],
    hotUrl: classifyUrl,
  });
  def({
    id: "facebook", hosts: /(^|\.)(facebook\.com|fb\.com|fb\.watch)$/i, highRisk: true,
    item: ["[role=article]", "div[role=feed] > div", "div[data-pagelet^=FeedUnit]", "div[data-pagelet*=Feed]", "div[aria-posinset]", "[role=listitem]", "li[role=presentation]", "[role=option]", "a[href*='/watch/?v=']", "a[href*='/reel/']"],
    thread: ["[role=article]", "div[role=main]"],
    context: ["input[type=search]", "div[data-ad-preview=message]", "h1"],
    media: ["video", "img", "div[data-visualcompletion=media-vc-image]"], attrs: ["aria-label", "alt", "title"],
    hotUrl: classifyUrl,
  });
  def({
    id: "threads", hosts: /(^|\.)threads\.(net|com)$/i, highRisk: true,
    item: ["div[data-pressable-container=true]", "[data-pagelet^=threads_post]", "article", "div[role=listitem]", "a[href*='/post/']"],
    thread: ["div[role=main]", "main"], context: ["input[type=search]", "h1"],
    media: ["video", "img"], attrs: ["alt", "aria-label", "title"],
    hotUrl: classifyUrl,
  });
  def({
    id: "bluesky", hosts: /(^|\.)bsky\.(app|social)$/i, highRisk: true,
    item: ["[data-testid^=feedItem-by-]", "[data-testid^=postThreadItem-by-]", "[data-testid^=searchResult]", "[data-testid=searchAutoCompleteResult]", "[data-testid^=profileCard]", "[data-testid=postText]", "[role=link][aria-label*=post]"],
    thread: ["[data-testid=postThreadScreen]", "[data-testid=searchScreen]", "main"],
    context: ["input[type=search]", "[data-testid=searchTextInput]"],
    media: ["[data-testid=postEmbed]", "video", "img"], attrs: ["aria-label", "alt", "title"],
    hotUrl: classifyUrl,
  });
  def({
    id: "twitch", hosts: /(^|\.)twitch\.tv$/i, highRisk: true,
    item: [".chat-line__message", "[data-a-target=chat-line-message]", "article", "[data-a-target^=video-tower-card]", "[data-a-target=card-title]", ".video-card", "[data-a-target=search-result-live-channel]", "[role=listitem]", "[role=option]", "[data-a-target=stream-title]", "[data-a-target=player-overlay-title]"],
    thread: ["[data-test-selector=video-player__video-layout]", "main", "[data-a-target=search-result-section]"],
    context: ["[data-a-target=stream-title]", "h1", "input[type=search]", "[data-a-target=tw-input]"],
    media: ["video", "img", ".tw-image"], attrs: ["aria-label", "alt", "title"],
    hotUrl: classifyUrl,
  });
  def({
    id: "discord", hosts: /(^|\.)discord(app)?\.com$/i, highRisk: true,
    item: ["li[id^=chat-messages-]", "[class*=messageListItem]", "[class*=message_]", "[role=article]", "[class*=searchResult]", "[class*=embedWrapper]", "[role=option]", "[class*=threadMessage]"],
    thread: ["[class*=chatContent]", "main[class*=chatContent]", "[data-list-id=chat-messages]"],
    context: ["h1", "[class*=title_] h1", "[class*=channelName]", "[class*=searchBar] [class*=public-DraftEditor-content]"],
    media: ["[class*=imageWrapper]", "[class*=embedVideo]", "video", "img"], attrs: ["aria-label", "alt", "title"],
    hotUrl: classifyUrl,
  });
  // Mastodon: any host, detected from markup (see detect()).
  const MASTODON = {
    id: "mastodon", hosts: null, highRisk: true,
    item: [".status", "article.status", ".detailed-status", ".status__content", "article[data-id]", ".account-card", ".search-results__section .status", ".trends__item", ".account", "[role=feed] article", ".notification"],
    thread: [".detailed-status", ".status-public", ".scrollable.detailed-status__wrapper"],
    context: [".detailed-status .status__content", ".hashtag-header", ".column-header__title", ".search__input"],
    media: [".media-gallery", ".status-card__image", ".video-player", ".audio-player", ".media-gallery__item", "img"],
    attrs: ["title", "alt", "aria-label"],
    hotUrl: classifyUrl,
  };

  const GENERIC = {
    id: "generic", hosts: null, highRisk: false,
    item: [], thread: [], context: GENERIC_CONTEXT, media: [], attrs: [], hotUrl: classifyUrl,
  };

  function detect(doc) {
    try {
      if (doc && doc.querySelector && doc.querySelector("#mastodon, .app-holder[data-react-class], .status__content, noscript[class*=mastodon], meta[name='application-name'][content=Mastodon]")) return MASTODON;
    } catch (e) { /* ignore */ }
    return null;
  }

  function join(list) { return list.filter(Boolean).join(","); }

  function compile(def, doc) {
    const own = valid(def.item, doc);
    const generic = valid(GENERIC_ITEM, doc);
    const item = own.concat(generic);
    return {
      id: def.id,
      highRisk: !!def.highRisk,
      own: join(own),
      generic: join(generic),
      item: join(item),
      thread: join(valid(def.thread, doc)),
      context: join(valid(def.context, doc)),
      media: join(valid(def.media.concat(GENERIC_MEDIA), doc)),
      attrs: def.attrs || [],
      hotUrl: def.hotUrl,
    };
  }

  // Drop selectors the engine rejects (the lists are hand-written; one typo must not disable protection).
  function valid(list, doc) {
    if (!doc || !doc.createDocumentFragment) return list.slice();
    const probe = doc.createDocumentFragment();
    const out = [];
    for (const s of list) {
      try { probe.querySelector(s); out.push(s); } catch (e) { /* drop */ }
    }
    return out;
  }

  const cache = new Map();

  // forLocation(locationLike, doc?) -> Adapter. `doc` (optional) enables selector validation and markup detection.
  function forLocation(loc, doc) {
    const host = String((loc && (loc.hostname || (loc.host || "").replace(/:\d+$/, ""))) || "").toLowerCase();
    let def = null;
    for (const a of A) if (a.hosts && a.hosts.test(host)) { def = a; break; }
    if (!def) def = detect(doc) || GENERIC;
    const key = def.id + (doc ? ":v" : "");
    let c = cache.get(key);
    if (!c) { c = compile(def, doc); cache.set(key, c); }
    return c;
  }

  function list() { return A.map((a) => a.id).concat(["mastodon", "generic"]); }
  function definition(id) { return A.find((a) => a.id === id) || (id === "mastodon" ? MASTODON : GENERIC); }

  return { forLocation, detect, list, definition, GENERIC_ITEM, GENERIC_MEDIA, REDDIT_WARM };
})();
