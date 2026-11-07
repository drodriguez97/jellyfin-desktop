// Keyboard/mouse shortcuts the Qt shell handled natively (QML Actions and
// EventFilter): F11 fullscreen, Ctrl+Q quit, Ctrl+0 reset zoom, and the mouse
// back/forward buttons for history navigation.
(function () {
    const invoke = (cmd, args) => window.api && window.__TAURI__ && window.__TAURI__.core.invoke(cmd, args || {});

    window.addEventListener('keydown', (e) => {
        if (e.key === 'F11') {
            e.preventDefault();
            invoke('window_toggle_fullscreen');
        } else if (e.key === 'q' && e.ctrlKey && !e.shiftKey && !e.altKey) {
            e.preventDefault();
            invoke('system_exit');
        } else if (e.key === 'Escape' && e.ctrlKey) {
            // Ctrl+Esc isn't deliverable here; nothing to do.
        }
    }, true);

    window.addEventListener('mouseup', (e) => {
        if (e.button === 3) { e.preventDefault(); history.back(); }
        else if (e.button === 4) { e.preventDefault(); history.forward(); }
    }, true);
    window.addEventListener('mousedown', (e) => {
        if (e.button === 3 || e.button === 4) { e.preventDefault(); }
    }, true);

    // Block drag-and-drop navigation (dropping a file would unload the client).
    window.addEventListener('dragover', (e) => e.preventDefault(), true);
    window.addEventListener('drop', (e) => e.preventDefault(), true);
})();
