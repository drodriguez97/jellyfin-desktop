#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Transparent WebView2 background (AARRGGBB). Set before the webview is
    // created so the first frame is already transparent (no white flash).
    // JFD_OPAQUE_WEBVIEW=1 keeps the webview opaque (video stays hidden); only
    // useful to measure what the transparent background costs.
    if std::env::var_os("JFD_OPAQUE_WEBVIEW").is_none() {
        std::env::set_var("WEBVIEW2_DEFAULT_BACKGROUND_COLOR", "00000000");
    }
    jellyfin_desktop_lib::run()
}
