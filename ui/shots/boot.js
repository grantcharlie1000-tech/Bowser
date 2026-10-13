window.chrome = {
  webview: {
    listeners: [],
    addEventListener(_type, fn) { this.listeners.push(fn); },
    removeEventListener() {},
    postMessage() {},
  },
};
