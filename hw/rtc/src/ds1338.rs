use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;

use machina_hw_core::mdev::MDeviceState;
use machina_hw_i2c::{I2cError, I2cEvent, I2cSlave};

const NVRAM_SIZE: usize = 64;

// Control register bits
const CTRL_OSF: u8 = 0x20;

// Hours register bits
const HOURS_12: u8 = 0x40;
const HOURS_PM: u8 = 0x20;

fn to_bcd(mut val: u8) -> u8 {
    if val >= 100 {
        val = 99;
    }
    ((val / 10) << 4) | (val % 10)
}

fn from_bcd(val: u8) -> u8 {
    ((val >> 4) & 0x0F) * 10 + (val & 0x0F)
}

pub struct Ds1338 {
    state: Mutex<MDeviceState>,
    /// Time offset from real time in seconds.
    offset: AtomicI64,
    /// Weekday offset adjustment.
    wday_offset: AtomicI64,
    /// NVRAM including time registers (0-6) and user RAM (8-63).
    nvram: Mutex<[u8; NVRAM_SIZE]>,
    /// Current register pointer.
    ptr: Mutex<i32>,
    /// Whether next byte is the address pointer.
    addr_byte: Mutex<bool>,
    /// I2C address.
    i2c_address: u8,
}

impl Ds1338 {
    #[must_use]
    pub fn new(address: u8) -> Self {
        Self::new_named("ds1338", address)
    }

    #[must_use]
    pub fn new_named(local_id: &str, address: u8) -> Self {
        Self {
            state: Mutex::new(MDeviceState::new(local_id)),
            offset: AtomicI64::new(0),
            wday_offset: AtomicI64::new(0),
            nvram: Mutex::new([0u8; NVRAM_SIZE]),
            ptr: Mutex::new(0),
            addr_byte: Mutex::new(false),
            i2c_address: address,
        }
    }

    machina_hw_core::machina_std_mutex_mdevice_accessors!(state);

    fn capture_current_time(&self) {
        // Simplified: set time registers from offset.
        // offset is total seconds; decompose into time fields.
        let total_secs = self.offset.load(Ordering::Relaxed);
        let sec = (total_secs % 60) as u8;
        let total_mins = total_secs / 60;
        let min = (total_mins % 60) as u8;
        let total_hours = total_mins / 60;
        let hour = (total_hours % 24) as u8;
        let total_days = total_hours / 24;
        let wday = ((total_days + 4) % 7) as u8 + 1; // Thursday 1970-01-01
        let (year, mon, mday) = days_to_date(total_days);

        let mut nvram = self.nvram.lock().unwrap();
        nvram[0] = to_bcd(sec);
        nvram[1] = to_bcd(min);
        nvram[2] = to_bcd(hour);
        nvram[3] = wday;
        nvram[4] = to_bcd(mday);
        nvram[5] = to_bcd(mon);
        nvram[6] = to_bcd((year - 2000) as u8);
    }

    fn inc_regptr(&self) {
        let mut ptr = self.ptr.lock().unwrap();
        *ptr = (*ptr + 1) & (NVRAM_SIZE as i32 - 1);
        if *ptr == 0 {
            drop(ptr);
            self.capture_current_time();
        }
    }

    /// Set absolute time in seconds since epoch.
    pub fn set_time(&self, secs: i64) {
        self.offset.store(secs, Ordering::Relaxed);
    }

    /// Get current time offset.
    #[must_use]
    pub fn get_offset(&self) -> i64 {
        self.offset.load(Ordering::Relaxed)
    }
}

/// Simple days-to-date conversion.
/// Returns (year, month, day) for days since 1970-01-01.
fn days_to_date(mut days: i64) -> (i64, u8, u8) {
    let mut year = 1970i64;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let month_days = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut mon = 1u8;
    for &md in &month_days {
        if days < md as i64 {
            break;
        }
        days -= md as i64;
        mon += 1;
    }
    (year, mon, (days + 1) as u8)
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

impl I2cSlave for Ds1338 {
    fn address(&self) -> u8 {
        self.i2c_address
    }

    fn event(&self, event: I2cEvent) -> Result<(), I2cError> {
        match event {
            I2cEvent::StartRecv => {
                self.capture_current_time();
            }
            I2cEvent::StartSend => {
                *self.addr_byte.lock().unwrap() = true;
            }
            _ => {}
        }
        Ok(())
    }

    fn send(&self, data: u8) -> Result<(), I2cError> {
        if *self.addr_byte.lock().unwrap() {
            *self.ptr.lock().unwrap() = (data & (NVRAM_SIZE as u8 - 1)) as i32;
            *self.addr_byte.lock().unwrap() = false;
            return Ok(());
        }

        let ptr = *self.ptr.lock().unwrap();
        if ptr < 7 {
            // Time register update
            let total_secs = self.offset.load(Ordering::Relaxed);
            let mut sec = (total_secs % 60) as u8;
            let total_mins = total_secs / 60;
            let mut min = (total_mins % 60) as u8;
            let total_hours = total_mins / 60;
            let mut hour = (total_hours % 24) as u8;
            let mut total_days = total_hours / 24;

            match ptr {
                0 => {
                    sec = from_bcd(data & 0x7F);
                }
                1 => {
                    min = from_bcd(data & 0x7F);
                }
                2 => {
                    if data & HOURS_12 != 0 {
                        let mut tmp = from_bcd(data & (HOURS_PM - 1)) as i64;
                        if data & HOURS_PM != 0 {
                            tmp += 12;
                        }
                        if tmp % 12 == 0 {
                            tmp -= 12;
                        }
                        hour = tmp as u8;
                    } else {
                        hour = from_bcd(data & (HOURS_12 - 1));
                    }
                }
                3 => {
                    let user_wday = (data & 7) as i64 - 1;
                    let cur_wday = (total_days + 4) % 7;
                    self.wday_offset.store(
                        (user_wday - cur_wday + 7) % 7,
                        Ordering::Relaxed,
                    );
                }
                4 => {
                    let mday = from_bcd(data & 0x3F);
                    // Adjust total_days based on the new mday
                    let old_mday = (total_days % 31) as u8 + 1;
                    let diff = mday as i64 - old_mday as i64;
                    total_days = (total_days + diff).max(0);
                }
                5 => {
                    let mon = from_bcd(data & 0x1F);
                    let old_mon = ((total_days / 31) % 12) as u8 + 1;
                    let diff = (mon as i64 - old_mon as i64) * 31;
                    total_days = (total_days + diff).max(0);
                }
                6 => {
                    let year = from_bcd(data) as i64 + 2000;
                    let old_year = 1970 + total_days / 365;
                    let diff = (year - old_year) * 365;
                    total_days = (total_days + diff).max(0);
                }
                _ => {}
            }

            // Recompute offset from time fields
            let new_total = sec as i64
                + min as i64 * 60
                + hour as i64 * 3600
                + total_days * 86400;
            self.offset.store(new_total, Ordering::Relaxed);
        } else if ptr == 7 {
            // Control register
            let mut nvram = self.nvram.lock().unwrap();
            let mut val = data & 0xB3; // bits 2,3,6 read back as 0
            let old = nvram[ptr as usize];
            // OSF bit: write 1 is ignored, stays 1 if already 1
            val = (val & !CTRL_OSF) | (val & old & CTRL_OSF);
            nvram[ptr as usize] = val;
        } else {
            self.nvram.lock().unwrap()[ptr as usize] = data;
        }

        let _ = ptr;
        self.inc_regptr();
        Ok(())
    }

    fn recv(&self) -> u8 {
        let ptr = *self.ptr.lock().unwrap();
        let res = self.nvram.lock().unwrap()[ptr as usize];
        let _ = ptr;
        self.inc_regptr();
        res
    }
}
