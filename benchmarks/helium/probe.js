// Served from the same origin in every arm. Frame IDs are visible pixels for optical validation.
const token = location.pathname.split('/')[1];
const report = async (phase, data) => {
  await fetch(`/${token}/${phase}`, { method: 'POST', body: JSON.stringify(data) });
};
const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));
const viewport = () => [innerWidth, innerHeight, devicePixelRatio];
const fcp = new Promise((resolve) => {
  const observer = new PerformanceObserver((entries) => {
    const entry = entries.getEntries().find(({ name }) => name === 'first-contentful-paint');
    if (entry) {
      observer.disconnect();
      resolve(entry.startTime);
    }
  });
  observer.observe({ type: 'paint', buffered: true });
});
const visibility = [];
document.addEventListener('visibilitychange', () => visibility.push(document.visibilityState));
window.addEventListener('load', async () => {
  await frame();
  await frame();
  visibility.length = 0;
  await report('ready', {
    fcp_ms: await fcp,
    title: document.title,
    count: document.querySelectorAll('#items li, #grid article').length,
    canvas: [document.getElementById('payload').width, document.getElementById('payload').height],
    viewport: viewport(),
    visible: !document.hidden,
  });
  const { animation_frames } = await (await fetch(`/${token}/control`, { cache: 'no-store' })).json();
  if (animation_frames === 0) {
    await report('done', { viewport: viewport(), visible: !document.hidden, visibility });
    return;
  }

  const payload = document.getElementById('payload');
  const display = document.createElement('canvas');
  display.width = 640;
  display.height = 320;
  display.setAttribute('aria-label', 'Animated canvas benchmark with a visible frame counter');
  Object.assign(display.style, { position: 'fixed', top: '10px', left: '10px', zIndex: '2' });
  document.body.append(display);
  const ctx = display.getContext('2d', { alpha: false });
  const timestamps = [];
  for (let i = 1; i <= animation_frames; i++) {
    timestamps.push(await frame());
    ctx.fillStyle = '#0f2540';
    ctx.fillRect(0, 0, display.width, display.height);
    ctx.drawImage(payload, (i * 7) % 160, (i * 3) % 80 + 40, 480, 200);
    ctx.fillStyle = 'white';
    ctx.fillRect(0, 0, 12, 18);
    for (let bit = 0; bit < 12; bit++) {
      ctx.fillStyle = i & (1 << bit) ? 'white' : 'black';
      ctx.fillRect(14 * (bit + 1), 0, 12, 18);
    }
  }
  // Leave the final drawing available for presentation before acknowledging completed work.
  await frame();
  await frame();
  await report('done', { timestamps, viewport: viewport(), visible: !document.hidden, visibility });
}, { once: true });
