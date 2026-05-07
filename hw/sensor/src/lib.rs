//! Sensor devices.

mod tmp105;
mod tmp421;

pub use tmp105::{Tmp105, Tmp105Error};
pub use tmp421::{Tmp421, Tmp421Error};
