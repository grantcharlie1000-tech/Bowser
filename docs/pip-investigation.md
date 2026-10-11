# Picture-in-picture investigation

## Findings (11 October 2026)

- Bowser pins the Rust COM bindings to `webview2-com = 0.39.1`. The bindings expose `ICoreWebView2::ExecuteScript` and `ICoreWebView2::CallDevToolsProtocolMethod`; the matching WebView2 contracts include `NewWindowRequested` and `AreDevToolsEnabled`. This is API availability, not a guarantee that the installed runtime implements a web platform feature.
- The installed Evergreen WebView2 Runtime is `154.0.4258.62` (Edge is `155.0.4283.45`). Runtime and SDK versions are separate; the app follows Evergreen updates.
- The standard video PiP API is Chromium's `HTMLVideoElement.requestPictureInPicture()`. It requires transient user activation; executing that call using WebView2 `ExecuteScript` does not document or supply such activation. Chromium's CDP `Runtime.evaluate` does have a `userGesture` parameter, and WebView2 exposes the asynchronous CDP call. That is the lightest host-side path to try. The command is generated only for the browser's explicit user action and exposes no host object or web-message bridge to the page.
- Microsoft's WebView2 API documentation describes `NewWindowRequested` for content `window.open()` requests, but does not specify that native video PiP uses it or that WebView2 hosts the floating PiP window. PiP is Chromium/browser-embedder behavior, so whether it creates a movable, always-on-top OS window in this host must be established against the installed runtime. Bowser's existing popup handler only accepts user-initiated HTTP(S) destinations into a tab and blocks other popup requests; a PiP implementation must not weaken that general popup policy.
- Document PiP is a separate Chromium API for arbitrary page UI. Bowser only needs the browser-owned video PiP path; a second WebView and custom video-decoding path would add unnecessary resource and security cost.

## Decision

Try native video PiP with `Runtime.evaluate` and `userGesture: true`, after lazily selecting an eligible main-frame `<video>`. Treat API rejection or unsupported runtimes as unavailable; do not create a custom floating WebView. During an active PiP session, temporary `pause`, `ended`, and `leavepictureinpicture` listeners close PiP on stop/end and abort their listeners on exit. The implementation still needs live verification that PiP opens and survives tab switching, and that Chromium provides the requested window controls and lifecycle behavior. If that host behavior is absent, the feature should remain disabled rather than emulate PiP.

## References

- [WebView2 API overview](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/overview-features-apis)
- [WebView2 `ICoreWebView2` reference](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2)
- [WebView2 new-window event arguments](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2newwindowrequestedeventargs)
- [Chrome DevTools Protocol Runtime domain](https://chromedevtools.github.io/devtools-protocol/tot/Runtime/)
- [Picture-in-Picture API specification](https://www.w3.org/TR/picture-in-picture/)
