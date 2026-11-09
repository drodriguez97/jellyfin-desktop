//! Display refresh-rate switching (port of `DisplayManagerWin` plus the
//! matching logic of `DisplayManager`/`DisplayComponent`). When
//! `video.refreshrate.auto_switch` is on and the app is fullscreen, the
//! monitor the window is on is switched to the mode that best matches the
//! media frame rate before playback starts, and restored when playback ends.

pub use crate::display_match::VideoMode;
use crate::display_match::{find_best_match as best_match, nominal_refresh_rate, wide_to_string};
use parking_lot::Mutex;
use std::sync::OnceLock;
use tracing::{info, warn};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    ChangeDisplaySettingsExW, EnumDisplayDevicesW, EnumDisplaySettingsExW, EnumDisplaySettingsW, CDS_FULLSCREEN,
    DEVMODEW, DISPLAY_DEVICEW, DISPLAY_DEVICE_ACTIVE, DISPLAY_DEVICE_ATTACHED, DISP_CHANGE_SUCCESSFUL,
    DM_DISPLAYFLAGS, DM_DISPLAYFREQUENCY, DM_INTERLACED, DM_PELSHEIGHT, DM_PELSWIDTH, ENUM_CURRENT_SETTINGS,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;

impl VideoMode {
    fn from_devmode(id: usize, dm: &DEVMODEW) -> Self {
        let flags = unsafe { dm.Anonymous2.dmDisplayFlags };
        VideoMode {
            id,
            width: dm.dmPelsWidth,
            height: dm.dmPelsHeight,
            refresh_rate: nominal_refresh_rate(dm.dmDisplayFrequency),
            bits_per_pixel: dm.dmBitsPerPel,
            interlaced: flags & DM_INTERLACED != 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Display {
    pub adapter: Vec<u16>, // device name as NUL-terminated UTF-16
    pub name: String,
    pub modes: Vec<VideoMode>,
}

fn devmode() -> DEVMODEW {
    let mut dm: DEVMODEW = unsafe { std::mem::zeroed() };
    dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
    dm
}

pub fn enumerate() -> Vec<Display> {
    let mut out = Vec::new();
    let mut idx = 0u32;
    loop {
        let mut info: DISPLAY_DEVICEW = unsafe { std::mem::zeroed() };
        info.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
        if unsafe { EnumDisplayDevicesW(std::ptr::null(), idx, &mut info, 0) } == 0 {
            break;
        }
        idx += 1;
        if info.StateFlags & (DISPLAY_DEVICE_ACTIVE | DISPLAY_DEVICE_ATTACHED) == 0 {
            continue;
        }
        let adapter: Vec<u16> = info.DeviceName.to_vec();
        let mut modes = Vec::new();
        let mut mode_idx = 0u32;
        loop {
            let mut dm = devmode();
            if unsafe { EnumDisplaySettingsExW(adapter.as_ptr(), mode_idx, &mut dm, 0) } == 0 {
                break;
            }
            modes.push(VideoMode::from_devmode(mode_idx as usize, &dm));
            mode_idx += 1;
        }
        out.push(Display { adapter, name: wide_to_string(&info.DeviceString), modes });
    }
    out
}

fn current_devmode(adapter: &[u16]) -> Option<DEVMODEW> {
    let mut dm = devmode();
    if unsafe { EnumDisplaySettingsW(adapter.as_ptr(), ENUM_CURRENT_SETTINGS, &mut dm) } == 0 {
        None
    } else {
        Some(dm)
    }
}

pub fn current_mode(disp: &Display) -> Option<usize> {
    let dm = current_devmode(&disp.adapter)?;
    let now = VideoMode::from_devmode(usize::MAX, &dm);
    disp.modes.iter().position(|m| m.same_as(&now))
}

/// Index of the disp whose desktop rectangle contains the window's center.
pub fn display_for_window(displays: &[Display], hwnd: HWND) -> Option<usize> {
    let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetWindowRect(hwnd, &mut rc) };
    let (cx, cy) = ((rc.left + rc.right) / 2, (rc.top + rc.bottom) / 2);
    displays.iter().position(|d| {
        current_devmode(&d.adapter)
            .map(|dm| {
                let pos = unsafe { dm.Anonymous1.Anonymous2.dmPosition };
                cx >= pos.x
                    && cx < pos.x + dm.dmPelsWidth as i32
                    && cy >= pos.y
                    && cy < pos.y + dm.dmPelsHeight as i32
            })
            .unwrap_or(false)
    })
}

/// Port of `DisplayManager::findBestMatch` for the display's current mode.
pub fn find_best_match(disp: &Display, frame_rate: f32, avoid_25_30: bool) -> Option<usize> {
    let current = disp.modes.get(current_mode(disp)?)?;
    best_match(&disp.modes, current, frame_rate, avoid_25_30)
}

pub fn set_mode(disp: &Display, mode: usize) -> bool {
    let mut dm = devmode();
    if unsafe { EnumDisplaySettingsExW(disp.adapter.as_ptr(), mode as u32, &mut dm, 0) } == 0 {
        return false;
    }
    info!("Switching to mode {} on {}: {}", mode, disp.name, disp.modes[mode].pretty());
    dm.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY | DM_DISPLAYFLAGS;
    let rc = unsafe {
        ChangeDisplaySettingsExW(disp.adapter.as_ptr(), &dm, std::ptr::null_mut(), CDS_FULLSCREEN, std::ptr::null())
    };
    if rc != DISP_CHANGE_SUCCESSFUL {
        warn!("ChangeDisplaySettingsExW failed: {rc}");
        return false;
    }
    true
}

/// Remembers the mode that was active before the first switch so it can be restored.
#[derive(Default)]
struct Saved {
    disp: Option<Display>,
    mode: Option<usize>,
}

static SAVED: OnceLock<Mutex<Saved>> = OnceLock::new();

fn saved() -> &'static Mutex<Saved> {
    SAVED.get_or_init(|| Mutex::new(Saved::default()))
}

/// Refresh rate of the disp the window is on (0 when unknown).
pub fn current_refresh_rate(hwnd: HWND) -> f64 {
    let displays = enumerate();
    let Some(idx) = display_for_window(&displays, hwnd) else { return 0.0 };
    match current_mode(&displays[idx]) {
        Some(m) => displays[idx].modes[m].refresh_rate as f64,
        None => 0.0,
    }
}

/// Switches to the best mode for `frame_rate`. Returns true if a switch happened.
pub fn switch_to_best_mode(hwnd: HWND, frame_rate: f32, avoid_25_30: bool) -> bool {
    let displays = enumerate();
    let Some(idx) = display_for_window(&displays, hwnd) else {
        info!("Not switching rate - current disp not found.");
        return false;
    };
    let disp = &displays[idx];
    let Some(current) = current_mode(disp) else { return false };
    {
        let mut s = saved().lock();
        if s.mode.is_none() {
            s.disp = Some(disp.clone());
            s.mode = Some(current);
        }
    }
    match find_best_match(disp, frame_rate, avoid_25_30) {
        Some(best) if best != current => set_mode(disp, best),
        Some(_) => {
            info!("No better video mode than the currently active one found.");
            false
        }
        None => false,
    }
}

/// Restores the mode saved by the first `switch_to_best_mode`.
pub fn restore_previous_mode() -> bool {
    let (disp, mode) = {
        let mut s = saved().lock();
        match (s.disp.take(), s.mode.take()) {
            (Some(d), Some(m)) => (d, m),
            _ => return false,
        }
    };
    if current_mode(&disp) == Some(mode) {
        return true;
    }
    info!("Restoring video mode to {}", disp.modes[mode].pretty());
    set_mode(&disp, mode)
}
