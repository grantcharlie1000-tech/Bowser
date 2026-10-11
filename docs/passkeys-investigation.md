# Passkeys in Bowser

## Checked environment

- Bowser's `webview2-com` dependency is pinned to **0.39.1**. Its checked-in generated COM bindings expose WebView2 interfaces and frame messaging, but no WebAuthn-specific `ICoreWebView2` interface, flag, or passkey management API.
- `WebView2Engine::availability()` queried the machine's active Evergreen runtime as **154.0.4258.62** on 2026-10-11.
- Bowser has not run a WebAuthn registration (`navigator.credentials.create`) or assertion (`navigator.credentials.get`) against this runtime. Embedded-host create/get therefore remain **unverified**.

## What the platform and host expose

Microsoft documents Windows WebAuthn APIs for Windows Hello and FIDO2 authenticators. Windows 11 24H2 also supports plugin passkey managers. That documentation names Microsoft Edge as a WebAuthn client, but does not establish that every embedded WebView2 host/runtime combination exposes the same user experience. The WebView2 0.39.1 COM surface in this repo has no host-side WebAuthn control API; any web ceremony would have to be implemented by the web runtime and Windows authenticator stack.

There is a recent, still-open report in Microsoft's WebView2 feedback repo describing `navigator.credentials.create()` / `get()` failing to show Windows Hello in a WebView2 app, while working in Edge and Chrome. The report names runtime 148.0.3967.70 and WPF. It is a report about one environment, not a confirmed limitation for Bowser's runtime 154.0.4258.62, so it is a reason to keep support unverified rather than claim a universal blocker.

If a ceremony succeeds, the authenticator/provider owns the passkey. Windows documentation says the authenticator creates and stores the key locally; Windows Hello is a platform authenticator, while a roaming security key or passkey manager can own it elsewhere. Bowser does not store passkeys in `Passwords.dat` and cannot enumerate their provider or storage location.

## Import and export

WebAuthn credentials are scoped to a relying party and managed by an authenticator. The WebAuthn specification says the credential private key is bound to its managing authenticator and is expected never to be exposed to another party, including the authenticator's owner. Bowser cannot export or import that private key. Moving a passkey depends on the platform/provider's own sync or recovery features; otherwise register a replacement with the site before removing the old credential.

The Passwords & Passkeys panel shows only a read-only status notice. It does not offer passkey import/export controls.

## References

- [Windows WebAuthn APIs](https://learn.microsoft.com/en-us/windows/security/identity-protection/hello-for-business/webauthn-apis)
- [Windows passkeys](https://learn.microsoft.com/en-us/windows/security/identity-protection/passkeys/)
- [WebView2 frames and message APIs](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/frames)
- [WebAuthn Level 3](https://www.w3.org/TR/webauthn-3/)
- [WebView2 feedback issue #5663](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5663)
