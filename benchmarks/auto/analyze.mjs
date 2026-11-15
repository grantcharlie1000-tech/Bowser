// Offline analysis for benchmarks/auto/run-pairs.ps1 output.
// Usage: node analyze.mjs <outDir> [--json] [--seed N] [--resamples N]
// Inputs in outDir: samples.jsonl (driver receipts), logs/<sample>.csv (Bowser --diagnostic-log,
// QPC ticks), fixture-log.jsonl (fixture server receipts, incl. posted in-page stats).
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const TITLE_PREFIX = 'BOWSER_PERF_READY_';

export function parseLog(text) {
  const lines = text.split(/\r?\n/).filter(Boolean);
  if (!lines.length) return [];
  const header = lines[0].split(',');
  return lines.slice(1).map(line => {
    const cells = line.split(',');
    const row = {};
    header.forEach((name, i) => { row[name] = cells[i] ?? ''; });
    row.qpc = Number(row.qpc_ticks);
    return row;
  });
}

const first = (rows, event, from = -Infinity, match = () => true) => rows.find(r => r.event === event && r.qpc >= from && match(r));
const ms = (ticks, freq) => (ticks / freq) * 1000;

export function percentile(values, p) {
  if (!values.length) return NaN;
  const sorted = [...values].sort((a, b) => a - b);
  const rank = (sorted.length - 1) * p;
  const lo = Math.floor(rank), hi = Math.ceil(rank);
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (rank - lo);
}
export const median = values => percentile(values, 0.5);

// Returns { ok, reason, metrics, primary } for one driver receipt.
export function extractSample(sample, rows, statsRows) {
  const freq = sample.freq;
  const mark = name => sample.marks?.find(m => m.name === name)?.qpc;
  const titleOf = token => r => r.fixture_token === TITLE_PREFIX + token;
  const fail = reason => ({ ok: false, reason, metrics: {}, primary: null });
  if (sample.status !== 'ok') return fail(`driver_${sample.status}:${sample.reason ?? ''}`);
  if (!sample.log_flushed) return fail('log_not_flushed');
  if (!rows.length) return fail('empty_log');
  const metrics = {};
  switch (sample.workload) {
    case 'startup': {
      const start = mark('launch');
      const end = first(rows, 'perf_title_changed', start, titleOf(sample.tokens.startup));
      if (!end) return fail('missing_title_nonce');
      metrics.launch_to_title_ms = ms(end.qpc - start, freq);
      const ps = first(rows, 'process_start');
      if (ps) metrics.launch_to_process_start_ms = ms(ps.qpc - start, freq);
      const usable = first(rows, 'page_usable');
      if (usable) metrics.launch_to_page_usable_ms = ms(usable.qpc - start, freq);
      return { ok: true, metrics, primary: 'launch_to_title_ms' };
    }
    case 'new_tab': {
      const start = mark('new_tab');
      const native = first(rows, 'perf_input_new_tab', start);
      const end = first(rows, 'perf_new_tab_returned', start);
      if (!native || !end) return fail('missing_new_tab_events');
      metrics.input_to_handler_return_ms = ms(end.qpc - start, freq);
      metrics.input_to_native_dispatch_ms = ms(native.qpc - start, freq);
      metrics.handler_ms = ms(end.qpc - native.qpc, freq);
      return { ok: true, metrics, primary: 'input_to_handler_return_ms' };
    }
    case 'tab_switch': {
      const start = mark('tab_switch');
      if (!first(rows, 'tab_discarded') || first(rows, 'tab_discarded').qpc > start) return fail('tab_not_discarded_before_switch');
      const restored = first(rows, 'tab_restored', start);
      if (!restored) return fail('tab_not_restored');
      const select = first(rows, 'perf_tab_select_start', start);
      const end = first(rows, 'perf_title_changed', start, titleOf(sample.tokens.sleeper));
      if (!select || !end) return fail('missing_switch_events');
      metrics.input_to_content_title_ms = ms(end.qpc - start, freq);
      metrics.input_to_native_dispatch_ms = ms(select.qpc - start, freq);
      metrics.restore_to_title_ms = ms(end.qpc - restored.qpc, freq);
      return { ok: true, metrics, primary: 'input_to_content_title_ms' };
    }
    case 'omnibox_enter': {
      const start = mark('omnibox_enter');
      const submit = first(rows, 'perf_omnibox_submit', start) ?? first(rows, 'enter_received', start);
      const end = first(rows, 'perf_title_changed', start, titleOf(sample.tokens.target));
      if (!submit || !end) return fail('missing_omnibox_events');
      metrics.enter_to_title_ms = ms(end.qpc - start, freq);
      metrics.input_to_native_dispatch_ms = ms(submit.qpc - start, freq);
      const done = first(rows, 'navigation_completed', start);
      if (done) metrics.enter_to_navigation_completed_ms = ms(done.qpc - start, freq);
      return { ok: true, metrics, primary: 'enter_to_title_ms' };
    }
    case 'settings_open': {
      const start = mark('settings_enter');
      if (!first(rows, 'perf_settings_open', start)) return fail('settings_open_not_reached');
      const end = first(rows, 'perf_settings_ready', start);
      if (!end) return fail('settings_never_ready');
      metrics.enter_to_settings_ready_ms = ms(end.qpc - start, freq);
      const shown = first(rows, 'perf_internal_shown', start);
      if (shown) metrics.enter_to_internal_shown_ms = ms(shown.qpc - start, freq);
      return { ok: true, metrics, primary: 'enter_to_settings_ready_ms' };
    }
    case 'conversation': {
      const mine = statsRows.filter(r => r.token === sample.tokens.conversation && r.payload?.phase === 'stats');
      if (mine.length !== 1) return fail(`stats_posts_${mine.length}`);
      const p = mine[0].payload;
      if (p.action !== 'stream_complete') return fail(`stream_${p.action}`);
      if (p.typed < sample.expect.chars) return fail(`typed_${p.typed}_of_${sample.expect.chars}`);
      const gaps = p.gaps ?? [];
      if (gaps.length < 20) return fail('too_few_frames');
      const slow = (p.events ?? []).map(e => e[2]);
      metrics.frame_gap_p95_ms = percentile(gaps, 0.95);
      metrics.frame_gap_max_ms = Math.max(...gaps);
      metrics.frames_over_50ms = gaps.filter(g => g > 50).length;
      metrics.slow_event_count = slow.length;
      metrics.slow_event_max_ms = slow.length ? Math.max(...slow) : 0;
      if (p.loafSupported) {
        metrics.loaf_count = (p.loafs ?? []).length;
        metrics.loaf_blocking_ms = (p.loafs ?? []).reduce((sum, l) => sum + l[2], 0);
      }
      metrics.scroll_events = p.scrolls;
      return { ok: true, metrics, primary: 'frame_gap_p95_ms' };
    }
    default:
      return fail('unknown_workload');
  }
}

function mulberry32(seed) {
  let a = seed >>> 0;
  return () => { a = (a + 0x6D2B79F5) >>> 0; let t = a; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
}

// pairs: [[baselineValue, candidateValue], ...] (complete pairs only).
export function bootstrapRatio(pairs, { resamples = 10000, seed = 1 } = {}) {
  const n = pairs.length;
  if (n < 2) return { n, ratio: n ? median(pairs.map(p => p[1])) / median(pairs.map(p => p[0])) : NaN, lo: NaN, hi: NaN, pair_median: NaN };
  const rand = mulberry32(seed);
  const stat = sample => median(sample.map(p => p[1])) / median(sample.map(p => p[0]));
  const draws = [];
  for (let i = 0; i < resamples; i++) {
    const sample = Array.from({ length: n }, () => pairs[Math.floor(rand() * n)]);
    const value = stat(sample);
    if (Number.isFinite(value)) draws.push(value);
  }
  return {
    n,
    ratio: stat(pairs),
    lo: percentile(draws, 0.025),
    hi: percentile(draws, 0.975),
    pair_median: median(pairs.map(p => p[1] / p[0])),
  };
}

export function analyzeDirectory(dir, options = {}) {
  const samples = fs.readFileSync(path.join(dir, 'samples.jsonl'), 'utf8').split(/\r?\n/).filter(Boolean).map(JSON.parse);
  const fixturePath = path.join(dir, 'fixture-log.jsonl');
  const statsRows = fs.existsSync(fixturePath)
    ? fs.readFileSync(fixturePath, 'utf8').split(/\r?\n/).filter(Boolean).map(l => { try { return JSON.parse(l); } catch { return null; } }).filter(r => r && r.path === '/auto-stats')
    : [];
  const results = [];
  for (const sample of samples) {
    const logPath = sample.log ? path.join(dir, sample.log) : null;
    const rows = logPath && fs.existsSync(logPath) ? parseLog(fs.readFileSync(logPath, 'utf8')) : [];
    results.push({ sample, ...extractSample(sample, rows, statsRows) });
  }
  const table = [];
  const workloads = [...new Set(results.map(r => r.sample.workload))];
  for (const workload of workloads) {
    const inWorkload = results.filter(r => r.sample.workload === workload);
    const count = (arm, ok) => inWorkload.filter(r => r.sample.arm === arm && r.ok === ok).length;
    const failures = {};
    for (const r of inWorkload) if (!r.ok) failures[`${r.sample.arm}:${r.reason}`] = (failures[`${r.sample.arm}:${r.reason}`] ?? 0) + 1;
    const metricNames = [...new Set(inWorkload.filter(r => r.ok).flatMap(r => Object.keys(r.metrics)))];
    for (const metric of metricNames) {
      const values = arm => inWorkload.filter(r => r.ok && r.sample.arm === arm && metric in r.metrics).map(r => r.metrics[metric]);
      const byPair = new Map();
      for (const r of inWorkload) if (r.ok && metric in r.metrics) byPair.set(`${r.sample.pair}:${r.sample.arm}`, r.metrics[metric]);
      const pairIds = [...new Set(inWorkload.map(r => r.sample.pair))];
      const complete = pairIds.filter(id => byPair.has(`${id}:baseline`) && byPair.has(`${id}:candidate`)).map(id => [byPair.get(`${id}:baseline`), byPair.get(`${id}:candidate`)]);
      const ratio = bootstrapRatio(complete, options);
      table.push({
        workload, metric,
        primary: inWorkload.some(r => r.ok && r.primary === metric),
        baseline: { n: values('baseline').length, median: median(values('baseline')), p90: percentile(values('baseline'), 0.9), failed: count('baseline', false) },
        candidate: { n: values('candidate').length, median: median(values('candidate')), p90: percentile(values('candidate'), 0.9), failed: count('candidate', false) },
        paired: ratio,
        failures,
      });
    }
    if (!metricNames.length) table.push({ workload, metric: null, primary: false, baseline: { n: 0, failed: count('baseline', false) }, candidate: { n: 0, failed: count('candidate', false) }, paired: null, failures });
  }
  return { table, samples: results.length };
}

const f = (v, d = 2) => (typeof v === 'number' && Number.isFinite(v) ? v.toFixed(d) : '-');
export function renderMarkdown(report) {
  const lines = ['| workload | metric | base n/fail | base med | base p90 | cand n/fail | cand med | cand p90 | ratio cand/base (95% CI) | pairs |', '|---|---|---|---|---|---|---|---|---|---|'];
  for (const row of report.table) {
    const p = row.paired;
    lines.push(`| ${row.workload} | ${row.metric ?? '(no valid samples)'}${row.primary ? ' *' : ''} | ${row.baseline.n}/${row.baseline.failed} | ${f(row.baseline.median)} | ${f(row.baseline.p90)} | ${row.candidate.n}/${row.candidate.failed} | ${f(row.candidate.median)} | ${f(row.candidate.p90)} | ${p ? `${f(p.ratio, 3)} (${f(p.lo, 3)}..${f(p.hi, 3)})` : '-'} | ${p ? p.n : 0} |`);
  }
  const seen = new Set();
  const reasons = report.table.filter(r => Object.keys(r.failures).length && !seen.has(r.workload) && seen.add(r.workload));
  if (reasons.length) { lines.push('', 'Failed samples:'); for (const r of reasons) lines.push(`- ${r.workload}: ${JSON.stringify(r.failures)}`); }
  lines.push('', '* primary metric. Ratio is median(candidate)/median(baseline) over complete pairs; CI is a percentile bootstrap over pairs.');
  return lines.join('\n');
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  const args = process.argv.slice(2);
  const dir = args.find(a => !a.startsWith('--') && !/^\d+$/.test(a));
  const num = name => { const i = args.indexOf(name); return i >= 0 ? Number(args[i + 1]) : undefined; };
  if (!dir) { console.error('usage: node analyze.mjs <outDir> [--json] [--seed N] [--resamples N]'); process.exit(2); }
  const report = analyzeDirectory(dir, { seed: num('--seed') ?? 1, resamples: num('--resamples') ?? 10000 });
  console.log(args.includes('--json') ? JSON.stringify(report, null, 2) : renderMarkdown(report));
}
