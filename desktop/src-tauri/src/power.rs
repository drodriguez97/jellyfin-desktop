//! Keeps the display awake while video plays (port of `PowerComponentWin`).
//! `SetThreadExecutionState` is per-thread, so this must be called from a
//! thread that outlives playback; the mpv event thread is used for that.

use windows_sys::Win32::System::Power::{
    SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
};

pub fn set_display_required(required: bool) {
    unsafe {
        if required {
            SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED);
        } else {
            SetThreadExecutionState(ES_CONTINUOUS);
        }
    }
}
