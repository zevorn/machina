use std::sync::Arc;

use machina_hw_core::irq::InterruptSource;
use machina_hw_core::mdev::MDeviceState;
use machina_hw_i2c::{I2cError, I2cEvent, I2cSlave};

const REG_TEMPERATURE: u8 = 0;
const REG_CONFIG: u8 = 1;
const REG_T_LOW: u8 = 2;
const REG_T_HIGH: u8 = 3;

const CONFIG_SHUTDOWN_MODE: u8 = 1 << 0;
const CONFIG_THERMOSTAT_MODE: u8 = 1 << 1;
const CONFIG_POLARITY: u8 = 1 << 2;
const CONFIG_FAULT_QUEUE_SHIFT: u8 = 3;
const CONFIG_CONVERTER_RESOLUTION_SHIFT: u8 = 5;
const CONFIG_ONE_SHOT: u8 = 1 << 7;

const FAULT_QUEUE: [u8; 4] = [1, 2, 4, 6];

#[derive(Debug, PartialEq, Eq)]
pub enum Tmp105Error {
    TemperatureOutOfRange,
}

impl std::fmt::Display for Tmp105Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TemperatureOutOfRange => {
                f.write_str("temperature is out of range")
            }
        }
    }
}

impl std::error::Error for Tmp105Error {}

struct Tmp105State {
    len: u8,
    buf: [u8; 2],
    pointer: u8,
    config: u8,
    temperature: i16,
    limit: [i16; 2],
    faults: u8,
    alarm: bool,
    detect_falling: bool,
}

impl Default for Tmp105State {
    fn default() -> Self {
        Self {
            len: 0,
            buf: [0; 2],
            pointer: 0,
            config: 0,
            temperature: 0,
            limit: [0x4b00, 0x5000],
            faults: FAULT_QUEUE[0],
            alarm: false,
            detect_falling: false,
        }
    }
}

pub struct Tmp105 {
    mdevice: parking_lot::Mutex<MDeviceState>,
    address: u8,
    state: parking_lot::Mutex<Tmp105State>,
    alert: parking_lot::Mutex<Option<InterruptSource>>,
}

impl Tmp105 {
    pub fn new(address: u8) -> Arc<Self> {
        Self::new_named("tmp105", address)
    }

    pub fn new_named(local_id: &str, address: u8) -> Arc<Self> {
        Arc::new(Self {
            mdevice: parking_lot::Mutex::new(MDeviceState::new(local_id)),
            address,
            state: parking_lot::Mutex::new(Tmp105State::default()),
            alert: parking_lot::Mutex::new(None),
        })
    }

    machina_hw_core::machina_parking_lot_mdevice_accessors!(mdevice);

    pub fn connect_alert(&self, irq: InterruptSource) {
        *self.alert.lock() = Some(irq);
        self.update_interrupt();
    }

    pub fn reset_runtime(&self) {
        *self.state.lock() = Tmp105State::default();
        self.update_interrupt();
    }

    pub fn set_temperature_millicelsius(
        &self,
        temp: i64,
    ) -> Result<(), Tmp105Error> {
        if !(-128_000..128_000).contains(&temp) {
            return Err(Tmp105Error::TemperatureOutOfRange);
        }
        {
            let mut state = self.state.lock();
            state.temperature = (temp * 256 / 1000) as i16;
            update_alarm(&mut state, false);
        }
        self.update_interrupt();
        Ok(())
    }

    fn read_to_buffer(state: &mut Tmp105State) {
        state.len = 0;
        if state.config & CONFIG_THERMOSTAT_MODE != 0 {
            state.alarm = false;
        }

        match state.pointer & 3 {
            REG_TEMPERATURE => {
                let temperature = state.temperature as u16;
                state.buf[0] = (temperature >> 8) as u8;
                let resolution =
                    ((!state.config) >> CONFIG_CONVERTER_RESOLUTION_SHIFT) & 3;
                let mask = (0x00f0u16 << resolution) as u8;
                state.buf[1] = (temperature as u8) & mask;
                state.len = 2;
            }
            REG_CONFIG => {
                state.buf[0] = state.config;
                state.len = 1;
            }
            REG_T_LOW => {
                let limit = state.limit[0] as u16;
                state.buf[0] = (limit >> 8) as u8;
                state.buf[1] = limit as u8;
                state.len = 2;
            }
            REG_T_HIGH => {
                let limit = state.limit[1] as u16;
                state.buf[0] = (limit >> 8) as u8;
                state.buf[1] = limit as u8;
                state.len = 2;
            }
            _ => {}
        }
    }

    fn write_from_buffer(state: &mut Tmp105State) {
        match state.pointer & 3 {
            REG_TEMPERATURE => {}
            REG_CONFIG => {
                state.config = state.buf[0] & !CONFIG_ONE_SHOT;
                let fault_index =
                    (state.config >> CONFIG_FAULT_QUEUE_SHIFT) & 3;
                state.faults = FAULT_QUEUE[usize::from(fault_index)];
                update_alarm(state, state.buf[0] & CONFIG_ONE_SHOT != 0);
            }
            REG_T_LOW | REG_T_HIGH => {
                if state.len >= 3 {
                    let limit = ((u16::from(state.buf[0])) << 8)
                        | u16::from(state.buf[1] & 0xf0);
                    state.limit[usize::from(state.pointer & 1)] = limit as i16;
                }
                update_alarm(state, false);
            }
            _ => {}
        }
    }

    fn update_interrupt(&self) {
        let state = self.state.lock();
        let active_low = state.config & CONFIG_POLARITY == 0;
        let level = state.alarm ^ active_low;
        drop(state);
        if let Some(ref irq) = *self.alert.lock() {
            irq.set(level);
        }
    }
}

impl I2cSlave for Tmp105 {
    fn address(&self) -> u8 {
        self.address
    }

    fn event(&self, event: I2cEvent) -> Result<(), I2cError> {
        {
            let mut state = self.state.lock();
            if event == I2cEvent::StartRecv {
                Self::read_to_buffer(&mut state);
            }
            state.len = 0;
        }
        self.update_interrupt();
        Ok(())
    }

    fn send(&self, data: u8) -> Result<(), I2cError> {
        {
            let mut state = self.state.lock();
            if state.len == 0 {
                state.pointer = data;
                state.len += 1;
                return Ok(());
            }
            if state.len <= 2 {
                let index = usize::from(state.len - 1);
                state.buf[index] = data;
            }
            state.len += 1;
            Self::write_from_buffer(&mut state);
        }
        self.update_interrupt();
        Ok(())
    }

    fn recv(&self) -> u8 {
        let mut state = self.state.lock();
        if state.len < 2 {
            let index = usize::from(state.len);
            state.len += 1;
            state.buf[index]
        } else {
            0xff
        }
    }
}

fn update_alarm(state: &mut Tmp105State, one_shot: bool) {
    if state.config & CONFIG_SHUTDOWN_MODE != 0 && !one_shot {
        return;
    }

    if state.config & CONFIG_THERMOSTAT_MODE != 0 {
        if state.detect_falling {
            if state.temperature < state.limit[0] {
                state.alarm = true;
                state.detect_falling = false;
            }
        } else if state.temperature >= state.limit[1] {
            state.alarm = true;
            state.detect_falling = true;
        }
    } else if state.detect_falling {
        if state.temperature < state.limit[0] {
            state.alarm = false;
            state.detect_falling = false;
        }
    } else if state.temperature >= state.limit[1] {
        state.alarm = true;
        state.detect_falling = true;
    }
}
