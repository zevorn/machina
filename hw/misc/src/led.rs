// Single LED device.
//
// Models a physical LED with configurable color and GPIO
// input. GPIO active-high polarity controls behavior:
// when GPIO is active, the LED is at 100% intensity;
// otherwise 0%. reset_runtime restores the initial
// intensity derived from gpio_active_high.

use std::sync::Arc;

use machina_hw_core::mdev::MDeviceState;

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

#[derive(machina_hw_core::MDevice)]
#[mom(state = state, lock = "parking_lot")]
pub struct Led {
    state: parking_lot::Mutex<MDeviceState>,
    intensity: parking_lot::Mutex<u8>,
    gpio_active_high: bool,
    color: LedColor,
    description: String,
}

impl Led {
    pub fn new(
        color: LedColor,
        description: &str,
        gpio_active_high: bool,
    ) -> Arc<Self> {
        Self::new_named("led", color, description, gpio_active_high)
    }

    pub fn new_named(
        local_id: &str,
        color: LedColor,
        description: &str,
        gpio_active_high: bool,
    ) -> Arc<Self> {
        let initial = if gpio_active_high { INTENSITY_MAX } else { 0 };
        Arc::new(Self {
            state: parking_lot::Mutex::new(MDeviceState::new(local_id)),
            intensity: parking_lot::Mutex::new(initial),
            gpio_active_high,
            color,
            description: description.to_string(),
        })
    }

    /// Set intensity as a percentage (0–100).
    pub fn set_intensity(&self, percent: u8) {
        *self.intensity.lock() = percent.min(INTENSITY_MAX);
    }

    /// Get current intensity as a percentage.
    pub fn get_intensity(&self) -> u8 {
        *self.intensity.lock()
    }

    /// Set the LED emitting state directly (e.g. from software).
    pub fn set_state(&self, is_emitting: bool) {
        self.set_intensity(if is_emitting { INTENSITY_MAX } else { 0 });
    }

    /// GPIO input: applies active-high/active-low polarity.
    pub fn set_gpio(&self, level: bool) {
        let is_emitting = level == self.gpio_active_high;
        self.set_state(is_emitting);
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

    pub fn reset_runtime(&self) {
        let initial = if self.gpio_active_high {
            INTENSITY_MAX
        } else {
            0
        };
        *self.intensity.lock() = initial;
    }
}
