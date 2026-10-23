// Test-harness observations only. No messages, stacks, URLs, DOM, or response bodies
// are retained in a diagnostic. Categories do not establish a failure's cause.
export const SURFACE_DIAGNOSTIC_LIMITS = Object.freeze({ browserCategories: 4, cspCategories: 4, requests: 32, dropped: 999, bytes: 16777216, serializedBytes: 8192 });

// Self-contained so the existing shim can install this function without another asset,
// eval, import interception, or any change to production scripts or CSP.
export function installSurfaceFailureObservation(scope, doc) {
  const errors = []; const directives = [];
  doc.documentElement.setAttribute('data-surface-error-overflow', 'no');
  doc.documentElement.setAttribute('data-surface-csp-overflow', 'no');
  const read = (value, key) => { try { return value?.[key]; } catch { return undefined; } };
  const category = (value) => {
    switch (read(value, 'name')) {
      case 'Error': return 'error';
      case 'TypeError': return 'type_error';
      case 'SyntaxError': return 'syntax_error';
      case 'ReferenceError': return 'reference_error';
      case 'RangeError': return 'range_error';
      case 'EvalError': return 'eval_error';
      case 'URIError': return 'uri_error';
      case 'AggregateError': return 'aggregate_error';
      case 'NetworkError': return 'network_error';
      case 'SecurityError': return 'security_error';
      case 'AbortError': return 'abort_error';
      case 'NotAllowedError': return 'not_allowed_error';
      default: return 'unknown';
    }
  };
  const record = (kind, event, key) => {
    // Preserve the existing error flag even when the original error is inaccessible.
    doc.documentElement.setAttribute('data-surface-error', 'yes');
    const item = `${kind}:${category(read(event, key))}`;
    if (!errors.includes(item)) {
      if (errors.length < 4) errors.push(item);
      else doc.documentElement.setAttribute('data-surface-error-overflow', 'yes');
    }
    doc.documentElement.setAttribute('data-surface-error-categories', errors.join(','));
  };
  scope.addEventListener('error', (event) => record('error', event, 'error'));
  scope.addEventListener('unhandledrejection', (event) => record('rejection', event, 'reason'));
  scope.addEventListener('securitypolicyviolation', (event) => {
    const value = read(event, 'effectiveDirective');
    const item = ['script-src', 'script-src-elem', 'script-src-attr', 'require-trusted-types-for', 'trusted-types', 'default-src'].includes(value) ? value : 'other';
    if (!directives.includes(item)) {
      if (directives.length < 4) directives.push(item);
      else doc.documentElement.setAttribute('data-surface-csp-overflow', 'yes');
    }
    doc.documentElement.setAttribute('data-surface-csp-directives', directives.join(','));
  });
  // Written last: absent, unsupported or malformed metadata must not look like a
  // functioning collector that simply observed no events. Never affects acceptance.
  doc.documentElement.setAttribute('data-surface-diagnostic-version', '1');
}

const errorCategories = ['error', 'type_error', 'syntax_error', 'reference_error', 'range_error', 'eval_error', 'uri_error', 'aggregate_error', 'network_error', 'security_error', 'abort_error', 'not_allowed_error', 'unknown'];
const errorTokens = new Set(['error', 'rejection'].flatMap((kind) => errorCategories.map((category) => `${kind}:${category}`)));
const cspTokens = new Set(['script-src', 'script-src-elem', 'script-src-attr', 'require-trusted-types-for', 'trusted-types', 'default-src', 'other']);
const views = new Set(['toolbar', 'tabs', 'rail', 'panel', 'popup', 'bar', 'dialog', 'hover', 'split', 'settings', 'history', 'downloads']);
const read = (value, key) => { try { return value?.[key]; } catch { return undefined; } };
const oneOf = (value, allowed) => allowed.has(value) ? value : 'unknown';
const boundedInteger = (value, max) => Number.isInteger(value) && value >= 0 ? Math.min(value, max) : 0;

// These are exactly the pre-existing acceptance regexes and numeric conversions.
// Boolean assertions using them do not include raw --dump-dom output in Node errors.
export function surfacePredicates(dom) {
  return {
    ready: /data-surface-ready="yes"/.test(dom),
    clean: !/data-surface-error="yes"|page-load-failed|csp-not-enforced/.test(dom),
    readyEndpoint: Number.isFinite(Number(/data-surface-ready-ms="([0-9.]+)"/.exec(dom)?.[1])),
    startEndpoint: Number.isFinite(Number(/data-start-page-ready-ms="([0-9.]+)"/.exec(dom)?.[1])),
  };
}

// Diagnostic metadata is stricter than acceptance and never returns unmatched text.
// Overlong/malformed attributes remain unavailable, not an invented explanation.
const attributeValue = (dom, attribute) => new RegExp(`\\s${attribute}="([^"<>]{0,256})"`).exec(dom)?.[1];
const attributeTokens = (dom, attribute, allowed, limit) => {
  const value = attributeValue(dom, attribute);
  if (value === undefined) return [];
  return [...new Set(value.split(',').filter((token) => allowed.has(token)))].slice(0, limit);
};
const overflowFlag = (dom, attribute) => {
  const value = attributeValue(dom, attribute);
  return value === 'yes' ? true : value === 'no' ? false : null;
};

export function surfaceAssetClass(path) {
  if (path === 'app.js') return 'app';
  if (path === 'settings-page.js') return 'settings';
  if (path === 'history-page.js') return 'history';
  if (path === 'start-page.js') return 'start';
  if (typeof path === 'string' && /^chunks\/chunk-[A-Z0-9]+\.js$/.test(path)) return 'chunk';
  return 'other';
}

export function surfaceStderrCategories(stderr) {
  if (typeof stderr !== 'string' || !stderr.length) return [];
  const categories = [];
  // Literal Chromium/OS labels only. Never copy the line around a match.
  for (const [pattern, category] of [
    [/net::ERR_BLOCKED_BY_CSP/, 'blocked_by_csp'],
    [/net::ERR_CONNECTION_REFUSED/, 'connection_refused'],
    [/net::ERR_(?:FILE_NOT_FOUND|ABORTED)/, 'request_failed'],
    [/network_service_instance_impl\.cc/, 'network_service'],
    [/gpu_process_host\.cc/, 'gpu_process'],
    [/sandbox_linux\.cc/, 'sandbox'],
  ]) if (pattern.test(stderr) && categories.length < 4) categories.push(category);
  return categories.length ? categories : ['unknown'];
}

export function createSurfaceFailureDiagnostics() {
  const attempts = new WeakMap(); let active = null;
  const noopReceipt = Object.freeze({ submitted() {}, finish() {}, close() {}, fail() {} });
  return Object.freeze({
    beginAttempt(context) {
      const token = Object.freeze({});
      const attempt = read(context, 'attempt');
      active = {
        context: Object.freeze({ source: oneOf(read(context, 'source'), new Set(['candidate', 'baseline'])), view: oneOf(read(context, 'view'), views), use: oneOf(read(context, 'use'), new Set(['first', 'warm'])), browser_attempt: attempt === 1 || attempt === 2 ? attempt : 0 }),
        requests: [], dropped: 0, outcome: 'unknown', exitCode: null,
      };
      attempts.set(token, active);
      return token;
    },
    beginRequest(path) {
      // Capture one immutable attempt reference at receipt time. A late finish/close
      // cannot be relabelled with whatever attempt happens to be active afterward.
      const owner = active;
      if (!owner) return noopReceipt;
      if (owner.requests.length >= SURFACE_DIAGNOSTIC_LIMITS.requests) { owner.dropped = Math.min(owner.dropped + 1, SURFACE_DIAGNOSTIC_LIMITS.dropped); return noopReceipt; }
      const record = { asset: surfaceAssetClass(path), status: null, body_bytes_submitted: 0, finished: false, closed: false, early_close: false, failed: false };
      owner.requests.push(record);
      return Object.freeze({
        submitted(status, bytes) {
          record.status = Number.isInteger(status) && status >= 100 && status <= 599 ? status : null;
          record.body_bytes_submitted = boundedInteger(bytes, SURFACE_DIAGNOSTIC_LIMITS.bytes);
        },
        finish() { record.finished = true; },
        close() { record.closed = true; record.early_close = !record.finished; },
        fail() { record.failed = true; },
      });
    },
    browserOutcome(token, outcome, code) {
      const owner = attempts.get(token);
      if (!owner) return;
      owner.outcome = oneOf(outcome, new Set(['closed', 'timeout', 'spawn_error']));
      owner.exitCode = Number.isInteger(code) && code >= -2147483648 && code <= 4294967295 ? code : null;
    },
    serialize(token, dom = '', stderr = '', stringify = JSON.stringify) {
      try {
        const owner = attempts.get(token);
        const output = typeof dom === 'string' ? dom : '';
        const collectorAvailable = attributeValue(output, 'data-surface-diagnostic-version') === '1';
        const report = {
          schema: 1,
          context: owner?.context ?? { source: 'unknown', view: 'unknown', use: 'unknown', browser_attempt: 0 },
          attribution: 'request_received_while_attempt_active',
          browser: { outcome: owner?.outcome ?? 'unknown', exit_code: owner?.exitCode ?? null, stderr_categories: surfaceStderrCategories(stderr) },
          document: {
            ...surfacePredicates(output),
            collector_status: collectorAvailable ? 'available' : 'unavailable',
            collector_version: collectorAvailable ? 1 : null,
            page_load_failed: /page-load-failed/.test(output),
            csp_not_enforced: /csp-not-enforced/.test(output),
            error_categories: collectorAvailable ? attributeTokens(output, 'data-surface-error-categories', errorTokens, SURFACE_DIAGNOSTIC_LIMITS.browserCategories) : [],
            error_categories_overflow: collectorAvailable ? overflowFlag(output, 'data-surface-error-overflow') : null,
            csp_directives: collectorAvailable ? attributeTokens(output, 'data-surface-csp-directives', cspTokens, SURFACE_DIAGNOSTIC_LIMITS.cspCategories) : [],
            csp_directives_overflow: collectorAvailable ? overflowFlag(output, 'data-surface-csp-overflow') : null,
          },
          requests: owner?.requests.map((request) => ({ ...request })) ?? [],
          dropped_requests: owner?.dropped ?? 0,
          byte_count_semantics: 'body_bytes_submitted_capped_not_client_receipt',
          interpretation: 'categories_are_observations_not_causes_http_finish_is_not_browser_acceptance',
        };
        const encoded = stringify(report);
        if (typeof encoded !== 'string' || Buffer.byteLength(encoded, 'utf8') > SURFACE_DIAGNOSTIC_LIMITS.serializedBytes) return '{"schema":1,"diagnostic_unavailable":"output_limit"}';
        return encoded;
      } catch { return '{"schema":1,"diagnostic_unavailable":"serialization_failed"}'; }
    },
  });
}
