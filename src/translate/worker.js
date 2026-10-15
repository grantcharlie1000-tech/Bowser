(function () {
  'use strict';
  /* Bowser translation worker page. Bowser opens it hidden, only while the user asks to translate, from
     a folder mapped to the virtual host bowser-translate.invalid. Its content policy (see worker.html)
     lets it read files from that folder and nothing else: no network, no other origin. Bowser sends
     text in as JSON messages and gets translations back the same way; nothing here stores anything.
     The engine is Mozilla's Bergamot translator (MPL-2.0), run on this machine from the hash-checked
     files in engine/ and models/<src>-<dst>/. */
  var W = window, bridge = W.chrome && W.chrome.webview;
  var PAIR = /^[a-z]{2,3}-[a-z]{2,3}$/;
  var MAX_MODELS = 3, MAX_ITEMS = 400;
  var CONFIG = [
    'beam-size: 1', 'normalize: 1.0', 'word-penalty: 0', 'max-length-break: 128', 'mini-batch-words: 1024',
    'workspace: 128', 'max-length-factor: 2.0', 'skip-cost: true', 'cpu-threads: 0', 'quiet: true',
    'quiet-translation: true', 'gemm-precision: int8shiftAlphaAll', 'alignment: soft'
  ].join('\n') + '\n';
  var M = null, service = null, models = new Map(), engine = null, queue = Promise.resolve();

  function post(o) { bridge.postMessage(JSON.stringify(o)); }
  function file(path) {
    return fetch(path).then(function (r) {
      if (!r.ok) throw new Error('file');
      return r.arrayBuffer();
    });
  }
  function memory(buffer, align) {
    var m = new M.AlignedMemory(buffer.byteLength, align);
    m.getByteArrayView().set(new Uint8Array(buffer));
    return m;
  }
  function loadEngine() {
    if (!engine) {
      engine = file('engine/bergamot-translator-worker.wasm').then(function (wasm) {
        return new Promise(function (resolve, reject) {
          var s = document.createElement('script');
          W.Module = { wasmBinary: wasm, print: function () {}, printErr: function () {}, onRuntimeInitialized: resolve };
          s.onerror = function () { reject(new Error('engine')); };
          s.src = 'engine/bergamot-translator-worker.js';
          document.head.appendChild(s);
        });
      }).then(function () {
        M = W.Module;
        service = new M.BlockingService({ cacheSize: 0 });
      }, function (e) { engine = null; throw e; });
    }
    return engine;
  }
  function loadModel(pair) {
    var dir = 'models/' + pair + '/';
    if (models.has(pair)) { var again = models.get(pair); models.delete(pair); models.set(pair, again); return Promise.resolve(); }
    return Promise.all([file(dir + 'model.bin'), file(dir + 'lex.bin'), file(dir + 'vocab.spm')]).then(function (f) {
      var vocabs = new M.AlignedMemoryList();
      vocabs.push_back(memory(f[2], 64));
      models.set(pair, new M.TranslationModel(CONFIG, memory(f[0], 256), memory(f[1], 64), vocabs, null));
      while (models.size > MAX_MODELS) {
        var oldest = models.keys().next().value;
        models.get(oldest).delete();
        models.delete(oldest);
      }
    });
  }
  function translate(route, texts) {
    var input = new M.VectorString(), options = new M.VectorResponseOptions(), result = null, out = [], i;
    try {
      for (i = 0; i < texts.length; i++) {
        input.push_back(texts[i]);
        options.push_back({ qualityScores: false, alignment: false, html: false });
      }
      result = route.length === 1
        ? service.translate(models.get(route[0]), input, options)
        : service.translateViaPivoting(models.get(route[0]), models.get(route[1]), input, options);
      for (i = 0; i < result.size(); i++) out.push(result.get(i).getTranslatedText());
      return out;
    } finally {
      input.delete(); options.delete();
      if (result) result.delete();
    }
  }
  function pairs(list, max) {
    return Array.isArray(list) && list.length >= 1 && list.length <= max && list.every(function (p) { return typeof p === 'string' && PAIR.test(p); });
  }
  function handle(m) {
    var id = m && Number.isInteger(m.id) ? m.id : 0;
    if (!m || typeof m !== 'object') return Promise.resolve();
    if (m.t === 'load' && pairs(m.pairs, 2)) {
      return loadEngine().then(function () { return Promise.all(m.pairs.map(loadModel)); }).then(
        function () { post({ t: 'loaded', id: id }); },
        function (e) { post({ t: 'error', id: id, m: e && e.message === 'engine' ? 'engine' : 'model' }); }
      );
    }
    if (m.t === 'tr' && pairs(m.route, 2) && Array.isArray(m.s) && m.s.length <= MAX_ITEMS && m.s.every(function (s) { return typeof s === 'string'; })) {
      if (!M || !m.route.every(function (p) { return models.has(p); })) { post({ t: 'error', id: id, m: 'model' }); return Promise.resolve(); }
      return new Promise(function (resolve) { setTimeout(resolve, 0); }).then(function () {
        try { post({ t: 'out', id: id, s: translate(m.route, m.s) }); } catch (e) { post({ t: 'error', id: id, m: 'translate' }); }
      });
    }
    post({ t: 'error', id: id, m: 'bad' });
    return Promise.resolve();
  }

  if (!bridge) return;
  bridge.addEventListener('message', function (e) {
    // Messages run one at a time, in the order they arrived.
    queue = queue.then(function () { return handle(e.data); }).catch(function () { post({ t: 'error', id: 0, m: 'bad' }); });
  });
  post({ t: 'ready' });
})();
