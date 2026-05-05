#[allow(
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    unused_variables,
    unused_imports
)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/loongarch_decode.rs"));
}
pub use generated::*;
