// SFW.adapters — data-driven platform adapters plus the generic resolver (DESIGN §5.5).
//
// Adapter = { id, highRisk, item, own, generic, thread, context, media, attrs, hotUrl(url) -> 0|1|2 }
//   item     selector list of units (adapter selectors merged with the generic fallback)
//   own      adapter-specific item selectors only (priority when resolving a unit)
//   thread   selector for the thread / conversation root whose context defines hotness for everything inside
//   context  PAGE-LEVEL selectors whose text/value defines hotness for the whole page: search box value, page or
//            channel heading, subreddit name. Never a selector that matches every post of a feed: one post that names
//            the game must not turn every other post on the page into a suspect.
//   threadContext  selectors searched inside a thread root (defaults to context): the thread's own title / caption
//   threadFirst    the thread root's first adapter item is its head (the post being replied to) and defines the
//            thread's hotness. Only for real thread pages (threadUrl) or per-post roots: on a feed, the "first item"
//            of <main> is just whichever post happens to be on top.
//   media    media selector (adapter specific + generic)
//   attrs    extra attribute names whose text is part of a unit's text (custom-element data attributes)
//   avatars  profile pictures. Never part of what is checked: they are not quarantined, never held for OCR (no
//            "Checking image" veil, no "couldn't check" cover) and never make a unit a picture post. In a GTA context
//            (a spoiler thread, a search for the game) every clear reply used to lose its profile picture, because media
//            there is only released for cold units. A covered post still covers its picture with the rest of the post.
//            Keep the lists to selectors that only ever match profile pictures: anything they match shows unchecked.
//   mediaIgnore  other images that do not make a unit a picture post (emoji images every post of the site carries); the
//            avatars are added to it. Without it every post on X "has media", and media in any GTA context (a spoiler
//            post opened as the thread head, a spoiler reply's own page) is covered: one spoiler covered every post.
//   ocr      false: the site's pictures are judged by the words around them (titles, descriptions), never held for the OCR
//            gate. YouTube: every thumbnail sat under "Checking image" until OCR had read it, seconds on a busy page.
//   exempt   { sel, live } regions that are never checked while `live` (a selector) matches the page: the player of a live
//            YouTube stream plays uncovered; its title, description, comments and chat are still checked.
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
    context: ["[data-testid=SearchBox_Search_Input]"], threadFirst: true,
    media: ["[data-testid=tweetPhoto]", "[data-testid=videoPlayer]", "[data-testid='card.layoutLarge.media']", "[data-testid='card.layoutSmall.media']"],
    attrs: [],
    avatars: ["[data-testid=Tweet-User-Avatar] *", "[data-testid^=UserAvatar-Container] *", "img[src*='/profile_images/']", "[style*='/profile_images/']"],
    mediaIgnore: ["img[src*='/emoji/']", "img[src*='/hashflags/']"],
    hotUrl: classifyUrl,
  });
  // ---- Reddit (shreddit, sh.reddit, old.reddit) ---------------------------------------------------------------
  def({
    id: "reddit", threadUrl: /\/comments\//, hosts: /(^|\.)(reddit\.com|redd\.it|redditmedia\.com)$/i, highRisk: true,
    item: ["shreddit-post", "shreddit-comment", "faceplate-tracker", "search-telemetry-tracker", "reddit-search-large", "shreddit-search-suggestion",
      ".thing", "div.search-result", "li.search-result", "li[role=presentation]", "[data-testid=post-container]", "[data-testid=comment]", "div.Comment", "div.Post"],
    thread: ["shreddit-post", "div.thing.link", "#siteTable > .thing", "[data-testid=post-container]", "main"],
    context: ["shreddit-subreddit-header", "[data-testid=subreddit-name]", "#header-bottom-left .redditname", "input[name=q]", "faceplate-search-input"],
    threadContext: ["shreddit-post[post-title]", "h1"], threadFirst: true,
    media: ["shreddit-player", "shreddit-aspect-ratio", "gallery-carousel", "[slot=post-media-container]", "a.thumbnail", "img.preview"],
    avatars: ["[slot=commentAvatar] *", "img[src*='redditstatic.com/avatars/']", "img[src*='/profileIcon_']", "img[src*='/snoovatar/']"],
    attrs: ["post-title", "subreddit-prefixed-name", "subreddit-name", "permalink", "content-href", "author", "aria-label", "title", "alt"],
    hotUrl: redditHot,
  });
  // ---- YouTube -----------------------------------------------------------------------------------------------
  const YOUTUBE_CARDS = ["ytd-rich-item-renderer", "ytd-rich-grid-media", "ytd-video-renderer", "ytd-compact-video-renderer",
    "ytd-grid-video-renderer", "ytd-playlist-renderer", "ytd-playlist-panel-video-renderer", "ytd-reel-item-renderer",
    "yt-lockup-view-model", "yt-lockup-view-model-wiz", "ytm-compact-video-renderer", "ytm-video-with-context-renderer",
    "ytm-compact-playlist-renderer", "ytm-shorts-lockup-view-model",
    ".ytp-ce-element", ".ytp-videowall-still", ".ytp-suggestion-link"];
  def({
    id: "youtube", threadUrl: /^\/(watch|shorts\/|live\/)/, hosts: /(^|\.)(youtube\.com|youtu\.be|youtube-nocookie\.com)$/i, highRisk: true,
    item: YOUTUBE_CARDS.concat(["ytd-comment-thread-renderer", "ytd-comment-view-model",
      "ytd-channel-renderer", "ytd-shelf-renderer", "ytd-watch-metadata", "ytd-watch-info-text", "#movie_player", ".html5-video-player",
      ".ytp-caption-window-container", ".captions-text", "ytd-engagement-panel-section-list-renderer", "li.sbsb_c", "[role=option]", "ytd-notification-renderer",
      "yt-live-chat-text-message-renderer", "yt-live-chat-paid-message-renderer", "yt-live-chat-membership-item-renderer", "yt-live-chat-banner-renderer"]),
    cards: YOUTUBE_CARDS,
    thread: ["ytd-watch-flexy", "ytd-watch-grid", "ytd-search", "ytd-browse"],
    context: ["ytd-watch-metadata h1", "h1.ytd-watch-metadata", "#title h1", "ytd-watch-metadata #description", "#description-inner"],
    media: ["ytd-thumbnail", "yt-image", "yt-img-shadow", "#movie_player", ".ytp-cued-thumbnail-overlay-image"],
    avatars: ["#avatar img", "#avatar-link img", "#author-thumbnail img", "#channel-thumbnail img", "#author-photo img", "#avatar-btn img", "yt-avatar-shape img", "yt-decorated-avatar-view-model img", "img.yt-spec-avatar-shape__image"],
    attrs: ["title", "aria-label", "alt"],
    ocr: false,
    exempt: { live: ".html5-video-player.ytp-live", sel: ["#movie_player", "ytd-player", "#player-container", ".html5-video-player", ".ytp-caption-window-container", ".captions-text"] },
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
    thread: ["[data-e2e=browse-video]", "[data-e2e=video-detail]"],
    context: ["[data-e2e=browse-video-desc]", "input[type=search]", "input[data-e2e=search-user-input]"],
    threadContext: ["[data-e2e=browse-video-desc]", "[data-e2e=video-desc]"],
    media: ["video", "img", "[data-e2e=video-player]"],
    avatars: ["[data-e2e*=avatar] img", "[data-e2e*=avatar] [style*=background-image]"], attrs: ["aria-label", "alt", "title"],
    hotUrl: classifyUrl,
  });
  def({
    id: "instagram", hosts: /(^|\.)instagram\.com$/i, highRisk: true,
    item: ["article", "div[role=dialog] ul li", "a[href*='/p/']", "a[href*='/reel/']", "li[role=menuitem]", "div[role=button] div._ac7v", "[role=listbox] [role=option]"],
    thread: ["article", "div[role=dialog]"],
    context: ["article h1", "input[type=text][placeholder]", "input[aria-label*=Search]"],
    media: ["video", "img", "div[role=button] img"],
    avatars: ["img[alt$='profile picture']", "img[alt$='profile picture.']"], attrs: ["alt", "aria-label", "title"],
    hotUrl: classifyUrl,
  });
  def({
    id: "facebook", hosts: /(^|\.)(facebook\.com|fb\.com|fb\.watch)$/i, highRisk: true,
    item: ["[role=article]", "div[role=feed] > div", "div[data-pagelet^=FeedUnit]", "div[data-pagelet*=Feed]", "div[aria-posinset]", "[role=listitem]", "li[role=presentation]", "[role=option]", "a[href*='/watch/?v=']", "a[href*='/reel/']"],
    thread: ["[role=article]"],
    context: ["input[type=search]", "h1"],
    threadContext: ["div[data-ad-preview=message]", "h1"],
    media: ["video", "img", "div[data-visualcompletion=media-vc-image]"],
    avatars: ["img[alt$='profile picture']"], attrs: ["aria-label", "alt", "title"],
    hotUrl: classifyUrl,
  });
  def({
    id: "threads", threadUrl: /\/post\//, threadFirst: true, hosts: /(^|\.)threads\.(net|com)$/i, highRisk: true,
    item: ["div[data-pressable-container=true]", "[data-pagelet^=threads_post]", "article", "div[role=listitem]", "a[href*='/post/']"],
    thread: ["div[role=main]", "main"], context: ["input[type=search]", "h1"],
    media: ["video", "img"],
    avatars: ["img[alt$='profile picture']"], attrs: ["alt", "aria-label", "title"],
    hotUrl: classifyUrl,
  });
  def({
    id: "bluesky", threadUrl: /\/post\//, threadFirst: true, hosts: /(^|\.)bsky\.(app|social)$/i, highRisk: true,
    item: ["[data-testid^=feedItem-by-]", "[data-testid^=postThreadItem-by-]", "[data-testid^=searchResult]", "[data-testid=searchAutoCompleteResult]", "[data-testid^=profileCard]", "[data-testid=postText]", "[role=link][aria-label*=post]"],
    thread: ["[data-testid=postThreadScreen]", "[data-testid=searchScreen]", "main"],
    context: ["input[type=search]", "[data-testid=searchTextInput]"],
    media: ["[data-testid=postEmbed]", "video", "img"],
    avatars: ["[data-testid=userAvatarImage] *", "img[src*='/img/avatar/']", "img[src*='/img/avatar_thumbnail/']"], attrs: ["aria-label", "alt", "title"],
    hotUrl: classifyUrl,
  });
  def({
    id: "twitch", hosts: /(^|\.)twitch\.tv$/i, highRisk: true,
    item: [".chat-line__message", "[data-a-target=chat-line-message]", "article", "[data-a-target^=video-tower-card]", "[data-a-target=card-title]", ".video-card", "[data-a-target=search-result-live-channel]", "[role=listitem]", "[role=option]", "[data-a-target=stream-title]", "[data-a-target=player-overlay-title]"],
    thread: ["[data-test-selector=video-player__video-layout]", "main", "[data-a-target=search-result-section]"],
    context: ["[data-a-target=stream-title]", "h1", "input[type=search]", "[data-a-target=tw-input]"],
    media: ["video", "img", ".tw-image"],
    avatars: [".tw-avatar img", "img.tw-image-avatar"], attrs: ["aria-label", "alt", "title"],
    hotUrl: classifyUrl,
  });
  def({
    id: "discord", hosts: /(^|\.)discord(app)?\.com$/i, highRisk: true,
    item: ["li[id^=chat-messages-]", "[class*=messageListItem]", "[class*=message_]", "[role=article]", "[class*=searchResult]", "[class*=embedWrapper]", "[role=option]", "[class*=threadMessage]"],
    thread: ["[class*=chatContent]", "main[class*=chatContent]", "[data-list-id=chat-messages]"],
    context: ["h1", "[class*=title_] h1", "[class*=channelName]", "[class*=searchBar] [class*=public-DraftEditor-content]"],
    media: ["[class*=imageWrapper]", "[class*=embedVideo]", "video", "img"],
    avatars: ["img[src*='/avatars/'][src*='discordapp']", "img[src*='/embed/avatars/']", "img[class*=avatar]", "[class*=avatar] img"], attrs: ["aria-label", "alt", "title"],
    hotUrl: classifyUrl,
  });
  // ---- Image boards, forums, chat and press: where GTA VI leaks and recaps land --------------------------------
  // Selector lists are best effort (drift degrades to the generic resolver). What matters is highRisk: media is held
  // until it was checked, URLs and titles are classified cold, and the OCR allowlist (src/spoiler_ocr.rs, hosts/chrome)
  // covers the same hosts. Top-level image documents (https://i.imgur.com/x.png) are handled by the runtime.
  def({
    id: "imgur", threadUrl: /^\/(gallery|a|t)\//, threadFirst: true, hosts: /(^|\.)imgur\.com$/i, highRisk: true,
    item: ["div.Post", "div.post-container", "[class*=Gallery-Content]", "[class*=PostContent]", "div.Comment", "[class*=Comment-]", "div.post"],
    thread: ["div.Gallery", "[class*=Gallery-Content]", "main"], context: ["h1", "[class*=Gallery-Title]", "input[type=search]"],
    media: ["[class*=PostImage]", "[class*=imageContainer]", "video"], attrs: ["alt", "title", "aria-label"],
    hotUrl: classifyUrl,
  });
  def({
    id: "fourchan", threadFirst: true, hosts: /(^|\.)(4chan|4channel|4cdn)\.org$/i, highRisk: true,
    item: [".postContainer", ".post", ".thread", ".catalog-thread", "div.thread", ".teaser", ".fileText"],
    thread: [".thread", "#threads", "form[name=delform]"], context: ["#boardNavDesktop", ".subject", "title", "input[type=search]"],
    media: [".fileThumb", ".file", "img", "video"], attrs: ["alt", "title"],
    hotUrl: classifyUrl,
  });
  def({
    id: "gtaforums", threadUrl: /\/topic\//, threadFirst: true, hosts: /(^|\.)gtaforums\.com$/i, highRisk: true,
    item: ["article.cPost", "article[id^=elComment_]", ".ipsComment", "li.ipsDataItem", ".ipsStreamItem", "[data-commentid]", ".cTopic"],
    thread: ["[data-controller='core.front.core.commentFeed']", ".cTopic", "main"], context: ["h1", ".ipsType_pageTitle", "input[type=search]"],
    media: [".ipsImage", ".ipsAttachLink_image", "video", "img"],
    avatars: [".ipsUserPhoto img", "img.ipsUserPhoto"], attrs: ["alt", "title", "aria-label"],
    hotUrl: classifyUrl,
  });
  def({
    id: "resetera", threadUrl: /\/threads\//, threadFirst: true, hosts: /(^|\.)resetera\.com$/i, highRisk: true,
    item: ["article.message", ".message", "li.structItem", ".structItem", ".block-row", ".message-body"],
    thread: [".block--messages", ".p-body-pageContent", "main"], context: ["h1.p-title-value", "h1", "input[name=keywords]"],
    media: [".bbImage", ".bbMediaWrapper", "video", "img"],
    avatars: [".avatar img", "img.avatar"], attrs: ["alt", "title", "aria-label"],
    hotUrl: classifyUrl,
  });
  def({
    id: "telegram", hosts: /(^|\.)(telegram\.org|t\.me)$/i, highRisk: true,
    item: [".Message", ".message", ".bubble", ".ListItem", ".search-result", ".tgme_widget_message", "[class*=Message_]", "[role=listitem]"],
    thread: ["#MiddleColumn", ".MessageList", "#column-center", "main"], context: ["h3.fullName", ".ChatInfo", "input[type=search]"],
    media: [".media-inner", ".Photo", ".media-photo", ".media-video", "video", "img"],
    avatars: [".Avatar img", ".avatar img", "img.avatar-photo"], attrs: ["alt", "title", "aria-label"],
    hotUrl: classifyUrl,
  });
  def({
    id: "kick", hosts: /(^|\.)kick\.com$/i, highRisk: true,
    item: ["[class*=chat-entry]", "[data-chat-entry]", "[class*=stream-card]", "[class*=video-card]", "[role=listitem]", "[role=option]", "article"],
    thread: ["main", "#chatroom", "[class*=chatroom]"], context: ["h1", "input[type=search]"],
    media: ["video", "img", "[class*=thumbnail]"], attrs: ["alt", "title", "aria-label"],
    hotUrl: classifyUrl,
  });
  def({
    id: "tumblr", threadUrl: /\/post\//, threadFirst: true, hosts: /(^|\.)tumblr\.com$/i, highRisk: true,
    item: ["article", "[data-id]", "[role=article]", "div.post", ".post_container", "li.post", "[class*=post_]"],
    thread: ["main", "#posts", "[data-testid=timeline]"], context: ["h1", "[data-testid=blog-title]", "input[type=search]"],
    media: ["figure", "[data-testid=imageBlock]", "[class*=photo]", "video", "img"],
    avatars: ["img[src*='media.tumblr.com'][src*='/avatar_']"], attrs: ["alt", "title", "aria-label"],
    hotUrl: classifyUrl,
  });
  def({
    id: "steam", hosts: /(^|\.)(steamcommunity\.com|steamuserimages-a\.akamaihd\.net)$/i, highRisk: true,
    item: [".apphub_Card", ".apphub_CardContentMain", ".commentthread_comment", ".workshopItem", ".review_box", ".profile_comment", ".newsPostBlock", ".discussion_topic", "[class*=Panel]"],
    thread: [".apphub_background", ".commentthread_area", "#responsive_page_template_content", "main"], context: ["h1", ".apphub_AppName", ".workshopItemTitle", "input[name=text]"],
    media: [".apphub_CardContentMain img", ".workshopItemPreviewImage", ".screenshot_img", ".earlyaccess_wishlist_img", "video", "img"],
    avatars: [".playerAvatar img", "img[src*='avatars.'][src*='steamstatic.com']", "img[src*='avatars.akamai.steamstatic']"], attrs: ["alt", "title", "aria-label"],
    hotUrl: classifyUrl,
  });
  // The official site posts story trailers and recaps after launch.
  def({
    id: "rockstar", hosts: /(^|\.)rockstargames\.com$/i, highRisk: true,
    item: ["article", "[class*=card]", "[class*=Card]", "section[class*=post]", "[class*=newswire]", "[class*=Newswire]", "[class*=post]"],
    thread: ["main", "#main", "[class*=newswire]"], context: ["h1", "title"],
    media: ["[class*=hero]", "[class*=Hero]", "[class*=poster]", "video", "img"], attrs: ["alt", "title", "aria-label"],
    hotUrl: classifyUrl,
  });
  // Games press and guides: spoilers appear in headlines, recaps, comment threads and guide pages.
  def({
    id: "gamespress", hosts: /(^|\.)(ign|gamespot|kotaku|polygon|eurogamer|gamesradar|pcgamer|vg247|pushsquare|purexbox|gamefaqs|giantbomb|screenrant|dexerto|thegamer|gamerant|destructoid)\.(com|net)$/i, highRisk: true,
    item: ["article", ".content-item", ".article-card", ".card", ".listingResult", ".river-item", "li.item", ".main-listing-item", ".post", ".message", ".comment", "[class*=card]"],
    thread: ["article", "main", "#main"], context: ["h1", "title", "input[type=search]"],
    media: ["figure", "picture", ".article-image", "[class*=thumbnail]", "video", "img"], attrs: ["alt", "title", "aria-label"],
    hotUrl: classifyUrl,
  });
  // Fandom wikis: gta.fandom.com is the single biggest source of plot detail. Hot by URL for any page that names
  // GTA VI; every other page of that wiki is at least warm.
  def({
    id: "fandom", hosts: /(^|\.)fandom\.com$/i, highRisk: true,
    item: [".mw-parser-output p", ".mw-parser-output li", ".portable-infobox", ".page-content section", "article", ".WikiaArticle", ".category-page__member", ".mw-search-result", ".unified-search__result"],
    thread: [".page-content", "#content", "main"], context: ["h1", ".page-header__title", "input[name=query]"],
    media: [".portable-infobox img", ".thumb", ".image", "figure", "video", "img"], attrs: ["alt", "title", "aria-label"],
    hotUrl: function (url) {
      const u = parse(url);
      let best = classifyUrl(url);
      if (u && /(^|\.)gta\.fandom\.com$/i.test(u.hostname)) {
        if (best < 1) best = 1;
        let tail = u.pathname + u.hash;
        try { tail = decodeURIComponent(tail); } catch (e) { /* keep the raw text */ }
        if (/(^|[/_ -])(vi|gta ?6|gta ?vi|grand[_ ]theft[_ ]auto[_ ](vi|6))([/_ #?-]|$)/i.test(tail)) best = 2;
      }
      return best;
    },
  });
  // Mastodon: any host, detected from markup (see detect()).
  const MASTODON = {
    id: "mastodon", hosts: null, highRisk: true,
    item: [".status", "article.status", ".detailed-status", ".status__content", "article[data-id]", ".account-card", ".search-results__section .status", ".trends__item", ".account", "[role=feed] article", ".notification"],
    thread: [".detailed-status", ".status-public", ".scrollable.detailed-status__wrapper"],
    context: [".detailed-status .status__content", ".hashtag-header", ".column-header__title", ".search__input"],
    media: [".media-gallery", ".status-card__image", ".video-player", ".audio-player", ".media-gallery__item", "img"],
    avatars: [".account__avatar", ".account__avatar *", ".status__avatar *", ".account__header__avatar", ".account__header__avatar *"],
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
      threadContext: join(valid(def.threadContext || def.context, doc)),
      threadFirst: !!def.threadFirst,
      media: join(valid(def.media.concat(GENERIC_MEDIA), doc)),
      attrs: def.attrs || [],
      avatars: join(valid(def.avatars || [], doc)),
      cards: join(valid(def.cards || [], doc)),
      mediaIgnore: join(valid((def.avatars || []).concat(def.mediaIgnore || []), doc)),
      ocr: def.ocr !== false,
      exempt: def.exempt ? join(valid(def.exempt.sel, doc)) : "",
      exemptWhen: def.exempt ? join(valid([def.exempt.live], doc)) : "",
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
