// node --test benchmarks/auto/analyze.test.mjs  (synthetic logs only; no browser, input or network)
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { analyzeDirectory, bootstrapRatio, extractSample, parseLog, percentile, renderMarkdown } from './analyze.mjs';

const FREQ = 10_000_000;
const HEADER = 'event,elapsed_ms,qpc_ticks,tab_id,navigation_id,success,fixture_token,view,pid,private_bytes,count';
const ticks = msValue => Math.round(msValue * FREQ / 1000);
const row = (event, qpc, token = '') => `${event},0,${qpc},1,0,,${token},,0,0,0`;

function omniboxSample(pair, arm, dir, latencyMs, { broken = false } = {}) {
  const id = `${pair}-${arm}-omnibox_enter`;
  const start = 1_000_000_000 + pair * 1000;
  const token = `tok${pair}${arm[0]}`;
  const lines = [HEADER, row('process_start', start - ticks(900)),
    row('perf_omnibox_submit', start + ticks(2)),
    ...(broken ? [] : [row('perf_title_changed', start + ticks(latencyMs), `BOWSER_PERF_READY_${token}`)])];
  fs.mkdirSync(path.join(dir, 'logs'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'logs', `${id}.csv`), lines.join('\n') + '\n');
  return { id, pair, arm, workload: 'omnibox_enter', status: 'ok', log_flushed: true, freq: FREQ, log: `logs/${id}.csv`, tokens: { target: token }, marks: [{ name: 'omnibox_enter', qpc: start }] };
}

test('percentile and parseLog', () => {
  assert.equal(percentile([1, 2, 3, 4, 5], 0.5), 3);
  assert.equal(percentile([10, 20], 0.9), 19);
  const rows = parseLog(`${HEADER}\n${row('x', 42, 'a_b')}\n`);
  assert.equal(rows[0].qpc, 42);
  assert.equal(rows[0].fixture_token, 'a_b');
});

test('bootstrap CI brackets a known 1.10 ratio and is seed-stable', () => {
  const pairs = Array.from({ length: 10 }, (_, i) => { const base = 100 + (i % 5) * 3; return [base, base * 1.1]; });
  const a = bootstrapRatio(pairs, { seed: 7, resamples: 2000 });
  const b = bootstrapRatio(pairs, { seed: 7, resamples: 2000 });
  assert.deepEqual(a, b);
  assert.ok(Math.abs(a.ratio - 1.1) < 1e-9);
  assert.ok(a.lo <= 1.1 + 1e-9 && a.hi >= 1.1 - 1e-9);
  assert.equal(bootstrapRatio([[1, 2]]).lo, NaN);
});

test('end-to-end directory analysis with failed samples', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'bowser-auto-analyze-'));
  const samples = [];
  for (let pair = 1; pair <= 6; pair++) {
    samples.push(omniboxSample(pair, 'baseline', dir, 100 + pair));
    samples.push(omniboxSample(pair, 'candidate', dir, (100 + pair) * 0.8, { broken: pair === 6 }));
  }
  samples.push({ id: '7-baseline-omnibox_enter', pair: 7, arm: 'baseline', workload: 'omnibox_enter', status: 'aborted', reason: 'foreground_lost', log_flushed: false, freq: FREQ, marks: [] });
  fs.writeFileSync(path.join(dir, 'samples.jsonl'), samples.map(s => JSON.stringify(s)).join('\n') + '\n');
  const report = analyzeDirectory(dir, { seed: 3, resamples: 1000 });
  const primary = report.table.find(r => r.metric === 'enter_to_title_ms');
  assert.equal(primary.baseline.n, 6);
  assert.equal(primary.candidate.n, 5);
  assert.equal(primary.baseline.failed, 1);
  assert.equal(primary.candidate.failed, 1);
  assert.equal(primary.paired.n, 5, 'only complete pairs are compared');
  assert.ok(Math.abs(primary.paired.ratio - 0.8) < 0.01, `ratio ${primary.paired.ratio}`);
  assert.ok(primary.paired.hi < 0.9);
  assert.match(renderMarkdown(report), /driver_aborted:foreground_lost/);
  assert.match(renderMarkdown(report), /missing_omnibox_events/);
});

test('conversation sample requires a complete stream and typing', () => {
  const stats = (action, typed) => [{ token: 'c1', payload: { phase: 'stats', action, typed, loafSupported: true, gaps: Array.from({ length: 60 }, (_, i) => (i === 10 ? 80 : 16.7)), events: [['keydown', 1, 24, 2]], loafs: [[1, 60, 10]], scrolls: 12 } }];
  const sample = { workload: 'conversation', status: 'ok', log_flushed: true, freq: FREQ, tokens: { conversation: 'c1' }, expect: { chars: 20 }, marks: [] };
  const ok = extractSample(sample, [{ event: 'x', qpc: 1 }], stats('stream_complete', 20));
  assert.equal(ok.ok, true);
  assert.equal(ok.metrics.frames_over_50ms, 1);
  assert.equal(ok.metrics.loaf_blocking_ms, 10);
  assert.equal(extractSample(sample, [{ event: 'x', qpc: 1 }], stats('failed', 20)).ok, false);
  assert.equal(extractSample(sample, [{ event: 'x', qpc: 1 }], stats('stream_complete', 5)).ok, false);
});
