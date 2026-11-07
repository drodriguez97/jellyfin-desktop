//! A plain Win32 child window that fills the Tauri window's client area and
//! sits at the bottom of the z-order. mpv renders into it (via `wid`) with its
//! own D3D11 flip-model swapchain, while the transparent WebView2 child draws
//! on top.
//!
//! The host must NOT have `WS_CLIPSIBLINGS`: with it, DWM clips the video
//! swapchain to the region not covered by the (full-size) webview sibling and
//! the video never shows. Without it, the transparent webview composes over
//! the video exactly like it composes over the parent window's own content.

use anyhow::{anyhow, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{GetStockObject, BLACK_BRUSH};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, RegisterClassW, SetWindowPos, HWND_BOTTOM,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WNDCLASSW, WS_CHILD, WS_VISIBLE,
};

static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);

fn class_name() -> Vec<u16> {
    "JellyfinDesktopVideoHost\0".encode_utf16().collect()
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Creates the child window inside `parent` and pushes it to the bottom of
/// the z-order so the webview (created earlier by Tauri) stays on top.
pub fn create(parent: HWND) -> Result<HWND> {
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let name = class_name();
        if !CLASS_REGISTERED.swap(true, Ordering::SeqCst) {
            let wc = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinstance,
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: GetStockObject(BLACK_BRUSH),
                lpszMenuName: std::ptr::null(),
                lpszClassName: name.as_ptr(),
            };
            if RegisterClassW(&wc) == 0 {
                return Err(anyhow!("RegisterClassW failed"));
            }
        }

        let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetClientRect(parent, &mut rc);

        let hwnd = CreateWindowExW(
            0,
            name.as_ptr(),
            std::ptr::null(),
            WS_CHILD | WS_VISIBLE,
            0,
            0,
            rc.right - rc.left,
            rc.bottom - rc.top,
            parent,
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return Err(anyhow!("CreateWindowExW failed"));
        }
        SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        Ok(hwnd)
    }
}

/// Asks DWM for the dark title bar so the frame matches the app. Windows 11
/// also applies its rounded corners; nothing is drawn by us, so no cost.
pub fn apply_dark_title_bar(hwnd: HWND) {
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
    let dark: i32 = 1;
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            &dark as *const i32 as *const _,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

/// Resizes the child to the parent's current client area, keeping it at the bottom.
pub fn resize_to_parent(hwnd: HWND, parent: HWND) {
    unsafe {
        let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetClientRect(parent, &mut rc);
        SetWindowPos(
            hwnd,
            HWND_BOTTOM,
            0,
            0,
            rc.right - rc.left,
            rc.bottom - rc.top,
            SWP_NOACTIVATE,
        );
    }
}
