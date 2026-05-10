use std::path::Path;

const FW_NAMES: &[&str] = &[
    "rustsbi-riscv64-machina-fw_dynamic.bin",
    "rustsbi-riscv64-machina-k230-fw_dynamic.bin",
];

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out = std::env::var("OUT_DIR").unwrap();
    let embed_firmware = std::env::var("CARGO_FEATURE_EMBED_FIRMWARE").is_ok();

    for name in FW_NAMES {
        let src = Path::new(&manifest).join("../../pc-bios").join(name);
        println!("cargo:rerun-if-changed={}", src.display());

        if !embed_firmware {
            continue;
        }

        let dst = Path::new(&out).join(name);
        if src.exists() {
            std::fs::copy(&src, &dst).unwrap();
        } else {
            // Registry build: firmware unavailable, write empty
            // stub. User must supply firmware at runtime.
            std::fs::write(&dst, []).unwrap();
        }
    }
}
