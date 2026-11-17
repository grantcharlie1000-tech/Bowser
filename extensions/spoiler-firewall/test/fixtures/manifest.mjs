// Fixture catalogue. `host` is mapped to loopback by the browser (--host-resolver-rules), so adapters see real
// site hostnames. `chunk` = stream the page with delays (spoilers get cut across network chunks).
// `expect` markup: data-expect="shield" / "clear" on units; sentinels are .sentinel (magenta #ff00ff).
export const FIXTURES = [
  { id: "x-timeline", host: "x.com", url: "/home", chunk: { delay: 120, size: 90 }, settle: 600 },
  { id: "reddit-new", host: "www.reddit.com", url: "/r/pics/comments/abc/some_post/", chunk: { delay: 120, size: 70 }, settle: 700 },
  { id: "reddit-old", host: "old.reddit.com", url: "/r/pics/", chunk: { delay: 100, size: 60 }, settle: 400 },
  { id: "youtube-watch", host: "www.youtube.com", url: "/watch?v=abc123", chunk: { delay: 100, size: 80 }, settle: 600 },
  { id: "youtube-search-cold", host: "www.youtube.com", url: "/results?search_query=weather+forecast", chunk: { delay: 100, size: 80 }, settle: 400 },
  { id: "youtube-search-hot", host: "www.youtube.com", url: "/results?search_query=gta+6+ending", settle: 400, page: "youtube-search.html", hotUrl: true },
  { id: "google-cold", host: "www.google.com", url: "/search?q=weather+tomorrow", chunk: { delay: 100, size: 80 }, settle: 400 },
  { id: "google-hot", host: "www.google.com", url: "/search?q=gta+6+ending+spoiler", settle: 400, hotUrl: true },
  { id: "news", host: "news.example.test", url: "/pages/news.html", chunk: { delay: 100, size: 50 }, settle: 300 },
  { id: "blog", host: "blog.example.test", url: "/pages/blog.html", chunk: { delay: 100, size: 50 }, settle: 300 },
  { id: "forum", host: "forum.example.test", url: "/pages/forum.html", chunk: { delay: 100, size: 50 }, settle: 300 },
  { id: "adv-split", host: "example.test", url: "/pages/adv-split.html", chunk: { delay: 100, size: 30 }, settle: 600 },
  { id: "adv-evasive", host: "example.test", url: "/pages/adv-evasive.html", settle: 300, detect: "evasive" },
  { id: "adv-churn", host: "example.test", url: "/pages/adv-churn.html", settle: 2500, churn: true },
  { id: "adv-scroll", host: "example.test", url: "/pages/adv-scroll.html", settle: 400, scenario: "scroll" },
  { id: "adv-virtual", host: "example.test", url: "/pages/adv-virtual.html", settle: 400, scenario: "virtual" },
  { id: "adv-spa", host: "example.test", url: "/pages/adv-spa.html", settle: 300, scenario: "spa", pageShield: true },
  { id: "adv-shadow", host: "example.test", url: "/pages/adv-shadow.html", chunk: { delay: 100, size: 40 }, settle: 500 },
  { id: "adv-iframes", host: "example.test", url: "/pages/adv-iframes.html", settle: 600 },
  { id: "adv-hover", host: "example.test", url: "/pages/adv-hover.html", settle: 300 },
  { id: "adv-video", host: "example.test", url: "/pages/adv-video.html", settle: 600 },
  { id: "adv-title", host: "example.test", url: "/pages/adv-title.html", settle: 700, pageShield: true },
  { id: "adv-transition", host: "example.test", url: "/pages/adv-transition.html", settle: 500 },
  { id: "adv-closed-dsd", host: "example.test", url: "/pages/adv-closed-dsd.html", settle: 300, knownGap: "closed declarative shadow roots are unreachable from the page world (no hook fires, shadowRoot is null)" },
  // page-runtime regressions (C-1): frames the engine cannot run in (sandbox without allow-scripts)
  { id: "rt-sandbox-srcdoc", host: "example.test", url: "/pages/rt-sandbox-srcdoc.html", settle: 500 },
  { id: "rt-sandbox-src", host: "example.test", url: "/pages/rt-sandbox-src.html", settle: 600 },
  { id: "rt-sandbox-small", host: "example.test", url: "/pages/rt-sandbox-small.html", settle: 600 },
  { id: "rt-sandbox-hot", host: "example.test", url: "/pages/rt-sandbox-hot.html", settle: 600, hotUrl: true },
  { id: "rt-frame-scripts", host: "example.test", url: "/pages/rt-frame-scripts.html", settle: 800 },
  { id: "adv-card", host: "example.test", url: "/pages/adv-card.html", chunk: { delay: 100, size: 60 }, settle: 600 },
  { id: "adv-react", host: "example.test", url: "/pages/adv-react.html", settle: 1500 },
];

// Build the URL for a fixture. `origin` is "http://<host>:<port>".
export function fixtureUrl(f, port, { chunked = false } = {}) {
  const base = `http://${f.host}:${port}`;
  if (!chunked || !f.chunk) return base + f.url;
  const sep = f.url.includes("?") ? "&" : "?";
  return `${base}${f.url}${sep}__chunk=${f.chunk.delay}&__size=${f.chunk.size}`;
}
