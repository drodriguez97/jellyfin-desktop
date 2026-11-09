//! Refresh-rate matching (port of `DisplayManager::findBestMatch`): given the
//! modes a monitor offers and the one it is in, pick the mode that best fits a
//! video frame rate. No Win32 here; `display.rs` enumerates the modes.

use tracing::{debug, info};

const WEIGHT_RES: f32 = 1000.0;
const WEIGHT_RATE_EXACT: f32 = 200.0;
const WEIGHT_RATE_MULTIPLE: f32 = 75.0;
const WEIGHT_RATE_CLOSE: f32 = 50.0;
const WEIGHT_RATE_MULTIPLE_CLOSE: f32 = 25.0;
const WEIGHT_INTERLACE: f32 = 10.0;
const WEIGHT_CURRENT: f32 = 5.0;

#[derive(Debug, Clone, PartialEq)]
pub struct VideoMode {
    pub id: usize,
    pub width: u32,
    pub height: u32,
    pub refresh_rate: f32,
    pub bits_per_pixel: u32,
    pub interlaced: bool,
}

impl VideoMode {
    pub fn same_as(&self, o: &VideoMode) -> bool {
        self.width == o.width
            && self.height == o.height
            && (self.refresh_rate - o.refresh_rate).abs() < 1e-6
            && self.bits_per_pixel == o.bits_per_pixel
            && self.interlaced == o.interlaced
    }
    pub fn pretty(&self) -> String {
        format!(
            "{}x{}{} @ {:.3}Hz {}bpp",
            self.width,
            self.height,
            if self.interlaced { "i" } else { "p" },
            self.refresh_rate,
            self.bits_per_pixel
        )
    }
}

/// Windows reports integer refresh rates; recover the fractional NTSC ones.
pub fn nominal_refresh_rate(reported: u32) -> f32 {
    match reported {
        59 | 29 | 23 => (reported as f32 + 1.0) / 1.001,
        other => other as f32,
    }
}

/// NUL-terminated UTF-16 to String.
pub fn wide_to_string(w: &[u16]) -> String {
    let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end])
}

/// Is `multiple` an integer multiple of `refresh` (within tolerance)?
pub fn is_rate_multiple_of(refresh: f32, multiple: f32, exact: bool) -> bool {
    let rounded_refresh = refresh.round() as i64;
    let rounded_multiple = multiple.round() as i64;
    if rounded_refresh == 0 {
        return false;
    }
    let factor = rounded_multiple / rounded_refresh;
    let new_rate = factor as f32 * refresh;
    if new_rate < 1.0 {
        return false;
    }
    let tolerance = if exact { 0.01 * factor as f32 } else { 1.0 };
    (new_rate - multiple).abs() < tolerance
}

/// The id of the mode that best matches `frame_rate`, keeping the current
/// resolution and depth; None when no mode scores above "same resolution".
pub fn find_best_match(modes: &[VideoMode], current: &VideoMode, frame_rate: f32, avoid_25_30: bool) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for c in modes {
        if avoid_25_30 && ((c.refresh_rate - 30.0).abs() < 0.5 || (c.refresh_rate - 25.0).abs() < 0.5) {
            continue;
        }
        let mut w = 0.0;
        if c.width == current.width && c.height == current.height && c.bits_per_pixel == current.bits_per_pixel {
            w += WEIGHT_RES;
        }
        if (c.refresh_rate - frame_rate).abs() <= 0.01 {
            w += WEIGHT_RATE_EXACT;
        }
        if is_rate_multiple_of(frame_rate, c.refresh_rate, true) {
            w += WEIGHT_RATE_MULTIPLE;
        }
        if (c.refresh_rate - frame_rate).abs() <= 0.5 {
            w += WEIGHT_RATE_CLOSE;
        }
        if is_rate_multiple_of(frame_rate, c.refresh_rate, false) {
            w += WEIGHT_RATE_MULTIPLE_CLOSE;
        }
        if !c.interlaced {
            w += WEIGHT_INTERLACE;
        }
        if c.id == current.id {
            w += WEIGHT_CURRENT;
        }
        debug!("mode {} ({}) weight {w}", c.id, c.pretty());
        if best.map(|(_, bw)| w > bw).unwrap_or(true) {
            best = Some((c.id, w));
        }
    }
    match best {
        Some((id, w)) if w > WEIGHT_RES => {
            if let Some(m) = modes.iter().find(|m| m.id == id) {
                info!("RefreshMatch: found a suitable mode: {}", m.pretty());
            }
            Some(id)
        }
        _ => {
            info!("RefreshMatch: found no suitable video mode");
            None
        }
    }
}

#[cfg(test)]
#[path = "display_match_tests.rs"]
mod tests;
