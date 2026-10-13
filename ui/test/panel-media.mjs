// Media support check: verdicts, missing APIs, errors, timeouts, DRM and guidance, all with fake probes.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, '..', 'src', 'panels', 'media-logic.ts'), 'utf8');
const { code } = esbuild.transformSync(source, { loader: 'ts', format: 'esm' });
const logic = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);

const byId = (list, id) => list.find((item) => item.id === id);
const never = () => new Promise(() => {});
const info = (supported, smooth, powerEfficient) => async () => ({ supported, smooth, powerEfficient });

/** decodingInfo that answers per content type. */
const decodingBy = (table) => async (config) => table[config.video.contentType] ?? { supported: false, smooth: false, powerEfficient: false };

test('codec table uses the documented content types', () => {
  assert.deepEqual(logic.CODECS.map((c) => [c.id, c.contentType]), [
    ['h264', 'video/mp4; codecs="avc1.640028"'],
    ['vp9', 'video/webm; codecs="vp09.00.10.08"'],
    ['av1', 'video/mp4; codecs="av01.0.05M.08"'],
    ['hevc', 'video/mp4; codecs="hvc1.1.6.L93.B0"'],
  ]);
});

test('verdicts: hardware, software, unsupported for the four codecs', async () => {
  const [h264, vp9, av1, hevc] = logic.CODECS.map((c) => c.contentType);
  const report = await logic.runMediaCheck({
    canPlayType: (type) => (type === hevc ? '' : 'probably'),
    isTypeSupported: (type) => type !== hevc,
    decodingInfo: decodingBy({
      [h264]: { supported: true, smooth: true, powerEfficient: true },
      [vp9]: { supported: true, smooth: true, powerEfficient: false },
      [av1]: { supported: true, smooth: false, powerEfficient: false },
      [hevc]: { supported: false, smooth: false, powerEfficient: false },
    }),
  });
  assert.equal(byId(report.codecs, 'h264').verdict, 'hardware');
  assert.equal(byId(report.codecs, 'vp9').verdict, 'software');
  assert.equal(byId(report.codecs, 'av1').verdict, 'software');
  assert.equal(byId(report.codecs, 'av1').smooth, false);
  assert.equal(byId(report.codecs, 'hevc').verdict, 'unsupported');
  assert.equal(byId(report.codecs, 'h264').canPlay, 'probably');
  assert.equal(byId(report.codecs, 'hevc').canPlay, '');
  assert.equal(byId(report.codecs, 'hevc').mediaSource, false);
});

test('decodingInfo receives the 1080p probe configuration', async () => {
  const seen = [];
  await logic.runMediaCheck({ decodingInfo: async (config) => { seen.push(config); return { supported: true, smooth: true, powerEfficient: true }; } });
  assert.equal(seen.length, 4);
  for (const config of seen) {
    assert.equal(config.type, 'file');
    const { contentType, ...rest } = config.video;
    assert.ok(logic.CODECS.some((c) => c.contentType === contentType));
    assert.deepEqual(rest, { width: 1920, height: 1080, bitrate: 5000000, framerate: 30 });
  }
});

test('missing APIs give unknown, never a crash', async () => {
  const report = await logic.runMediaCheck({});
  assert.equal(report.codecs.length, 4);
  for (const codec of report.codecs) {
    assert.equal(codec.verdict, 'unknown');
    assert.equal(codec.smooth, null);
    assert.equal(codec.canPlay, null);
    assert.equal(codec.mediaSource, null);
  }
  for (const system of report.drm) assert.equal(system.state, 'unknown');
  assert.deepEqual(logic.guidance(report), []);
});

test('without MediaCapabilities only a unanimous no is unsupported', async () => {
  const none = await logic.runMediaCheck({ canPlayType: () => '', isTypeSupported: () => false });
  assert.ok(none.codecs.every((c) => c.verdict === 'unsupported'));
  const some = await logic.runMediaCheck({ canPlayType: () => 'maybe', isTypeSupported: () => false });
  assert.ok(some.codecs.every((c) => c.verdict === 'unknown'));
  const only = await logic.runMediaCheck({ canPlayType: () => '' });
  assert.ok(only.codecs.every((c) => c.verdict === 'unsupported'));
});

test('thrown and rejected probes are contained', async () => {
  const report = await logic.runMediaCheck({
    canPlayType: () => { throw new Error('boom'); },
    isTypeSupported: () => { throw new TypeError('boom'); },
    decodingInfo: async () => { throw new Error('rejected'); },
    requestKeySystemAccess: () => { throw new Error('sync throw'); },
  });
  assert.ok(report.codecs.every((c) => c.verdict === 'unknown' && c.smooth === null && c.canPlay === null && c.mediaSource === null));
  assert.ok(report.drm.every((s) => s.state === 'unavailable'));
});

test('malformed probe answers are ignored', async () => {
  const report = await logic.runMediaCheck({ canPlayType: () => 42, isTypeSupported: () => 'yes', decodingInfo: async () => ({ supported: 'true' }) });
  assert.ok(report.codecs.every((c) => c.verdict === 'unknown' && c.canPlay === null && c.mediaSource === null));
});

test('a never-resolving probe times out and the check still finishes', async () => {
  const started = Date.now();
  const report = await logic.runMediaCheck({ decodingInfo: never, canPlayType: () => 'probably', requestKeySystemAccess: never }, { timeoutMs: 30 });
  assert.ok(Date.now() - started < 2000);
  assert.ok(report.codecs.every((c) => c.verdict === 'unknown' && c.smooth === null && c.canPlay === 'probably'));
  assert.ok(report.drm.every((s) => s.state === 'unavailable'));
});

test('settle reports ok, throw and stall', async () => {
  assert.deepEqual(await logic.settle(() => 5, 50), { ok: true, value: 5 });
  assert.deepEqual(await logic.settle(async () => 6, 50), { ok: true, value: 6 });
  assert.deepEqual(await logic.settle(() => { throw new Error('x'); }, 50), { ok: false });
  assert.deepEqual(await logic.settle(never, 20), { ok: false });
});

test('DRM: available, unavailable, PlayReady falls back to the recommendation system', async () => {
  const calls = [];
  const report = await logic.runMediaCheck({
    requestKeySystemAccess: async (system, configs) => {
      calls.push([system, configs]);
      if (system === 'com.microsoft.playready.recommendation') return {};
      throw new Error('no');
    },
  });
  assert.equal(byId(report.drm, 'widevine').state, 'unavailable');
  assert.equal(byId(report.drm, 'playready').state, 'available');
  assert.deepEqual(calls.map(([system]) => system).sort(), ['com.microsoft.playready', 'com.microsoft.playready.recommendation', 'com.widevine.alpha']);
  const config = calls[0][1];
  assert.equal(config.length, 1);
  assert.deepEqual(config[0], {
    initDataTypes: ['cenc'],
    videoCapabilities: [{ contentType: 'video/mp4; codecs="avc1.640028"' }],
    audioCapabilities: [{ contentType: 'audio/mp4; codecs="mp4a.40.2"' }],
    persistentState: 'not-allowed',
    distinctiveIdentifier: 'not-allowed',
    sessionTypes: ['temporary'],
  });
  const both = await logic.runMediaCheck({ requestKeySystemAccess: async () => ({}) });
  assert.ok(both.drm.every((s) => s.state === 'available'));
});

test('DRM probe never touches createMediaKeys', async () => {
  let created = 0;
  await logic.runMediaCheck({ requestKeySystemAccess: async () => ({ createMediaKeys: () => { created += 1; } }) });
  assert.equal(created, 0);
});

test('guidance names the fix for each problem', async () => {
  const [h264, vp9] = logic.CODECS.map((c) => c.contentType);
  const report = await logic.runMediaCheck({
    decodingInfo: decodingBy({
      [h264]: { supported: true, smooth: true, powerEfficient: false },
      [vp9]: { supported: true, smooth: false, powerEfficient: true },
    }),
    requestKeySystemAccess: async () => { throw new Error('no'); },
  });
  const lines = logic.guidance(report);
  const text = lines.join('\n');
  assert.match(text, /Install the HEVC Video Extension from the Microsoft Store \(Windows\) so video can use hardware decode/);
  assert.match(text, /Install the AV1 Video Extension from the Microsoft Store/);
  assert.match(text, /H\.264 decodes in software\. The GPU driver may not offer hardware decode for it, so battery use can be higher/);
  assert.match(text, /VP9 at 1080p may not play smoothly/);
  assert.match(text, /Streaming services that need Widevine may not play/);
  assert.match(text, /Streaming services that need PlayReady may not play/);
  for (const line of lines) {
    assert.match(line, /^[A-Z]/);
    assert.doesNotMatch(line, /\p{Extended_Pictographic}/u);
    assert.match(line, /\.$/);
  }
});

test('guidance is empty when everything is hardware and DRM is available', async () => {
  const report = await logic.runMediaCheck({
    decodingInfo: info(true, true, true),
    requestKeySystemAccess: async () => ({}),
  });
  assert.deepEqual(logic.guidance(report), []);
});

test('verdict and DRM text and detail lines are readable', () => {
  assert.equal(logic.verdictText('hardware'), 'Hardware decode');
  assert.equal(logic.verdictText('software'), 'Software decode');
  assert.equal(logic.verdictText('unsupported'), 'Not supported');
  assert.equal(logic.verdictText('unknown'), 'Unknown');
  assert.equal(logic.drmText('available'), 'Available');
  assert.equal(logic.drmText('unavailable'), 'Not available');
  assert.equal(logic.drmText('unknown'), 'Unknown');
  assert.equal(logic.codecDetail({ id: 'av1', label: 'AV1', verdict: 'software', smooth: true, canPlay: 'maybe', mediaSource: false }), 'Player: maybe. Streaming: no. Smooth at 1080p: yes.');
  assert.equal(logic.codecDetail({ id: 'av1', label: 'AV1', verdict: 'unknown', smooth: null, canPlay: null, mediaSource: null }), 'This browser did not report support.');
});

test('the report has a fixed shape of ids, labels and primitives, no page-derived data', async () => {
  const report = await logic.runMediaCheck({
    canPlayType: () => 'probably', isTypeSupported: () => true, decodingInfo: info(true, true, true),
    requestKeySystemAccess: async () => ({ url: 'https://example.test/', title: 'page' }),
  });
  assert.deepEqual(Object.keys(report), ['codecs', 'drm']);
  assert.deepEqual(report.codecs.map((c) => c.id), ['h264', 'vp9', 'av1', 'hevc']);
  assert.deepEqual(report.drm.map((s) => s.id), ['widevine', 'playready']);
  for (const codec of report.codecs) assert.deepEqual(Object.keys(codec), ['id', 'label', 'verdict', 'smooth', 'canPlay', 'mediaSource']);
  for (const system of report.drm) assert.deepEqual(Object.keys(system), ['id', 'label', 'state']);
  assert.doesNotMatch(JSON.stringify(report), /example\.test|page/);
});

test('browserProbes is safe without browser globals', () => {
  assert.equal(typeof logic.browserProbes(), 'object');
});

test('the media pane stays client-side', () => {
  for (const file of ['media-logic.ts', 'settings.ts']) {
    const text = readFileSync(join(here, '..', 'src', 'panels', file), 'utf8');
    assert.doesNotMatch(text, /fetch\(|XMLHttpRequest|sendBeacon|WebSocket|createMediaKeys|createSession/);
  }
});
