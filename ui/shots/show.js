const messages = {
  workspace: { v: 1, t: 'composer', tok: 4, kind: 'composer:workspace-new', mode: 'workspace-new', max: 40, name: 'Workspace 3' },
  add: { v: 1, t: 'composer', tok: 5, kind: 'composer:app-add', mode: 'app-add', max: 40, name: '', url: '' },
  edit: {
    v: 1, t: 'composer', tok: 6, kind: 'composer:app-edit:9', mode: 'app-edit', max: 40,
    name: 'Mail', url: 'not a url',
    error: { field: 'url', text: 'Enter an address starting with http:// or https://.' },
  },
};
const message = messages[new URLSearchParams(location.search).get('composer')];
const webview = window.chrome.webview;
if (message) {
  for (const listener of webview.listeners) listener({ data: message });
}
