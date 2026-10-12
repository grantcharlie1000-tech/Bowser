# Side panel host

## Lifecycle

`win32::side_panel` owns one child host window for each Bowser top-level window.
The host is created lazily on the first successful `open` and remains alive while
the selected panel changes. Closing clears the active content and animates the
host width to zero; the host window itself is retained for the next open.
The animation is a 140 ms ease-out. Its owner-window timer runs only during that
transition and stops after the final width is applied.

The host window owns its `SidePanelHost` state and the GDI brush used to paint
its frosted surface. It does not own the top-level Bowser window. Registered
content is a boxed trait object created by its factory when selected. Content
that creates a native EDIT or another child control owns that HWND and should
parent it to the panel host; the host positions the HWND returned by
`wants_native_child` beneath the title bar.

The title bar and close affordance are host-painted. Mouse, wheel, character,
key, focus, and paint callbacks are forwarded to the active `PanelContent`.
Escape closes the panel and restores the previously focused HWND. Native child controls keep
their standard Win32 text editing behavior. The host remains a child of the
Bowser client area, so moving or destroying its owner follows normal Win32
parent-child lifetime rules.

## Geometry and settings

The panel begins at `chrome::layout::left_rail_inset(dpi)` and spans the same
top and bottom as the page viewport. Its animated pixel width advances the
viewport's left edge, so WebView2 receives a smaller, undistorted rectangle.
Split-pane bounds use that same adjusted rectangle. The notes panel remains a
separate right-docked window and can reserve its own width at the same time.

`side_panel_width` is stored in the profile's `settings.txt` as a logical
96-DPI width. It is clamped to 280–520 logical pixels. At the current DPI, the
host also limits the physical width to about 45% of the available window and
preserves at least 320 logical pixels for the page. If no width satisfies both
limits, opening is refused and `last_status()` explains why.

## Adding a panel

1. Choose a stable `PanelId` (use `WebApp(id)` for a registered web app).
2. Implement `PanelContent`, including its title and preferred/minimum widths.
3. Create and destroy native child controls with the content object. Parent
   them to the host and return the active child from `wants_native_child`.
4. Register a factory once with `register_content(id, || Box::new(Content::new()))`.
   Factories should construct content lazily and must run on the UI thread.
5. Wire the rail command through `window::rail_activate(hwnd, app, id)`. It
   toggles the selected panel and invokes the viewport resize path.
6. Use `PanelContent::paint` for custom-painted rows, and handle pointer or
   keyboard input through the corresponding callbacks.

The main window registers Bookmarks, History, Customize, and WebApp content at
startup. The Menu rail item opens the Customize panel's rail section.

WIRING-STATUS: app wrappers done
