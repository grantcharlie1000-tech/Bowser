# Browser extensions

## What WebView2 offers

Checked against the webview2-com 0.39.1 bindings in the cargo registry:

- `ICoreWebView2EnvironmentOptions6::SetAreBrowserExtensionsEnabled` turns extension support on
  when the environment is created.
- `ICoreWebView2Profile7::AddBrowserExtension(folder, handler)` loads an unpacked extension folder.
  The completion returns an `ICoreWebView2BrowserExtension`.
- `ICoreWebView2BrowserExtension` provides `Enable(bool, handler)` and `Remove(handler)`.
- Installed runtime on the development machine: 154.0.4258.62 (EdgeUpdate `pv`). Bowser still
  queries each interface at runtime and degrades if one is missing.

## Limits, stated plainly

- WebView2 supports a subset of Chrome extension APIs. Each extension is only as compatible as the
  APIs it uses.
- Only unpacked folders chosen by the user are supported. There is no Chrome Web Store install, and
  Bowser makes no Web Store compatibility claim.
- Bowser validates manifests itself (manifest version 2 or 3, name, version, permission lists). It
  is stricter than Chrome: non-text permission entries are refused. Declared permissions are shown
  to the user before install. Bowser does not enforce them; WebView2 does what it does.
- Enabling extensions is decided when the window's environment is created. A profile with no
  installed extension is created without the option. Installing the first extension in a running
  window reports "restart Bowser" when the option is absent.

## Verification status

- Unit tests cover manifest parsing (malicious and malformed input), size and depth caps, link and
  junction refusal, id and path containment, the records file, and removal.
- Not verified end to end: no extension was actually loaded into a live window during this work.
  The release exe was only started briefly to confirm it stays running.

## Navigation policy

The `ms-browser-extension` scheme stays blocked for user-initiated navigations. Loading an
unpacked extension does not require opening that scheme from the address bar.

## Where the code lives

- `src/extensions.rs`: validation, records file, install and remove, load-result store.
- `src/engine/webview2.rs`: environment option, startup loading, runtime enable and remove.
- `src/win32/window.rs`: Ctrl+Shift+E menu, folder picker, install confirmation.
