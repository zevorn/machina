use std::sync::Arc;

use machina_hw_core::mdev::MDeviceState;
use machina_hw_i2c::{I2cError, I2cEvent, I2cSlave};

const MANUFACTURER_ID: u8 = 0x55;
const TMP421_DEVICE_ID: u8 = 0x21;
const TMP422_DEVICE_ID: u8 = 0x22;
const TMP423_DEVICE_ID: u8 = 0x23;

const STATUS_REG: u8 = 0x08;
const CONFIG_REG_1: u8 = 0x09;
const CONFIG_REG_2: u8 = 0x0a;
const CONVERSION_RATE_REG: u8 = 0x0b;
const RESET_REG: u8 = 0xfc;
const MANUFACTURER_ID_REG: u8 = 0xfe;
const DEVICE_ID_REG: u8 = 0xff;

const TEMP_MSB0: u8 = 0x00;
const TEMP_MSB3: u8 = 0x03;
const TEMP_LSB0: u8 = 0x10;
const TEMP_LSB3: u8 = 0x13;

const CONFIG_RANGE: u8 = 1 << 2;

#[derive(Clone, Copy)]
enum Tmp421Model {
    Tmp421,
    Tmp422,
    Tmp423,
}

impl Tmp421Model {
    fn device_id(self) -> u8 {
        match self {
            Self::Tmp421 => TMP421_DEVICE_ID,
            Self::Tmp422 => TMP422_DEVICE_ID,
            Self::Tmp423 => TMP423_DEVICE_ID,
        }
    }

    fn default_config2(self) -> u8 {
        match self {
            Self::Tmp421 => 0x1c,
            Self::Tmp422 => 0x3c,
            Self::Tmp423 => 0x7c,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Tmp421Error {
    InvalidChannel,
    TemperatureOutOfRange,
}

impl std::fmt::Display for Tmp421Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidChannel => f.write_str("invalid temperature channel"),
            Self::TemperatureOutOfRange => {
                f.write_str("temperature is out of range")
            }
        }
    }
}

impl std::error::Error for Tmp421Error {}

#[derive(Clone, Copy)]
struct Tmp421State {
    temperature: [i16; 4],
    status: u8,
    config: [u8; 2],
    rate: u8,
    len: u8,
    buf: [u8; 2],
    pointer: u8,
}

impl Default for Tmp421State {
    fn default() -> Self {
        Self::new(Tmp421Model::Tmp421)
    }
}

impl Tmp421State {
    fn new(model: Tmp421Model) -> Self {
        Self {
            temperature: [0; 4],
            status: 0,
            config: [0, model.default_config2()],
            rate: 0x07,
            len: 0,
            buf: [0; 2],
            pointer: 0,
        }
    }
}

#[derive(machina_hw_core::MDevice)]
#[mom(state = mdevice, lock = "parking_lot")]
pub struct Tmp421 {
    mdevice: parking_lot::Mutex<MDeviceState>,
    address: u8,
    model: Tmp421Model,
    state: parking_lot::Mutex<Tmp421State>,
}

impl Tmp421 {
    pub fn new(address: u8) -> Arc<Self> {
        Self::new_named("tmp421", address)
    }

    pub fn new_named(local_id: &str, address: u8) -> Arc<Self> {
        Self::new_model_named(local_id, address, Tmp421Model::Tmp421)
    }

    pub fn new_tmp422(address: u8) -> Arc<Self> {
        Self::new_model_named("tmp422", address, Tmp421Model::Tmp422)
    }

    pub fn new_tmp423(address: u8) -> Arc<Self> {
        Self::new_model_named("tmp423", address, Tmp421Model::Tmp423)
    }

    fn new_model_named(
        local_id: &str,
        address: u8,
        model: Tmp421Model,
    ) -> Arc<Self> {
        Arc::new(Self {
            mdevice: parking_lot::Mutex::new(MDeviceState::new(local_id)),
            address,
            model,
            state: parking_lot::Mutex::new(Tmp421State::new(model)),
        })
    }

    pub fn reset_runtime(&self) {
        *self.state.lock() = Tmp421State::new(self.model);
    }

    pub fn set_temperature_millicelsius(
        &self,
        channel: usize,
        temp: i64,
    ) -> Result<(), Tmp421Error> {
        if channel >= 4 {
            return Err(Tmp421Error::InvalidChannel);
        }
        let mut state = self.state.lock();
        let ext_range = state.config[0] & CONFIG_RANGE != 0;
        let min = if ext_range { -55_000 } else { -40_000 };
        let max = if ext_range { 150_000 } else { 127_000 };
        if temp < min || temp >= max {
            return Err(Tmp421Error::TemperatureOutOfRange);
        }
        let offset = if ext_range { 64 * 256 } else { 0 };
        state.temperature[channel] =
            ((temp * 256 - 128) / 1000) as i16 + offset;
        Ok(())
    }

    fn read_to_buffer(model: Tmp421Model, state: &mut Tmp421State) {
        state.len = 0;
        match state.pointer {
            MANUFACTURER_ID_REG => push1(state, MANUFACTURER_ID),
            DEVICE_ID_REG => push1(state, model.device_id()),
            CONFIG_REG_1 => push1(state, state.config[0]),
            CONFIG_REG_2 => push1(state, state.config[1]),
            CONVERSION_RATE_REG => push1(state, state.rate),
            STATUS_REG => push1(state, state.status),
            TEMP_MSB0..=TEMP_MSB3 => {
                let index = usize::from(state.pointer - TEMP_MSB0);
                push_temperature(state, index);
            }
            TEMP_LSB0..=TEMP_LSB3 => {
                let index = usize::from(state.pointer - TEMP_LSB0);
                push1(state, (state.temperature[index] as u16 & 0x00f0) as u8);
            }
            _ => {}
        }
    }

    fn write_from_buffer(model: Tmp421Model, state: &mut Tmp421State) {
        match state.pointer {
            CONVERSION_RATE_REG => state.rate = state.buf[0],
            CONFIG_REG_1 => state.config[0] = state.buf[0],
            CONFIG_REG_2 => state.config[1] = state.buf[0],
            RESET_REG => *state = Tmp421State::new(model),
            _ => {}
        }
    }
}

impl I2cSlave for Tmp421 {
    fn address(&self) -> u8 {
        self.address
    }

    fn event(&self, event: I2cEvent) -> Result<(), I2cError> {
        let mut state = self.state.lock();
        if event == I2cEvent::StartRecv {
            Self::read_to_buffer(self.model, &mut state);
        }
        state.len = 0;
        Ok(())
    }

    fn send(&self, data: u8) -> Result<(), I2cError> {
        let mut state = self.state.lock();
        if state.len == 0 {
            state.pointer = data;
            state.len += 1;
        } else if state.len == 1 {
            state.buf[0] = data;
            Self::write_from_buffer(self.model, &mut state);
        }
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

fn push1(state: &mut Tmp421State, value: u8) {
    state.buf[0] = value;
    state.len = 1;
}

fn push_temperature(state: &mut Tmp421State, index: usize) {
    let temperature = state.temperature[index] as u16;
    state.buf[0] = (temperature >> 8) as u8;
    state.buf[1] = (temperature & 0x00f0) as u8;
    state.len = 2;
}
