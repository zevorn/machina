// Single LED device.
//
// Models a physical LED with configurable color and GPIO
// input. GPIO active-high polarity controls behavior:
// when GPIO is active, the LED is at 100% intensity;
// otherwise 0%.

use std::sync::Mutex;

const INTENSITY_MAX: u8 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedColor {
    Violet,
    Blue,
    Cyan,
    Green,
    Yellow,
    Amber,
    Orange,
    Red,
}

impl LedColor {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Violet => "violet",
            Self::Blue => "blue",
            Self::Cyan => "cyan",
            Self::Green => "green",
            Self::Yellow => "yellow",
            Self::Amber => "amber",
            Self::Orange => "orange",
            Self::Red => "red",
        }
    }
}

pub struct Led {
    intensity: Mutex<u8>,
    gpio_active_high: bool,
    color: LedColor,
    description: String,
}

impl Led {
    pub fn new(
        color: LedColor,
        description: &str,
        gpio_active_high: bool,
    ) -> Self {
        let initial = if gpio_active_high { INTENSITY_MAX } else { 0 };
        Self {
            intensity: Mutex::new(initial),
            gpio_active_high,
            color,
            description: description.to_string(),
        }
    }

    /// Set intensity as a percentage (0–100).
    pub fn set_intensity(&self, percent: u8) {
        *self.intensity.lock().unwrap() = percent.min(INTENSITY_MAX);
    }

    /// Get current intensity as a percentage.
    pub fn get_intensity(&self) -> u8 {
        *self.intensity.lock().unwrap()
    }

    /// Set the LED emitting state (used with GPIO input).
    pub fn set_state(&self, is_emitting: bool) {
        let level = is_emitting == self.gpio_active_high;
        self.set_intensity(if level { INTENSITY_MAX } else { 0 });
    }

    pub fn color(&self) -> LedColor {
        self.color
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn gpio_active_high(&self) -> bool {
        self.gpio_active_high
    }
}
