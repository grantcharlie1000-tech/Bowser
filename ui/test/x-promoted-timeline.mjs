// X promoted posts, timeline data path (src/blocker/x_promoted.js): the script drops the entries X itself flags
// as promoted from its own GraphQL timeline responses, read through XMLHttpRequest or fetch, before the page sees
// them. Runs the real script in a node:vm context with a minimal XMLHttpRequest / Response / document, against
// timeline JSON shaped like X's (HomeTimeline, TweetDetail, SearchTimeline, a module, an AddToModule instruction).
// Needs no browser; the headless Edge tests in x-promoted.mjs cover the label fallback and a real browser.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const here = dirname(fileURLToPath(import.meta.url));
const SCRIPT = readFileSync(join(here, '..', '..', 'src', 'blocker', 'x_promoted.js'), 'utf8');
const API = 'https://x.com/i/api/graphql/c-CzHF1LboFilMpsx4ZCrQ/';

// ---- X-shaped timeline data ----
const tweet = (id, text = 'hello') => ({ __typename: 'Tweet', rest_id: id, legacy: { full_text: text, id_str: id } });
const promotedMetadata = {
  advertiser_results: { result: { __typename: 'User', rest_id: '99' } },
  disclosureType: 'NoDisclosure', experimentValues: [], impressionId: '1a2b', impressionString: '1a2b',
  clickTrackingInfo: { urlParams: [] },
};
const item = (id, extra = {}) => ({
  entryType: 'TimelineTimelineItem', __typename: 'TimelineTimelineItem',
  itemContent: { itemType: 'TimelineTweet', __typename: 'TimelineTweet', tweet_results: { result: tweet(id, extra.text) }, tweetDisplayType: 'Tweet', ...(extra.promoted ? { promotedMetadata } : {}) },
});
const entry = (entryId, content) => ({ entryId, sortIndex: '1790000000000000000', content });
const cursor = (kind) => entry(`cursor-${kind}-1790`, { entryType: 'TimelineTimelineCursor', __typename: 'TimelineTimelineCursor', value: 'DAABCgAB', cursorType: kind === 'top' ? 'Top' : 'Bottom' });
const moduleItem = (entryId, content) => ({ entryId, item: { itemContent: content, clientEventInfo: {} } });
const userContent = (id, promoted) => ({ itemType: 'TimelineUser', __typename: 'TimelineUser', user_results: { result: { __typename: 'User', rest_id: id } }, userDisplayType: 'User', ...(promoted ? { promotedMetadata } : {}) });
const module = (entryId, items) => entry(entryId, { entryType: 'TimelineTimelineModule', __typename: 'TimelineTimelineModule', items, displayType: 'Vertical' });

const home = () => ({ data: { home: { home_timeline_urt: { instructions: [
  { type: 'TimelineAddEntries', entries: [
    entry('tweet-1', item('1')),
    entry('promoted-tweet-1790000000000000002-6a1c5f0e2b3d4c5a', item('2', { promoted: true })),
    // A normal post whose own text mentions "Promoted": kept.
    entry('tweet-3', item('3', { text: 'Promoted by nobody, just a word' })),
    // Flagged by its metadata only (no telling entryId): dropped.
    entry('tweet-4', item('4', { promoted: true })),
    entry('promotedTweet-5', item('5')),
    // Who to follow with one promoted account: the module stays, minus that account.
    module('who-to-follow-6', [moduleItem('who-to-follow-6-user-7', userContent('7', false)), moduleItem('who-to-follow-6-user-8', userContent('8', true))]),
    // A module of nothing but promoted items goes away.
    module('who-to-follow-9', [moduleItem('who-to-follow-9-user-10', userContent('10', true))]),
    cursor('top'), cursor('bottom'),
  ] },
  { type: 'TimelineAddToModule', moduleEntryId: 'who-to-follow-6', moduleItems: [moduleItem('who-to-follow-6-user-11', userContent('11', true)), moduleItem('who-to-follow-6-user-12', userContent('12', false))] },
  { type: 'TimelineReplaceEntry', entry_id_to_replace: 'tweet-1', entry: entry('promoted-tweet-13-ff', item('13', { promoted: true })) },
  { type: 'TimelineTerminateTimeline', direction: 'Top' },
] } } } });

const detail = () => ({ data: { threaded_conversation_with_injections_v2: { instructions: [{ type: 'TimelineAddEntries', entries: [
  entry('tweet-20', item('20')),
  module('conversationthread-21', [moduleItem('conversationthread-21-tweet-21', item('21').itemContent), moduleItem('conversationthread-21-tweet-22', item('22', { promoted: true }).itemContent)]),
  entry('promoted-tweet-23-aa', item('23', { promoted: true })),
] }] } } });

const search = () => ({ data: { search_by_raw_query: { search_timeline: { timeline: { instructions: [{ type: 'TimelineAddEntries', entries: [
  entry('tweet-30', item('30')),
  entry('promoted-tweet-31-bb', item('31', { promoted: true })),
] }] } } } } });

const ids = (data) => {
  const out = [];
  const walk = (node) => {
    if (Array.isArray(node)) node.forEach(walk);
    else if (node && typeof node === 'object') {
      if (typeof node.entryId === 'string') out.push(node.entryId);
      Object.values(node).forEach(walk);
    }
  };
  walk(data);
  return out;
};
const HOME_KEPT = ['tweet-1', 'tweet-3', 'who-to-follow-6', 'who-to-follow-6-user-7', 'cursor-top-1790', 'cursor-bottom-1790', 'who-to-follow-6-user-12'];

// ---- a minimal page: the readers X uses, with getters on the prototype as in a browser ----
function page({ style = true } = {}) {
  const document = {
    body: null,
    style,
    getElementById(id) { return id === 'bowser-cosmetic' && this.style ? {} : null; },
    addEventListener() {},
    querySelectorAll() { return []; },
  };
  class XMLHttpRequest {
    constructor(url, body, responseType = '', readyState = 4) { Object.assign(this, { _url: url, _body: body, _type: responseType, _state: readyState }); }
  }
  const proto = XMLHttpRequest.prototype;
  const getter = (name, get) => Object.defineProperty(proto, name, { configurable: true, enumerable: true, get });
  getter('readyState', function () { return this._state; });
  getter('responseURL', function () { return this._url; });
  getter('responseType', function () { return this._type; });
  getter('responseText', function () {
    if (this._type !== '' && this._type !== 'text') throw new Error('InvalidStateError');
    return this._body;
  });
  getter('response', function () { return this._type === 'json' ? this._json ??= JSON.parse(this._body) : this._body; });
  class Response {
    constructor(url, body) { this.url = url; this._body = body; }
    text() { return Promise.resolve(this._body); }
    json() { return Promise.resolve(JSON.parse(this._body)); }
  }
  const logs = [];
  class MutationObserver { observe() {} disconnect() {} }
  const context = { document, XMLHttpRequest, Response, MutationObserver, console: { debug: (line) => logs.push(line) } };
  context.window = context;
  vm.createContext(context);
  vm.runInContext(SCRIPT, context);
  return { context, document, logs, xhr: (url, data, type, state) => new XMLHttpRequest(url, JSON.stringify(data), type, state), response: (url, data) => new Response(url, JSON.stringify(data)) };
}

test('promoted entries are dropped from a HomeTimeline read through XMLHttpRequest', () => {
  const p = page();
  const request = p.xhr(API + 'HomeTimeline?variables=%7B%7D', home());
  const text = request.responseText;
  assert.deepEqual(ids(JSON.parse(text)), HOME_KEPT);
  // Every read of the same response, through either getter, gives the same pruned text.
  assert.equal(request.response, text);
  assert.equal(request.responseText, text);
  const instructions = JSON.parse(text).data.home.home_timeline_urt.instructions;
  assert.deepEqual(instructions.map((i) => i.type), ['TimelineAddEntries', 'TimelineAddToModule', 'TimelineTerminateTimeline']);
  const stats = p.context.__bowserXPromotedNet.stats;
  assert.equal(stats.removed, 7);
  assert.equal(stats.checked, 1, 'parsed once for both getters and repeated reads');
  assert.equal(p.logs.length, 1);
});

test('responseType json, TweetDetail and SearchTimeline are pruned too', () => {
  const p = page();
  const json = p.xhr(API + 'TweetDetail', detail(), 'json');
  assert.deepEqual(ids(json.response), ['tweet-20', 'conversationthread-21', 'conversationthread-21-tweet-21']);
  assert.throws(() => json.responseText, /InvalidStateError/, 'native behaviour kept');
  assert.deepEqual(ids(JSON.parse(p.xhr(API + 'SearchTimeline', search()).responseText)), ['tweet-30']);
});

test('fetch responses are pruned through Response.text and Response.json', async () => {
  const p = page();
  assert.deepEqual(ids(await p.response(API + 'HomeLatestTimeline', home()).json()), HOME_KEPT);
  assert.deepEqual(ids(JSON.parse(await p.response(API + 'UserTweets', search()).text())), ['tweet-30']);
});

test('other responses, unfinished requests and an off switch are left alone', async () => {
  const p = page();
  const original = JSON.stringify(home());
  // Not X's GraphQL API.
  assert.equal(p.xhr('https://x.com/i/api/1.1/jot/client_event.json', home()).responseText, original);
  assert.equal(await p.response('https://abs.twimg.com/x.json', home()).text(), original);
  // Not finished loading yet.
  assert.equal(p.xhr(API + 'HomeTimeline', home(), '', 3).responseText, original);
  // No promoted entries: the very same text comes back, without a parse.
  const plain = JSON.stringify(search().data.search_by_raw_query.search_timeline.timeline.instructions[0].entries[0]);
  assert.equal(p.xhr(API + 'HomeTimeline', JSON.parse(plain)).responseText, plain);
  // Not JSON at all.
  const bad = p.xhr(API + 'HomeTimeline', 0);
  bad._body = 'promoted <html>';
  assert.equal(bad.responseText, 'promoted <html>');
  // The cosmetic style is the on switch: without it (a site exception, a lower level) nothing changes.
  p.document.style = false;
  assert.equal(p.xhr(API + 'HomeTimeline', home()).responseText, original);
  assert.equal(await p.response(API + 'HomeTimeline', home()).text(), original);
  p.document.style = true;
  assert.deepEqual(ids(JSON.parse(p.xhr(API + 'HomeTimeline', home()).responseText)), HOME_KEPT);
});

test('running the script again does not wrap the readers twice', () => {
  const p = page();
  const get = Object.getOwnPropertyDescriptor(p.context.XMLHttpRequest.prototype, 'responseText').get;
  const text = p.context.Response.prototype.text;
  p.context.__bowserXPromoted = null; // the label part stopped (style removed) and runs again
  vm.runInContext(SCRIPT, p.context);
  assert.equal(Object.getOwnPropertyDescriptor(p.context.XMLHttpRequest.prototype, 'responseText').get, get);
  assert.equal(p.context.Response.prototype.text, text);
});
