# Documented native engine-item context-menu fixture

Keyed disposable MV3 fixture for the separate Windows-only custom presentation probe.
It requests contextMenus, scripting, activeTab and storage, with no host permissions,
tabs permission or content scripts. The no-activeTab lane removes only activeTab from
a copied source before install.

The top-level onInstalled listener requires the real contextMenus.create completion.
The genuine contextMenus.onClicked listener alone starts primary injection, using only
its native tab ID. Native executeScript results and distinct primary/negative markers
allow exact owned-view attribution. Messages only observe receipts or perform explicit
negative preflight/retained-event-ID retry operations. Nothing dispatches an onClicked
event or invokes a host permission-grant API.

This is not Bitwarden, a real-vault test, cold-worker proof or production routing.
The original observation fixture remains unchanged.
