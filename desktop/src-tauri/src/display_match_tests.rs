use super::*;

fn mode(id: usize, w: u32, h: u32, rate: f32, interlaced: bool) -> VideoMode {
    VideoMode { id, width: w, height: h, refresh_rate: rate, bits_per_pixel: 32, interlaced }
}

fn uhd_modes() -> Vec<VideoMode> {
    vec![
        mode(0, 3840, 2160, 23.976, false),
        mode(1, 3840, 2160, 24.0, false),
        mode(2, 3840, 2160, 25.0, false),
        mode(3, 3840, 2160, 29.97, false),
        mode(4, 3840, 2160, 30.0, false),
        mode(5, 3840, 2160, 50.0, false),
        mode(6, 3840, 2160, 59.94, false),
        mode(7, 3840, 2160, 60.0, false),
        mode(8, 1920, 1080, 24.0, false),
        mode(9, 3840, 2160, 60.0, true),
    ]
}

#[test]
fn nominal_rates_recover_ntsc_fractions() {
    assert!((nominal_refresh_rate(59) - 59.94).abs() < 0.01);
    assert!((nominal_refresh_rate(29) - 29.97).abs() < 0.01);
    assert!((nominal_refresh_rate(23) - 23.976).abs() < 0.01);
    assert_eq!(nominal_refresh_rate(60), 60.0);
    assert_eq!(nominal_refresh_rate(0), 0.0);
}

#[test]
fn wide_strings() {
    let w: Vec<u16> = "PIONEER\0garbage".encode_utf16().collect();
    assert_eq!(wide_to_string(&w), "PIONEER");
    let no_nul: Vec<u16> = "abc".encode_utf16().collect();
    assert_eq!(wide_to_string(&no_nul), "abc");
}

#[test]
fn mode_identity_and_pretty() {
    let a = mode(1, 3840, 2160, 23.976, false);
    let mut b = a.clone();
    b.id = 99;
    assert!(a.same_as(&b));
    b.interlaced = true;
    assert!(!a.same_as(&b));
    assert_eq!(a.pretty(), "3840x2160p @ 23.976Hz 32bpp");
    assert_eq!(mode(2, 1920, 1080, 50.0, true).pretty(), "1920x1080i @ 50.000Hz 32bpp");
}

#[test]
fn rate_multiples() {
    assert!(is_rate_multiple_of(23.976, 47.952, true));
    assert!(is_rate_multiple_of(24.0, 48.0, true));
    assert!(is_rate_multiple_of(29.97, 59.94, true));
    assert!(!is_rate_multiple_of(24.0, 60.0, true));
    assert!(is_rate_multiple_of(30.0, 60.5, false));
    assert!(!is_rate_multiple_of(0.0, 60.0, false));
    assert!(!is_rate_multiple_of(60.0, 30.0, false));
}

#[test]
fn film_picks_the_matching_rate_at_current_resolution() {
    let modes = uhd_modes();
    let current = &modes[7]; // 2160p60
    assert_eq!(find_best_match(&modes, current, 23.976, false), Some(0));
    assert_eq!(find_best_match(&modes, current, 24.0, false), Some(1));
    assert_eq!(find_best_match(&modes, current, 25.0, false), Some(2));
    assert_eq!(find_best_match(&modes, current, 29.97, false), Some(3));
    // 60 fps content keeps the current mode
    assert_eq!(find_best_match(&modes, current, 60.0, false), Some(7));
}

#[test]
fn avoiding_25_and_30_falls_back_to_multiples() {
    let modes = uhd_modes();
    let current = &modes[7];
    assert_eq!(find_best_match(&modes, current, 25.0, true), Some(5));
    assert_eq!(find_best_match(&modes, current, 29.97, true), Some(6));
    assert_eq!(find_best_match(&modes, current, 30.0, true), Some(7));
}

#[test]
fn progressive_beats_interlaced_and_resolution_wins() {
    let modes = uhd_modes();
    let current = &modes[7];
    // 60 fps: the progressive 60 Hz mode wins over the interlaced one.
    assert_eq!(find_best_match(&modes, current, 60.0, false), Some(7));
    // From 1080p24, only the 1080p24 mode keeps the resolution.
    let from_1080 = find_best_match(&modes, &modes[8], 24.0, false);
    assert_eq!(from_1080, Some(8));
}

#[test]
fn no_suitable_mode() {
    // Only one mode and the frame rate has nothing to do with it.
    let modes = vec![mode(0, 1280, 720, 60.0, false)];
    assert_eq!(find_best_match(&modes, &modes[0], 24.0, false), Some(0)); // current + res + progressive > 1000
    let other = mode(5, 1920, 1080, 60.0, false);
    // nothing matches the current resolution and the rate does not fit
    assert_eq!(find_best_match(&modes, &other, 24.0, false), None);
    assert_eq!(find_best_match(&[], &other, 24.0, false), None);
}
