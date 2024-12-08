use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::{Duration, Instant},
};

use sdl2::pixels::Color;

/*
⠀⠀⣄⠀⠀
⠠⢴⣿⡦⠄
⠉⣽⣿⣏⠉
⠀⠀⣿⠀⠀
*/

const WPM_SATURATION: f64 = 70.0;
const WORST_PACKET_DROP: u32 = 2 * (u32::MAX / 5); // 40% drop rate at 0 WPM

pub fn wpm_to_drop_amt(wpm: f64) -> u32 {
    let clipped_wpm = wpm.min(WPM_SATURATION);

    let drop_frac = (WPM_SATURATION - clipped_wpm) / WPM_SATURATION;
    // exponentiate drop_fac to make packet drop rate even more apparent
    ((WORST_PACKET_DROP as f64) * f64::powi(drop_frac, 2)) as u32
}

pub fn wpm_to_sdl_color(wpm: f64, base_color: Color) -> Color {
    let clipped_wpm = wpm.min(WPM_SATURATION);

    let wpm_frac = clipped_wpm / WPM_SATURATION;
    let wpm_color = Color::RGB(
        (base_color.r as f64 * wpm_frac) as u8,
        (base_color.g as f64 * wpm_frac) as u8,
        (base_color.b as f64 * wpm_frac) as u8,
    );

    wpm_color
}

const WORST_JPEG_QUALITY: f64 = 1.0;
const BEST_JPEG_QUALITY: f64 = 0.03;

pub fn wpm_to_jpeg_quality(wpm: f64) -> f64 {
    let clipped_wpm = wpm.min(WPM_SATURATION);
    
    let wpm_ratio = (WPM_SATURATION - clipped_wpm) / WPM_SATURATION;
    // flip wpm_ratio so that higher WPMs result in higher quality
    WORST_JPEG_QUALITY - (WORST_JPEG_QUALITY - BEST_JPEG_QUALITY) * f64::powi(1.0 - wpm_ratio, 1)
}

pub const CHART_DATA_LENGTH: usize = 1000;

#[derive(Debug)]
pub struct TypingMetrics {
    stroke_window: VecDeque<(i32, Instant)>,
    repeated_keys: BTreeMap<i32, u32>,
}

impl TypingMetrics {
    pub fn new() -> Self {
        Self {
            stroke_window: VecDeque::new(),
            repeated_keys: BTreeMap::new(),
        }
    }

    fn update_stroke_window(&mut self) {
        let now = Instant::now();
        let window_duration = Duration::from_secs(3);
        while self
            .stroke_window
            .front()
            .map_or(false, |(_, t)| now.duration_since(*t) > window_duration)
        {
            self.stroke_window.pop_front();
        }
    }

    /// Calculate the WPM based on the stored stroke window.
    fn calculate_wpm(&mut self) -> f64 {
        // penalize repeated keys
        let mut wpm = 0.0;
        self.repeated_keys.clear();

        for (c, _) in &self.stroke_window {
            wpm += 1.0;
            // repeated keys get higher penalties for each repetition
            if let Some(times) = self.repeated_keys.get(c) {
                wpm -= (0.04 * (*times as f64)).max(0.0);
                self.repeated_keys.insert(*c, times + 1);
            } else {
                self.repeated_keys.insert(*c, 1);
            }
        }
        wpm
    }

    /// Calculate the WPM in the current instant based on the current stroke window.
    pub fn calc_wpm(&mut self) -> f64 {
        self.update_stroke_window();
        self.calculate_wpm()
    }

    pub fn receive_char_stroke(&mut self, c: i32) {
        self.stroke_window.push_back((c, Instant::now()));
    }

    /// The given timestamp must be more recent than previously supplied timestamps.
    pub fn receive_char_stroke_with_timestamp(&mut self, c: i32, timestamp: Instant) {
        self.stroke_window.push_back((c, timestamp));
    }
}
