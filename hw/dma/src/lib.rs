//! DMA controller devices.

mod pl080;
mod sifive_pdma;

pub use pl080::{Pl080, Pl080Mmio, PL080_MMIO_SIZE};
pub use sifive_pdma::{SifivePdma, SifivePdmaMmio, SIFIVE_PDMA_REG_SIZE};
