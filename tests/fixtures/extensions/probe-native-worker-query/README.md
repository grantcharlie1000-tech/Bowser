# Native worker-current-window prerequisite fixture

A fresh disposable keyed MV3 fixture, not a Chrome API shim or a signed marketplace package.
It requests activeTab/scripting/storage only, no host permission or content scripts. The public
key is the existing development fixture's public key, not signature verification.

Ctrl+Shift+X is a named, non-global command. The test verifies native commands.getAll assignment,
real focused OS key delivery, and the actual production accelerator decision before interpreting
native onCommand or its absence. Ctrl+Shift+L/Y are deliberately not used because Bowser owns them.
Before any event-handler injection, the genuine worker queries active:true with native WINDOW_ID_CURRENT.
Only its unique returned tab ID is a positive injection target. The event tab is comparison evidence,
never an injection fallback. Negative pre-gesture IDs come
from native tabs.query, never URL/title-based Bowser identity mapping. Separate COM-owned views
establish the actual effect/other-tab boundary. A fresh copy without activeTab is a negative control;
a different-origin document plus retained native ID is the revocation control, without a new gesture.

A diagnostic reports unsupported/unresolved routes honestly; a green diagnostic is not a product
compatibility pass. No account, real credential, debug port or browser-target CDP is involved.

Scope: warm worker, hosted observer popup present, unbound Ctrl+Shift+X. Not actual Bitwarden
autofill, Ctrl+Shift+L conflict resolution, closed-popup behavior, or cold-worker wake.
