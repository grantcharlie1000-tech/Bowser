// The one place the term-update feed's constants live for the Chrome extension. Classic script: loaded by the
// service worker (importScripts) and by the popup and options pages.
//
//   FEED_URL     where the signed feed is fetched from. It answers 404 until the maintainer publishes a feed;
//                that is handled quietly ("no update published yet").
//   PUBLIC_KEYS  Ed25519 verification keys, base64 of the raw 32 bytes. A feed is accepted when ANY verifies it
//                (that is how a key is rotated). Must equal feed/public-key.txt (a unit test checks it).
//   BUILTIN_VERSION  the bundled lexicon's version, shown next to the update number. Must equal
//                SFW.lexicon.version of the engine in the same build (a unit test checks it).
//
// The development test key is NOT here. It is trusted only through the unpacked-build override in
// feed-update.js, which a web page cannot set.
var SFWFeedConfig = {
  FEED_URL: "https://raw.githubusercontent.com/grantcharlie1000-tech/Bowser/main/extensions/spoiler-firewall/feed/published/feed.json",
  PUBLIC_KEYS: ["bbeYDKCBPf+bkGjnhDnJPXCkBh8oB7k4ejct5oonwNI="],
  BUILTIN_VERSION: "2026.11.18-1",
  CHECK_EVERY_MINUTES: 720,
  FETCH_TIMEOUT_MS: 10000,
};
