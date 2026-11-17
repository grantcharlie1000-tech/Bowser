// Live run of the built Bowser against the spoiler fixtures. NOT part of `cargo test`: it opens a real,
// visible Bowser window, sends keystrokes (private tab) and takes window screenshots, so run it only when
// the machine is free. Status when this file was written: NOT RUN (deferred: user busy).
//
//   node extensions/spoiler-firewall/test/bowser/live-run.mjs <stage> [--bundle probe|real] [--exe path] [--out dir]
//
// stages: normal - top document + cross-origin / srcdoc / sandboxed / about:blank / blob: / data: frames, a clean
//                  control page, then a private tab (Ctrl+Shift+N) in the same window
//         guest  - the same fixture in a --guest window
//         popup  - needs a human click on the link (Bowser has no remote-debugging path: it refuses to start
//                  when WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS is set)
//
// How it observes: the injected script (probe-bundle.js, or the real bundle with --bundle real) cannot be read
// over CDP, so the PROBE reports each frame it runs in to /__report on the fixture server. Absence of a report
// for a frame is the evidence that the script did not run there. Screenshots (PrintWindow) are the visual
// evidence. The real bundle does not beacon, so with --bundle real only the screenshots are produced.
//
// Safety: launches only the exe it is given, with `--dev` and a throwaway --data-dir under %TEMP%; stops only the
// process id it started; sends keystrokes only while that window is foreground (win.ps1 refuses otherwise).
// The debug exe reads BOWSER_SPOILER_BUNDLE (debug builds only) to swap the bundle without a rebuild.

import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { serve } from './serve.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '..', '..', '..', '..');
const args = process.argv.slice(2);
const stage = args[0] ?? 'normal';
const opt = (name, fallback) => { const i = args.indexOf(`--${name}`); return i >= 0 ? args[i + 1] : fallback; };
const exe = opt('exe', path.join(repo, 'target', 'debug', 'bowser.exe'));
const bundleKind = opt('bundle', 'probe');
const bundle = bundleKind === 'real'
  ? path.join(repo, 'extensions', 'spoiler-firewall', 'dist', 'bowser', 'spoiler-firewall.js')
  : path.join(here, 'probe-bundle.js');
const outDir = opt('out', path.join(os.tmpdir(), 'bowser-spoiler-live-out'));
fs.mkdirSync(outDir, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function win(action, pid, extra = []) {
  const r = spawnSync('powershell', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', path.join(here, 'win.ps1'), '-Action', action, '-ProcessId', String(pid), ...extra], { encoding: 'utf8' });
  return (r.stdout || '').trim() + (r.status ? ` [exit ${r.status}]` : '');
}

/** Which frame kinds beaconed `inject`, and which of them shielded. */
function summarise(reports) {
  const kind = (r) => r.href.startsWith('about:srcdoc') ? 'srcdoc iframe'
    : r.href.startsWith('about:blank') ? 'about:blank iframe'
    : r.href.startsWith('blob:') ? 'blob: iframe'
    : r.href.startsWith('data:') ? 'data: iframe'
    : r.href.includes('sandboxed=1') ? 'sandboxed iframe'
    : r.href.includes('localhost:18082') ? 'cross-origin iframe'
    : r.href.includes('clean.html') ? 'control page (no spoiler)'
    : r.href.includes('popup=1') ? 'popup top document'
    : r.href.includes('private=1') ? 'private tab top document'
    : 'top document';
  const rows = new Map();
  for (const r of reports) {
    const k = kind(r);
    const row = rows.get(k) ?? { frame: k, injected: false, shielded: false, config: null };
    if (r.phase === 'inject') { row.injected = true; row.config = r.cfg; }
    if (r.phase === 'shield') row.shielded = true;
    rows.set(k, row);
  }
  return [...rows.values()];
}

async function main() {
  const server = serve();
  const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), `bowser-spoiler-${stage}-`));
  const env = { ...process.env, BOWSER_SPOILER_BUNDLE: bundle, BOWSER_UI: process.env.BOWSER_UI ?? 'html' };
  const start = stage === 'popup' ? 'http://127.0.0.1:18081/popup-opener.html' : 'http://127.0.0.1:18081/main.html';
  const launch = ['--dev', '--data-dir', dataDir, ...(stage === 'guest' ? ['--guest'] : []), '--open-url', start];
  const child = spawn(exe, launch, { env, stdio: 'ignore' });
  const pid = child.pid;
  const report = { stage, exe, bundle: bundleKind, pid, notes: [] };
  try {
    await sleep(9000);
    report.screenshotMain = win('shot', pid, ['-Out', path.join(outDir, `${stage}-main.png`)]);
    report.windowInfo = win('info', pid);
    if (stage === 'normal') {
      win('focus', pid);
      report.keys = [win('keys', pid, ['-Keys', '^+n'])];
      await sleep(2500);
      report.keys.push(win('keys', pid, ['-Keys', '^l']));
      await sleep(400);
      report.keys.push(win('keys', pid, ['-Keys', 'http://127.0.0.1:18081/main.html?private=1{ENTER}']));
      await sleep(7000);
      report.screenshotPrivate = win('shot', pid, ['-Out', path.join(outDir, 'normal-private.png')]);
      report.keys.push(win('keys', pid, ['-Keys', '^l']));
      await sleep(400);
      report.keys.push(win('keys', pid, ['-Keys', 'http://127.0.0.1:18081/clean.html{ENTER}']));
      await sleep(4000);
      report.screenshotClean = win('shot', pid, ['-Out', path.join(outDir, 'normal-clean.png')]);
    }
    if (stage === 'popup') {
      report.notes.push('Click the link in the opened window within 40 s.');
      await sleep(40000);
    }
  } finally {
    spawnSync('taskkill', ['/PID', String(pid), '/T', '/F'], { encoding: 'utf8' }); // this run's own tree only
    await sleep(800);
    server.stop();
    try { fs.rmSync(dataDir, { recursive: true, force: true }); } catch {}
  }
  report.frames = summarise(server.reports);
  report.rawReports = server.reports.length;
  fs.writeFileSync(path.join(outDir, `${stage}.json`), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
  process.exit(0);
}
main();
