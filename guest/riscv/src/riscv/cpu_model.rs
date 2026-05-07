use super::ext::{MisaExt, RiscvCfg};

pub const THEAD_VENDOR_ID: u64 = 0x5b7;
pub const THEAD_C908_MARCHID: u64 = 0x8d143000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiscvVendor {
    Generic,
    Thead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiscvCpuModel {
    GenericRv64,
    TheadC908,
}

#[derive(Clone, Copy, Debug)]
pub struct RiscvCpuProfile {
    pub name: &'static str,
    pub vendor: RiscvVendor,
    pub misa: MisaExt,
    pub cfg: RiscvCfg,
    pub mvendorid: u64,
    pub marchid: u64,
    pub max_satp_mode: u64,
}

impl RiscvCpuProfile {
    pub const fn generic_rv64() -> Self {
        Self {
            name: "rv64",
            vendor: RiscvVendor::Generic,
            misa: RiscvCfg::RV64GC.misa,
            cfg: RiscvCfg::RV64GC,
            mvendorid: 0,
            marchid: 0,
            max_satp_mode: 8,
        }
    }

    pub const fn thead_c908() -> Self {
        Self {
            name: "thead-c908",
            vendor: RiscvVendor::Thead,
            misa: MisaExt::from_bits_truncate(
                MisaExt::I.bits()
                    | MisaExt::M.bits()
                    | MisaExt::A.bits()
                    | MisaExt::F.bits()
                    | MisaExt::D.bits()
                    | MisaExt::C.bits()
                    | MisaExt::S.bits()
                    | MisaExt::U.bits(),
            ),
            cfg: RiscvCfg::RV64GC,
            mvendorid: THEAD_VENDOR_ID,
            marchid: THEAD_C908_MARCHID,
            max_satp_mode: 9,
        }
    }
}

impl RiscvCpuModel {
    pub const fn profile(self) -> RiscvCpuProfile {
        match self {
            Self::GenericRv64 => RiscvCpuProfile::generic_rv64(),
            Self::TheadC908 => RiscvCpuProfile::thead_c908(),
        }
    }
}
