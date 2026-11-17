// Synthetic perf fixtures (fictional content), served by a tiny dependency-free Node server.
// Hostnames are mapped to loopback by the browser (--host-resolver-rules), so adapters see real hosts:
//   x.com -> feed (X adapter, highRisk)     youtube.com -> grid / watch (YouTube adapter, highRisk)
//   google.com -> search (Google adapter, highRisk)     news.test -> article (generic adapter, not highRisk)
//   img.cdn.test -> images / video (cross-origin, as on real sites)
import http from "node:http";
import { readFileSync, existsSync } from "node:fs";
import { join, dirname, extname } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const IMG = join(HERE, "img");
const OCR_IMG = join(HERE, "..", "..", "fixtures", "ocr-images");
const MANIFEST = JSON.parse(readFileSync(join(IMG, "manifest.json"), "utf8"));
export const HOSTS = ["x.com", "youtube.com", "google.com", "news.test", "img.cdn.test"];

function prng(seed) { let s = seed >>> 0; return () => ((s = (s * 1664525 + 1013904223) >>> 0) / 4294967296); }
const WORDS = "coffee train weekend garden music football recipe rain commute puppy sunset keyboard bicycle movie reading budget sketch hiking podcast bakery laptop album marathon painting library market garage festival harbor summer winter pasta thunder pocket window violin planet mirror orchard canyon lantern whisper".split(" ");
const SPOILERS = [
  "GTA 6 ending spoiler: the hero dies in the final mission",
  "GTA VI leak: Lucia betrays Jason in the last chapter, ending explained",
  "Grand Theft Auto 6 spoiler thread - the final boss and the ending revealed",
  "GTA 6 story leak: major character death in mission 40 (ending)",
];
function sentence(r, n) { const w = []; for (let i = 0; i < n; i++) w.push(WORDS[(r() * WORDS.length) | 0]); w[0] = w[0][0].toUpperCase() + w[0].slice(1); return w.join(" "); }
const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;");

// ---------------------------------------------------------------------------------------------- feed (X-like)
export const FEED_TOTAL = 300, FEED_INITIAL = 40, FEED_STEP = 20;
export function postHtml(i, imgOrigin) {
  const r = prng(1000 + i * 7919);
  const spoiler = i % 50 === 25; // 2% of posts carry fictional spoiler text
  const media = r() < 0.6;
  const nImg = media ? 1 + ((r() * 4) | 0) : 0;
  const text = spoiler ? SPOILERS[Math.floor(i / 50) % SPOILERS.length] : sentence(r, 14 + ((r() * 22) | 0)) + ".";
  const av = (r() * MANIFEST.avatars) | 0;
  let imgs = "";
  if (nImg) {
    const cell = nImg === 1 ? "100%" : "calc(50% - 2px)";
    for (let j = 0; j < nImg; j++) {
      // a handful of images are OCR fixtures with captions (2 spoilers, 1 clean) - these matter for the OCR variant only
      const ocrName = (i % 40 === 3 && j === 0) ? "ocr-meme-spoiler.png" : (i % 40 === 11 && j === 0) ? "ocr-tweet-spoiler.png" : (i % 20 === 7 && j === 0) ? "ocr-meme-cooking.png" : null;
      const pi = (i * 5 + j * 11) % MANIFEST.photos.length;
      const src = ocrName ? `${imgOrigin}/ocr/${ocrName}?p=${i}` : `${imgOrigin}/img/photo-${pi}.${MANIFEST.photos[pi]}?p=${i}_${j}`;
      imgs += `<div class="ph" style="width:${cell}"><img loading="lazy" decoding="async" width="${nImg === 1 ? 504 : 250}" height="${nImg === 1 ? 336 : 167}" alt="" src="${src}"></div>`;
    }
    imgs = `<div data-testid="tweetPhoto" class="photos">${imgs}</div>`;
  }
  return `<div data-testid="cellInnerDiv"><article data-testid="tweet" role="article" data-i="${i}"${spoiler ? ' data-fixture-spoiler="1"' : ""}><div class="av"><img width="48" height="48" alt="" src="${imgOrigin}/img/avatar-${av}.jpg"></div><div class="bd"><div data-testid="User-Name"><a href="/user${i % 97}">User ${i % 97}</a> <span>@user${i % 97}</span> &middot; <time>${(i % 23) + 1}h</time></div><div data-testid="tweetText" lang="en"><span>${esc(text)}</span></div>${imgs}<div role="group" class="acts"><button>Reply ${i % 13}</button><button>Repost ${i % 7}</button><button>Like ${(i * 3) % 41}</button><button>Share</button></div></div></article></div>`;
}
const FEED_CSS = `body{margin:0;background:#fff;font:15px/1.35 system-ui,sans-serif;color:#0f1419}#primary{width:600px;margin:0 auto;border-inline:1px solid #eee}article{display:flex;gap:12px;padding:12px 16px;border-bottom:1px solid #eee}.av img{border-radius:50%}.bd{flex:1;min-width:0}.photos{display:flex;flex-wrap:wrap;gap:4px;margin:8px 0;border-radius:14px;overflow:hidden}.ph img{display:block;width:100%;height:auto;background:#ddd}.acts{display:flex;gap:18px;margin-top:6px}.acts button{background:none;border:0;color:#536471;font:inherit;padding:0}`;
function feedPage(imgOrigin) {
  let posts = ""; for (let i = 0; i < FEED_INITIAL; i++) posts += postHtml(i, imgOrigin);
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Home / X</title><style>${FEED_CSS}</style></head><body><div data-testid="primaryColumn" id="primary"><div aria-label="Timeline: Your Home Timeline" id="tl">${posts}</div></div><div id="sentinel" style="height:1px"></div>
<script>
var next=${FEED_INITIAL},busy=false,total=${FEED_TOTAL},step=${FEED_STEP},lastAt=0;
function more(){if(busy||next>=total)return;busy=true;var from=next;fetch('/api/posts?from='+from+'&n='+step).then(function(r){return r.text()}).then(function(h){document.getElementById('tl').insertAdjacentHTML('beforeend',h);next+=step;busy=false;window.__appends=(window.__appends||0)+1;});}
// infinite scroll: one append of ${FEED_STEP} posts every ~1200px scrolled (and when near the bottom)
addEventListener('scroll',function(){var y=scrollY;if(y-lastAt>1200||(innerHeight+y>document.documentElement.scrollHeight-2500)){lastAt=y;more();}},{passive:true});
</script></body></html>`;
}

// ---------------------------------------------------------------------------------------------- video (YouTube-like)
function videoGrid(imgOrigin) {
  let items = "";
  for (let i = 0; i < 120; i++) {
    const r = prng(5000 + i * 31); const pi = (i * 3) % MANIFEST.photos.length;
    const title = i % 50 === 20 ? "GTA 6 ending leaked - full spoiler breakdown" : sentence(r, 4 + ((r() * 6) | 0));
    items += `<ytd-rich-item-renderer><ytd-thumbnail><a href="/watch?v=v${i}"><yt-image><img loading="lazy" width="320" height="180" alt="${esc(title)}" src="${imgOrigin}/img/photo-${pi}.${MANIFEST.photos[pi]}?v=${i}"></yt-image></a></ytd-thumbnail><div class="meta"><h3 id="video-title"><a href="/watch?v=v${i}" title="${esc(title)}">${esc(title)}</a></h3><ytd-channel-name>Channel ${i % 17}</ytd-channel-name><div>${(i * 37) % 900}K views &middot; ${(i % 11) + 1} days ago</div></div></ytd-rich-item-renderer>`;
  }
  return `<!doctype html><html><head><meta charset="utf-8"><title>YouTube</title><style>body{margin:0;font:14px system-ui,sans-serif}ytd-rich-grid-renderer{display:grid;grid-template-columns:repeat(4,1fr);gap:16px;padding:16px}ytd-rich-item-renderer,ytd-thumbnail,yt-image,ytd-channel-name,ytd-browse,ytd-rich-grid-renderer{display:block}ytd-rich-grid-renderer{display:grid}ytd-thumbnail img{width:100%;height:auto;border-radius:12px;background:#ddd}h3{margin:8px 0 2px;font-size:15px}input#search{width:300px}</style></head><body><div><input id="search" placeholder="Search"></div><ytd-browse><ytd-rich-grid-renderer>${items}</ytd-rich-grid-renderer></ytd-browse></body></html>`;
}
function videoWatch(imgOrigin) {
  let comments = "";
  for (let i = 0; i < 100; i++) {
    const r = prng(9000 + i * 13);
    const t = i % 50 === 33 ? "GTA 6 spoiler: the ending is wild, the hero dies" : sentence(r, 10 + ((r() * 25) | 0)) + ".";
    comments += `<ytd-comment-thread-renderer><ytd-comment-view-model><div class="c"><img width="40" height="40" alt="" src="${imgOrigin}/img/avatar-${i % 8}.jpg"><div><b>@viewer${i}</b><div id="content-text">${esc(t)}</div></div></div></ytd-comment-view-model></ytd-comment-thread-renderer>`;
  }
  let rec = "";
  for (let i = 0; i < 20; i++) { const pi = (i * 7) % MANIFEST.photos.length; rec += `<ytd-compact-video-renderer><img width="168" height="94" alt="" src="${imgOrigin}/img/photo-${pi}.${MANIFEST.photos[pi]}?r=${i}"><span>${esc(sentence(prng(i + 3), 6))}</span></ytd-compact-video-renderer>`; }
  return `<!doctype html><html><head><meta charset="utf-8"><title>Sample clip - YouTube</title><style>body{margin:0;font:14px system-ui,sans-serif}ytd-watch-flexy{display:flex;gap:24px;padding:24px}ytd-comment-thread-renderer,ytd-comment-view-model,ytd-watch-metadata{display:block}#primary{width:854px}video{width:854px;height:480px;background:#000;display:block}.c{display:flex;gap:12px;margin:12px 0}ytd-compact-video-renderer{display:flex;gap:8px;margin-bottom:8px}#secondary{width:402px}</style></head><body><ytd-watch-flexy><div id="primary"><div id="movie_player"><video id="v1" muted loop autoplay playsinline src="${imgOrigin}/media/clip.webm"></video></div><ytd-watch-metadata><h1 class="ytd-watch-metadata">A calm walk through a fictional harbor town</h1><div id="description">Synthetic description text for the performance fixture.</div></ytd-watch-metadata><div id="comments">${comments}</div></div><div id="secondary"><video id="v2" muted loop autoplay playsinline width="402" height="226" src="${imgOrigin}/media/clip.webm"></video>${rec}</div></ytd-watch-flexy></body></html>`;
}

// ---------------------------------------------------------------------------------------------- article (20k words, clean)
function article() {
  const r = prng(77); let words = 0, html = "", pi = 0;
  while (words < 20000) {
    if (pi % 12 === 0) html += `<h2>${esc(sentence(r, 5))}</h2>`;
    const n = 40 + ((r() * 70) | 0); let p = ""; for (let k = 0; k < n / 12; k++) p += sentence(r, 10 + ((r() * 6) | 0)) + ". ";
    html += `<p>${p}<a href="/more/${pi}">related</a></p>`; words += n; pi++;
  }
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Long read: the harbor town</title><style>body{margin:0;font:18px/1.6 Georgia,serif;color:#222}article{max-width:720px;margin:0 auto;padding:24px}</style></head><body><article><h1>A very long, fully clean article</h1>${html}</article></body></html>`;
}

// ---------------------------------------------------------------------------------------------- search + autocomplete churn
function search() {
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>harbor town - Search</title><style>body{margin:0;font:14px arial,sans-serif;padding:16px}#sb{position:relative;width:560px}textarea{width:100%;height:32px;font:16px arial}ul#sug{list-style:none;margin:0;padding:0;position:absolute;top:36px;left:0;right:0;background:#fff;border:1px solid #ddd;z-index:3}li.sbct{padding:6px 10px}.g{margin:18px 0;max-width:600px}.g h3{margin:0;font-size:20px;color:#1a0dab}.g .u{color:#006621}</style></head><body>
<div id="sb"><textarea name="q" id="APjFqb">harbor town</textarea><ul id="sug" role="listbox"></ul></div><div id="rso"></div>
<script>
var W="coffee train weekend garden music football recipe rain commute puppy sunset keyboard bicycle movie reading budget sketch hiking podcast bakery laptop album marathon".split(" ");
var seed=42;function rnd(){seed=(seed*1664525+1013904223)>>>0;return seed/4294967296}
function s(n){var a=[];for(var i=0;i<n;i++)a.push(W[(rnd()*W.length)|0]);return a.join(" ")}
function results(round){var h="";for(var i=0;i<10;i++){var t=(round%25===12&&i===3)?"GTA 6 ending spoiler leak explained":s(5);h+='<div class="MjjYud"><div class="g"><div class="u">example'+i+'.test</div><a href="/r/'+round+'_'+i+'"><h3>'+t+'</h3></a><div class="VwiC3b">'+s(24)+'.</div></div></div>'}document.getElementById("rso").innerHTML=h}
function suggest(prefix){var h="";for(var i=0;i<8;i++)h+='<li class="sbct" role="option"><span>'+prefix+' '+s(2)+'</span></li>';document.getElementById("sug").innerHTML=h}
results(0);suggest("harbor");
// churn: a "keystroke" every 90ms rebuilds the autocomplete list; every 4th rebuild also replaces the whole results block
window.__churn=function(ms){return new Promise(function(res){var t0=performance.now(),n=0,q="";var iv=setInterval(function(){n++;q+=W[n%W.length][0];var ta=document.getElementById("APjFqb");ta.value=q;suggest(q);if(n%4===0)results(n);if(performance.now()-t0>ms){clearInterval(iv);res(n)}},90)})};
</script></body></html>`;
}

const TYPES = { ".png": "image/png", ".jpg": "image/jpeg", ".webm": "video/webm", ".json": "application/json" };
export function startServer(port = 0) {
  const server = http.createServer((req, res) => {
    const u = new URL(req.url, "http://x");
    const host = (req.headers.host || "").split(":")[0];
    const p = server.address().port;
    const o = (h) => `http://${h}:${p}`;
    const html = (b) => res.writeHead(200, { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" }).end(b);
    const file = (f) => { if (!existsSync(f)) return res.writeHead(404).end("nf"); res.writeHead(200, { "content-type": TYPES[extname(f)] || "application/octet-stream", "cache-control": "no-store" }).end(readFileSync(f)); };
    if (u.pathname.startsWith("/img/") || u.pathname.startsWith("/media/")) return file(join(IMG, u.pathname.replace(/^\/(img|media)\//, "")));
    if (u.pathname.startsWith("/ocr/")) return file(join(OCR_IMG, u.pathname.slice(5)));
    if (u.pathname === "/api/posts") { const from = +u.searchParams.get("from"), n = +u.searchParams.get("n"); let h = ""; for (let i = from; i < Math.min(from + n, FEED_TOTAL); i++) h += postHtml(i, o("img.cdn.test")); return html(h); }
    if (u.pathname === "/favicon.ico") return res.writeHead(204).end();
    if (host === "x.com") return html(feedPage(o("img.cdn.test")));
    if (host === "youtube.com") return html(u.pathname === "/watch" ? videoWatch(o("img.cdn.test")) : videoGrid(o("img.cdn.test")));
    if (host === "google.com") return html(search());
    if (host === "news.test") return html(article());
    res.writeHead(404).end("nf");
  });
  return new Promise((r) => server.listen(port, "127.0.0.1", () => r(server)));
}
