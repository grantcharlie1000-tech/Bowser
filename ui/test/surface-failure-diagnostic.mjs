// Deterministic Node-only tests: no HTTP server, browser, product build, or native run.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import { createSurfaceFailureDiagnostics, installSurfaceFailureObservation, surfaceAssetClass, surfacePredicates, surfaceStderrCategories, SURFACE_DIAGNOSTIC_LIMITS } from './surface-failure-diagnostic-helpers.mjs';

const SECRET = 'PRIVATE_TOKEN_SENTINEL_https://private.invalid/account?secret=do-not-log';
const context = { source: 'candidate', view: 'settings', use: 'first', attempt: 1 };
const installed = 'data-surface-diagnostic-version="1" data-surface-error-overflow="no" data-surface-csp-overflow="no"';
const observed = (install = installSurfaceFailureObservation) => {
  const attrs = new Map(); const listeners = new Map();
  const window = { addEventListener: (name, listener) => listeners.set(name, listener) };
  const document = { documentElement: { setAttribute: (name, value) => attrs.set(name, value) } };
  install(window, document);
  return { attrs, emit: (kind, event) => listeners.get(kind)(event), dom: () => `<html ${[...attrs].map(([name, value]) => `${name}="${value}"`).join(' ')}></html>` };
};
const report = (diagnostic, token, dom, stderr) => JSON.parse(diagnostic.serialize(token, dom, stderr));

test('browser observer retains fixed original-error name categories without reading messages/stacks/URLs', () => {
  const observer = observed();
  const error = { name: 'SyntaxError', get message() { throw Error(SECRET); }, get stack() { throw Error(SECRET); }, get sourceURL() { throw Error(SECRET); } };
  observer.emit('unhandledrejection', { reason: error });
  observer.emit('error', { error: { name: 'TypeError', message: SECRET } });
  assert.equal(observer.attrs.get('data-surface-error'), 'yes');
  assert.equal(observer.attrs.get('data-surface-error-categories'), 'rejection:syntax_error,error:type_error');
  assert.ok(!observer.dom().includes(SECRET));
});

test('malicious browser error values and throwing getters remain unknown without string coercion', () => {
  const values = [undefined, null, SECRET, 3, Symbol(SECRET), { name: SECRET }, { name: { toString() { throw Error(SECRET); } } }, { get name() { throw Error(SECRET); } }, new Proxy({}, { get() { throw Error(SECRET); } })];
  for (const reason of values) {
    const observer = observed();
    assert.doesNotThrow(() => observer.emit('unhandledrejection', { reason }));
    assert.equal(observer.attrs.get('data-surface-error-categories'), 'rejection:unknown');
    assert.ok(!observer.dom().includes(SECRET));
  }
  const observer = observed();
  observer.emit('error', { get error() { throw Error(SECRET); } });
  observer.emit('unhandledrejection', { get reason() { throw Error(SECRET); } });
  assert.equal(observer.attrs.get('data-surface-error-categories'), 'error:unknown,rejection:unknown');
});

test('browser error categories are unique and capped at four', () => {
  const observer = observed();
  for (const name of ['TypeError', 'TypeError', 'SyntaxError', 'Error', 'RangeError', 'ReferenceError', SECRET]) observer.emit('error', { error: { name } });
  assert.equal(observer.attrs.get('data-surface-error-categories'), 'error:type_error,error:syntax_error,error:error,error:range_error');
  assert.equal(observer.attrs.get('data-surface-error-categories').split(',').length, SURFACE_DIAGNOSTIC_LIMITS.browserCategories);
});

test('browser CSP observation uses only bounded fixed directive categories and preserves error acceptance', () => {
  const observer = observed();
  for (const effectiveDirective of ['script-src', 'script-src-elem', { toString() { throw Error(SECRET); } }, 'trusted-types', 'default-src']) observer.emit('securitypolicyviolation', { effectiveDirective, blockedURI: SECRET, sourceFile: SECRET });
  assert.equal(observer.attrs.get('data-surface-csp-directives'), 'script-src,script-src-elem,other,trusted-types');
  assert.equal(observer.attrs.has('data-surface-error'), false);
  assert.equal(surfacePredicates(observer.dom()).clean, true);
  const inaccessible = observed();
  assert.doesNotThrow(() => inaccessible.emit('securitypolicyviolation', { get effectiveDirective() { throw Error(SECRET); } }));
  assert.equal(inaccessible.attrs.get('data-surface-csp-directives'), 'other');
});


test('installed version distinguishes an available collector with no events from unavailable telemetry', () => {
  const diagnostic = createSurfaceFailureDiagnostics(); const token = diagnostic.beginAttempt(context);
  const observer = observed();
  const available = report(diagnostic, token, observer.dom()).document;
  assert.equal(available.collector_status, 'available'); assert.equal(available.collector_version, 1);
  assert.deepEqual(available.error_categories, []); assert.deepEqual(available.csp_directives, []);
  assert.equal(available.error_categories_overflow, false); assert.equal(available.csp_directives_overflow, false);
  const unavailable = report(diagnostic, token, '<html></html>').document;
  assert.equal(unavailable.collector_status, 'unavailable'); assert.equal(unavailable.collector_version, null);
  assert.deepEqual(unavailable.error_categories, []); assert.deepEqual(unavailable.csp_directives, []);
  assert.equal(unavailable.error_categories_overflow, null); assert.equal(unavailable.csp_directives_overflow, null);
});

test('absent, malformed or unsupported installed markers never imply available error or CSP telemetry', () => {
  const diagnostic = createSurfaceFailureDiagnostics(); const token = diagnostic.beginAttempt(context);
  for (const marker of ['', 'data-surface-diagnostic-version="2"', "data-surface-diagnostic-version='1'", 'data-surface-diagnostic-version="1 "', 'data-surface-diagnostic-version="1', `data-surface-diagnostic-version="${SECRET}"`, `data-surface-diagnostic-version="${'1'.repeat(257)}"`]) {
    const value = report(diagnostic, token, `<html ${marker} data-surface-error-categories="rejection:syntax_error" data-surface-error-overflow="yes" data-surface-csp-directives="script-src" data-surface-csp-overflow="yes">`).document;
    assert.equal(value.collector_status, 'unavailable'); assert.equal(value.collector_version, null);
    assert.deepEqual(value.error_categories, []); assert.deepEqual(value.csp_directives, []);
    assert.equal(value.error_categories_overflow, null); assert.equal(value.csp_directives_overflow, null);
    assert.ok(!JSON.stringify(value).includes(SECRET));
  }
});

test('overflow flags mean additional unique categories, not duplicates at the cap', () => {
  const observer = observed();
  for (const name of ['TypeError', 'SyntaxError', 'Error', 'RangeError', 'TypeError', 'RangeError']) observer.emit('error', { error: { name } });
  for (const effectiveDirective of ['script-src', 'script-src-elem', 'trusted-types', 'default-src', 'script-src', 'trusted-types']) observer.emit('securitypolicyviolation', { effectiveDirective });
  assert.equal(observer.attrs.get('data-surface-error-overflow'), 'no');
  assert.equal(observer.attrs.get('data-surface-csp-overflow'), 'no');
  observer.emit('unhandledrejection', { reason: { name: 'SyntaxError' } });
  observer.emit('securitypolicyviolation', { effectiveDirective: 'script-src-attr' });
  assert.equal(observer.attrs.get('data-surface-error-overflow'), 'yes');
  assert.equal(observer.attrs.get('data-surface-csp-overflow'), 'yes');
  assert.equal(observer.attrs.get('data-surface-error-categories').split(',').length, 4);
  assert.equal(observer.attrs.get('data-surface-csp-directives').split(',').length, 4);
  const diagnostic = createSurfaceFailureDiagnostics(); const token = diagnostic.beginAttempt(context);
  const value = report(diagnostic, token, observer.dom()).document;
  assert.equal(value.error_categories_overflow, true); assert.equal(value.csp_directives_overflow, true);
});

test('overflow metadata accepts only exact yes/no, keeping malformed or absent flags unknown', () => {
  const diagnostic = createSurfaceFailureDiagnostics(); const token = diagnostic.beginAttempt(context);
  for (const [flag, expected] of [['yes', true], ['no', false], ['true', null], ['YES', null], ['yesno', null], ['', null], [SECRET, null], ['x'.repeat(257), null]]) {
    const value = report(diagnostic, token, `<html data-surface-diagnostic-version="1" data-surface-error-overflow="${flag}" data-surface-csp-overflow="${flag}">`).document;
    assert.equal(value.error_categories_overflow, expected); assert.equal(value.csp_directives_overflow, expected);
  }
  const absent = report(diagnostic, token, '<html data-surface-diagnostic-version="1">').document;
  assert.equal(absent.collector_status, 'available');
  assert.equal(absent.error_categories_overflow, null); assert.equal(absent.csp_directives_overflow, null);
});

test('shim observation installer is self-contained when serialized as the existing shim does', () => {
  const installer = runInNewContext(`(${installSurfaceFailureObservation.toString()})`);
  const observer = observed(installer);
  observer.emit('unhandledrejection', { reason: { name: 'SyntaxError', message: SECRET } });
  assert.equal(observer.attrs.get('data-surface-error-categories'), 'rejection:syntax_error');
});

test('asset classification returns no raw path and does not overclaim unknown scripts', () => {
  for (const [path, expected] of [['app.js', 'app'], ['settings-page.js', 'settings'], ['history-page.js', 'history'], ['start-page.js', 'start'], ['chunks/chunk-A10Z.js', 'chunk'], ['app.css', 'other'], ['shim.js', 'other'], ['index.html', 'other'], ['settings-page.js?' + SECRET, 'other'], ['chunks/chunk-secret.js', 'other'], [SECRET, 'other'], [null, 'other'], [{ toString() { throw Error(SECRET); } }, 'other']]) assert.equal(surfaceAssetClass(path), expected);
});

test('HTTP submission, status, finish, close and failure are distinct observations', () => {
  const diagnostic = createSurfaceFailureDiagnostics(); const token = diagnostic.beginAttempt(context);
  const done = diagnostic.beginRequest('settings-page.js');
  done.submitted(200, 831); done.finish(); done.close();
  const early = diagnostic.beginRequest('chunks/chunk-A.js');
  early.submitted(404, 0); early.close(); early.fail();
  const pending = diagnostic.beginRequest('app.js');
  const snapshots = report(diagnostic, token).requests;
  assert.deepEqual(snapshots, [
    { asset: 'settings', status: 200, body_bytes_submitted: 831, finished: true, closed: true, early_close: false, failed: false },
    { asset: 'chunk', status: 404, body_bytes_submitted: 0, finished: false, closed: true, early_close: true, failed: true },
    { asset: 'app', status: null, body_bytes_submitted: 0, finished: false, closed: false, early_close: false, failed: false },
  ]);
  assert.ok(Object.isFrozen(done)); assert.ok(Object.isFrozen(pending));
});

test('late previous-attempt response events stay with their immutable receipt owner', () => {
  const diagnostic = createSurfaceFailureDiagnostics();
  const first = diagnostic.beginAttempt(context);
  const prior = diagnostic.beginRequest('settings-page.js'); prior.submitted(200, 10);
  const second = diagnostic.beginAttempt({ ...context, attempt: 2 });
  const current = diagnostic.beginRequest('app.js'); current.submitted(200, 20);
  prior.finish(); prior.close(); prior.fail();
  diagnostic.browserOutcome(first, 'timeout'); diagnostic.browserOutcome(second, 'closed', 0);
  assert.deepEqual(report(diagnostic, first).requests.map((x) => [x.asset, x.finished, x.closed, x.failed]), [['settings', true, true, true]]);
  assert.deepEqual(report(diagnostic, second).requests.map((x) => [x.asset, x.finished, x.closed, x.failed]), [['app', false, false, false]]);
  assert.equal(report(diagnostic, first).browser.outcome, 'timeout');
  assert.equal(report(diagnostic, second).browser.outcome, 'closed');
  assert.equal(report(diagnostic, second).attribution, 'request_received_while_attempt_active');
});

test('response close before finish stays early-close even if a later finish arrives', () => {
  const diagnostic = createSurfaceFailureDiagnostics(); const token = diagnostic.beginAttempt(context);
  const receipt = diagnostic.beginRequest('settings-page.js'); receipt.close(); receipt.finish();
  assert.equal(report(diagnostic, token).requests[0].early_close, true);
});

test('request counts, byte counts, dropped counts and complete JSON output have explicit caps', () => {
  const diagnostic = createSurfaceFailureDiagnostics(); const token = diagnostic.beginAttempt(context);
  for (let i = 0; i < 2000; i++) {
    const receipt = diagnostic.beginRequest('settings-page.js'); receipt.submitted(599, Number.MAX_SAFE_INTEGER); receipt.close(); receipt.fail();
  }
  const json = diagnostic.serialize(token); const value = JSON.parse(json);
  assert.equal(value.requests.length, SURFACE_DIAGNOSTIC_LIMITS.requests);
  assert.equal(value.dropped_requests, SURFACE_DIAGNOSTIC_LIMITS.dropped);
  assert.ok(value.requests.every((x) => x.body_bytes_submitted === SURFACE_DIAGNOSTIC_LIMITS.bytes));
  assert.ok(Buffer.byteLength(json) <= SURFACE_DIAGNOSTIC_LIMITS.serializedBytes);
  assert.equal(value.byte_count_semantics, 'body_bytes_submitted_capped_not_client_receipt');
});

test('invalid status/bytes and unknown context are sanitized, including throwing context getters', () => {
  const diagnostic = createSurfaceFailureDiagnostics();
  const token = diagnostic.beginAttempt({ source: SECRET, get view() { throw Error(SECRET); }, use: { toString() { throw Error(SECRET); } }, attempt: Infinity });
  for (const [status, bytes] of [[SECRET, SECRET], [99, -1], [600, Infinity], [NaN, NaN]]) diagnostic.beginRequest(SECRET).submitted(status, bytes);
  diagnostic.browserOutcome(token, SECRET, SECRET);
  const value = report(diagnostic, token);
  assert.deepEqual(value.context, { source: 'unknown', view: 'unknown', use: 'unknown', browser_attempt: 0 });
  assert.ok(value.requests.every((x) => x.status === null && x.body_bytes_submitted === 0 && x.asset === 'other'));
  assert.deepEqual(value.browser, { outcome: 'unknown', exit_code: null, stderr_categories: [] });
  assert.ok(!diagnostic.serialize(token).includes(SECRET));
});

test('malformed and overlong DOM metadata is omitted or limited to exact allowlisted values', () => {
  const diagnostic = createSurfaceFailureDiagnostics(); const token = diagnostic.beginAttempt(context);
  for (const metadata of [
    `data-surface-error-categories='rejection:syntax_error'`,
    'data-surface-error-categories="rejection:syntax_error',
    `data-surface-error-categories="${'x'.repeat(257)}"`,
    'data-surface-error-categories="rejection:syntax_error<"',
    `data-surface-error-categories="${SECRET}"`,
  ]) assert.deepEqual(report(diagnostic, token, `<html ${installed} ${metadata}>`).document.error_categories, []);
  const value = report(diagnostic, token, `<html ${installed} data-surface-error-categories="error:type_error,${SECRET},rejection:syntax_error,error:error,error:range_error,error:eval_error" data-surface-csp-directives="${SECRET},script-src,other,script-src,default-src,trusted-types,script-src-elem">`);
  assert.deepEqual(value.document.error_categories, ['error:type_error', 'rejection:syntax_error', 'error:error', 'error:range_error']);
  assert.deepEqual(value.document.csp_directives, ['script-src', 'other', 'default-src', 'trusted-types']);
  assert.ok(!JSON.stringify(value).includes(SECRET));
});

test('stderr emits a few known fixed categories or unknown, never source lines', () => {
  assert.deepEqual(surfaceStderrCategories(''), []);
  assert.deepEqual(surfaceStderrCategories(SECRET), ['unknown']);
  assert.deepEqual(surfaceStderrCategories(`${SECRET} net::ERR_BLOCKED_BY_CSP net::ERR_CONNECTION_REFUSED net::ERR_ABORTED network_service_instance_impl.cc gpu_process_host.cc sandbox_linux.cc`), ['blocked_by_csp', 'connection_refused', 'request_failed', 'network_service']);
  assert.deepEqual(surfaceStderrCategories({ toString() { throw Error(SECRET); } }), []);
});

test('serialized report never retains secret-like sentinels from DOM, error values, paths, stderr or context', () => {
  const diagnostic = createSurfaceFailureDiagnostics(); const token = diagnostic.beginAttempt({ ...context, source: SECRET });
  const observer = observed(); observer.emit('unhandledrejection', { reason: { name: SECRET, message: SECRET, stack: SECRET } });
  const receipt = diagnostic.beginRequest(SECRET); receipt.submitted(404, 0); receipt.finish();
  const json = diagnostic.serialize(token, `${observer.dom()}<script src="${SECRET}"></script><p>${SECRET}</p>`, `${SECRET} net::ERR_ABORTED`);
  assert.ok(!json.includes(SECRET)); assert.ok(!json.includes('https://')); assert.ok(!json.includes('<script'));
  assert.equal(JSON.parse(json).document.error_categories[0], 'rejection:unknown');
});

test('HTTP success with SyntaxError or TypeError does not claim evaluation, fetch, or policy causality', () => {
  for (const name of ['TypeError', 'SyntaxError']) {
    const diagnostic = createSurfaceFailureDiagnostics(); const token = diagnostic.beginAttempt(context);
    const receipt = diagnostic.beginRequest('settings-page.js'); receipt.submitted(200, 500); receipt.finish(); receipt.close();
    const observer = observed(); observer.emit('unhandledrejection', { reason: { name } });
    const value = report(diagnostic, token, observer.dom());
    assert.equal(value.interpretation, 'categories_are_observations_not_causes_http_finish_is_not_browser_acceptance');
    assert.equal(Object.hasOwn(value, 'cause'), false);
    assert.equal(value.requests[0].status, 200);
  }
});

test('boolean acceptance predicates have exactly the existing regex/conversion semantics', () => {
  const readyValues = ['', 'data-surface-ready="yes"', 'data-surface-ready="no"', `data-surface-ready="yes" ${SECRET}`];
  const errors = ['', 'data-surface-error="yes"', 'page-load-failed', 'csp-not-enforced', 'data-surface-error="no"'];
  const timings = ['', '0', '12.5', '.', '1.2.3', '-1', '1e2', '9'.repeat(400)];
  for (const ready of readyValues) for (const error of errors) for (const timing of timings) {
    const dom = `<html ${ready} ${error} data-surface-ready-ms="${timing}" data-start-page-ready-ms="${timing}">`;
    const actual = surfacePredicates(dom);
    let readyAccepted = true; let cleanAccepted = true;
    try { assert.match(dom, /data-surface-ready="yes"/); } catch { readyAccepted = false; }
    try { assert.doesNotMatch(dom, /data-surface-error="yes"|page-load-failed|csp-not-enforced/); } catch { cleanAccepted = false; }
    assert.deepEqual(actual, {
      ready: readyAccepted,
      clean: cleanAccepted,
      readyEndpoint: Number.isFinite(Number(/data-surface-ready-ms="([0-9.]+)"/.exec(dom)?.[1])),
      startEndpoint: Number.isFinite(Number(/data-start-page-ready-ms="([0-9.]+)"/.exec(dom)?.[1])),
    });
  }
});

test('boolean assertion failure output does not include raw DOM', () => {
  const dom = `<html><p>${SECRET}</p><title>page-load-failed</title></html>`;
  try { assert.ok(surfacePredicates(dom).ready, 'settings: module did not register and send ready'); }
  catch (error) { assert.ok(!String(error).includes(SECRET)); assert.equal(error.actual, false); return; }
  assert.fail('expected ready assertion to fail');
});

test('serialization has fixed safe fallback for exceptions, invalid outputs and output-limit overflow', () => {
  const diagnostic = createSurfaceFailureDiagnostics(); const token = diagnostic.beginAttempt(context);
  assert.equal(diagnostic.serialize(token, SECRET, SECRET, () => { throw Error(SECRET); }), '{"schema":1,"diagnostic_unavailable":"serialization_failed"}');
  for (const output of [undefined, 'x'.repeat(SURFACE_DIAGNOSTIC_LIMITS.serializedBytes + 1), '💡'.repeat(SURFACE_DIAGNOSTIC_LIMITS.serializedBytes / 2)]) assert.equal(diagnostic.serialize(token, '', '', () => output), '{"schema":1,"diagnostic_unavailable":"output_limit"}');
  assert.equal(report(diagnostic, null, null, null).context.view, 'unknown');
});

test('harness retains exact budgets, retries, launch URLs, endpoint assertions and non-consuming error observation', () => {
  const source = readFileSync(new URL('./surface-modules.mjs', import.meta.url), 'utf8');
  assert.match(source, /const LAUNCH_TIMEOUT_MS = 30000;/);
  assert.match(source, /const LAUNCH_ATTEMPTS = 2;/);
  assert.match(source, /timeout: 180000/);
  assert.match(source, /'--virtual-time-budget=2500'/);
  assert.match(source, /attempt >= LAUNCH_ATTEMPTS \|\| !\/browser timeout\/\.test\(e\.message\)/);
  assert.match(source, /\$\{label\}\/index\.html\?view=\$\{view\}/);
  assert.match(source, /assert\.ok\(Number\.isFinite\(readyMs\)/);
  assert.match(source, /if \(view === 'split'\) assert\.ok\(Number\.isFinite\(startMs\)/);
  assert.match(source, /res\.once\(errorMonitor,/);
  assert.doesNotMatch(source, /res\.once\('error'/);
  assert.doesNotMatch(source, /assert\.(?:match|doesNotMatch)\(result\.out|result\.err\.slice/);
});


test('npm pretest runs diagnostics once while the existing surface smoke remains in test', () => {
  const { scripts } = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));
  assert.equal(scripts.pretest, 'node --test --test-concurrency=1 test/surface-failure-diagnostic.mjs');
  assert.equal(scripts.test.includes('test/surface-failure-diagnostic.mjs'), false);
  assert.equal(scripts.test.split(/\s+/).filter((entry) => entry === 'test/surface-modules.mjs').length, 1);
});
