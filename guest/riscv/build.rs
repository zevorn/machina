use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();

    // 32-bit decoder
    let decode32 = Path::new("src/riscv/insn32.decode");
    let decode_xthead = Path::new("src/riscv/xthead.decode");
    println!("cargo::rerun-if-changed={}", decode32.display());
    println!("cargo::rerun-if-changed={}", decode_xthead.display());
    let input32 =
        fs::read_to_string(decode32).expect("failed to read insn32.decode");
    let xthead = fs::read_to_string(decode_xthead)
        .expect("failed to read xthead.decode");
    let combined32 = format!("{input32}\n{xthead}");
    let mut out32 = Vec::new();
    machina_decode::generate(&combined32, &mut out32)
        .expect("insn32 code generation failed");
    let path32 = Path::new(&out_dir).join("riscv32_decode.rs");
    fs::write(&path32, out32).expect("failed to write riscv32_decode.rs");

    // 16-bit decoder
    let decode16 = Path::new("src/riscv/insn16.decode");
    println!("cargo::rerun-if-changed={}", decode16.display());
    let input16 =
        fs::read_to_string(decode16).expect("failed to read insn16.decode");
    let mut out16 = Vec::new();
    machina_decode::generate_with_width(&input16, &mut out16, 16)
        .expect("insn16 code generation failed");
    let path16 = Path::new(&out_dir).join("riscv16_decode.rs");
    fs::write(&path16, out16).expect("failed to write riscv16_decode.rs");
}
