#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Transparent WebView2 background (AARRGGBB). Set before the webview is
    // created so the first frame is already transparent (no white flash).
    std::env::set_var("WEBVIEW2_DEFAULT_BACKGROUND_COLOR", "00000000");
    jellyfin_desktop_lib::run()
}
