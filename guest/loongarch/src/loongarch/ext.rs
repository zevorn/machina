#[allow(clippy::struct_excessive_bools)]
pub struct LoongArchCfg {
    pub has_fpu: bool,
    pub has_lsx: bool,
    pub has_lasx: bool,
    pub has_lbt: bool,
}

impl Default for LoongArchCfg {
    fn default() -> Self {
        Self {
            has_fpu: true,
            has_lsx: false,
            has_lasx: false,
            has_lbt: false,
        }
    }
}
