// Beacon: GET <page origin>/__bowser_probe__/beacon?stage=<stage>&reply=<url-encoded JSON> (same-origin loopback).
(() => {
  const VARIANT = 'probe-mv3-key';
  const root = document.documentElement;
  root.setAttribute('data-bowser-probe', 'content-script');
  const beacon = (stage, detail) =>
    fetch(
      '/__bowser_probe__/beacon?stage=' + encodeURIComponent(stage) +
        '&reply=' + encodeURIComponent(JSON.stringify(detail)),
      { cache: 'no-store' }
    ).catch(() => {});
  beacon('content-script', { variant: VARIANT });
  try {
    chrome.runtime.sendMessage({ type: 'bowser-probe', variant: VARIANT }, (reply) => {
      const lastError = chrome.runtime.lastError ? String(chrome.runtime.lastError.message) : null;
      root.setAttribute('data-bowser-probe-reply', reply ? 'ok' : 'none');
      beacon('worker-reply', { reply: reply || null, error: lastError });
    });
  } catch (error) {
    root.setAttribute('data-bowser-probe-reply', 'error');
    beacon('send-failed', { error: String(error) });
  }
})();
