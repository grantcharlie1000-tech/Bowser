// Regressions found on real social posts (Bluesky searches, November 2026): ordinary photo posts that named a Jason or
// a Raul were covered, and setting / name + story-cue posts in the launch window were missed. Texts are paraphrased.
import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const SFW = createRequire(import.meta.url)("../../dist/node/engine.cjs");
const { classify: C, policy: P, adapters: A } = SFW;
const LAUNCH = { hot: 0, source: "text", launchWindow: true };
const post = (text, mode = "strict", media = false) =>
  P.decide(C.text(text, LAUNCH), { mode, hot: 0, kind: "item", hasMedia: media, highRisk: true, ocrDone: true });

test("photo posts that only name a common first name, or say 'spoiler' about something else, are not covered", () => {
  for (const t of [
    "Mural by Jason Mann on Short Vine Street, created 2025",
    "SUBSTITUTION 63' Austria: Raul Florucz for Christoph Baumgartner",
    "Calling all cats! Cal Anderson park sticker sign",
    "Brian da Palmas",
    "the blue-footed boobie, a birding highlight",
    "[CW: SPOILER ALERT] you can't imagine the pleasure he'll feel when he arrives",
    "Spoilers below btw. Absolute cinema, this Scrooge has a forehead tattoo",
  ]) {
    assert.equal(post(t, "strict", true), "clear", t);
    assert.equal(post(t, "balanced", true), "clear", t);
  }
  // Lockdown stays maximally protective for media next to any flag
  assert.equal(post("Mural by Jason Mann on Short Vine Street, created 2025", "lockdown", true), "shield");
  // real GTA evidence still covers the picture
  assert.equal(post("GTA 6 screenshot", "strict", true), "shield");
  assert.equal(post("Lucia Caminos fan art", "strict", true), "shield");
});

test("launch window: distinctive name straight before a death / betrayal verb is GTA VI", () => {
  for (const t of ["Lucia dies", "Lucia dies 😭😭", "lucia gets killed lmao", "Lucia's death", "Duval dies"]) assert.equal(post(t), "shield", t);
  // a surname in between is a real person in the news; outside the launch window the shape alone is not enough
  assert.equal(post("Police say Lucia Santos betrayed the trust of investors", "lockdown"), "clear");
  assert.equal(P.decide(C.text("Lucia dies", { hot: 0 }), { mode: "strict", hot: 0, kind: "item" }), "clear");
});

test("launch window: Vice City with a story cue or a game word is GTA VI; the bare place name is not", () => {
  for (const t of ["the Vice City ending got me crying", "bro the last mission in vice city 💀", "the twist in vice city tho"]) assert.equal(post(t), "shield", t);
  for (const t of ["Miami unveiled the special Vice City court for one night only", "gonna find the best cuban sandwich in vice city"]) assert.equal(post(t), "clear", t);
});

test("Rockstar + a character + a strong cue in the launch window", () => {
  assert.equal(post("R* really killed off Jason"), "shield");
});

test("other works and real people keep their spoilers", () => {
  for (const t of ["brian died in the end of family guy", "Jason Kelce dies laughing", "i'm dying 😂", "this is the end of an era", "Raul Jimenez scores in the final minute"]) assert.equal(post(t), "clear", t);
  assert.equal(post("The last Mission Impossible was last year, but Dead Reckoning and Final Reckoning performed below Fallout"), "clear");
});

test("adapters: page context never matches every post of a feed; thread heads only on real thread pages", () => {
  const x = A.forLocation({ hostname: "x.com" });
  assert.ok(!/tweetText/.test(x.context), "a tweet is not page context");
  assert.equal(x.threadFirst, true);
  const fb = A.forLocation({ hostname: "www.facebook.com" });
  assert.ok(!/data-ad-preview/.test(fb.context), "a feed post's message is not page context");
  assert.ok(/data-ad-preview/.test(fb.threadContext), "it is the context of its own post");
  assert.ok(!/role=main/.test(fb.thread));
  const yt = A.forLocation({ hostname: "www.youtube.com" });
  assert.ok(!/(^|,)ytd-channel-name/.test(yt.context), "every video card has a channel name");
  const tt = A.forLocation({ hostname: "www.tiktok.com" });
  assert.ok(!/\[data-e2e=video-desc\]/.test(tt.context));
  for (const host of ["bsky.app", "www.threads.net", "www.google.com", "www.twitch.tv", "discord.com"]) {
    const a = A.forLocation({ hostname: host });
    if (a.threadFirst) assert.ok(A.definition(a.id).threadUrl, host + ": a feed root's first post must not define the thread");
  }
});
