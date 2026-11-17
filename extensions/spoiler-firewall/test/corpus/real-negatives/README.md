# Real negative corpus

Plain-text extracts of Wikipedia articles (films, novels, other games, Grand Theft Auto V and
earlier titles, Rockstar Games/Energy, Saint Lucia, Greater Toronto Area, people named Jason/Lucia,
football and news topics, plus es/fr/de/ja/ru/zh pages) used to measure false positives of the
spoiler classifier. They are test fixtures only and are not shipped.

- Source: https://en.wikipedia.org (and es/fr/de/ja/ru/zh subdomains), MediaWiki API
  `action=query&prop=extracts&explaintext=1`.
- License: text is available under CC BY-SA 4.0 (https://creativecommons.org/licenses/by-sa/4.0/).
  Each `data/<lang>.json` records the article titles; author attribution is available from the
  page history of the named article on Wikipedia. Paragraphs may be truncated or chunked.
- Regenerate with `node test/corpus/real-negatives/fetch-real-negatives.mjs` (network required;
  generic User-Agent `SpoilerFirewallTest/1.0 (test corpus)`).
- Labels are not hand-picked per paragraph: `run-corpus.mjs` labels a paragraph "mentions GTA VI"
  with an independent simple regex (franchise names and GTA VI entity names). Those paragraphs are
  expected to shield in Lockdown/Strict and are excluded from the Balanced rate; every other
  paragraph is expected to be clear in Strict and Balanced.
