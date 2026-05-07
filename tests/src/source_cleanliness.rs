use std::path::Path;

const DEVICE_SOURCE_FILES: &[&str] = &[
    "hw/char/src/pl011.rs",
    "hw/char/src/riscv_htif.rs",
    "hw/char/src/sifive_uart.rs",
    "hw/char/src/uart.rs",
    "hw/dma/src/pl080.rs",
    "hw/dma/src/sifive_pdma.rs",
    "hw/firmware/src/lib.rs",
    "hw/gpio/src/gpio_key.rs",
    "hw/gpio/src/gpio_pwr.rs",
    "hw/gpio/src/pl061.rs",
    "hw/gpio/src/sifive_gpio.rs",
    "hw/i2c/src/eeprom_at24c.rs",
    "hw/i2c/src/lib.rs",
    "hw/i2c/src/smbus_eeprom.rs",
    "hw/intc/src/aclint.rs",
    "hw/intc/src/aplic.rs",
    "hw/intc/src/dintc.rs",
    "hw/intc/src/eiointc.rs",
    "hw/intc/src/imsic.rs",
    "hw/intc/src/ipi.rs",
    "hw/intc/src/liointc.rs",
    "hw/intc/src/pch_msi.rs",
    "hw/intc/src/pch_pic.rs",
    "hw/intc/src/plic.rs",
    "hw/loongarch/src/boot.rs",
    "hw/misc/src/cmgcr.rs",
    "hw/misc/src/cpc.rs",
    "hw/misc/src/led.rs",
    "hw/misc/src/pl050.rs",
    "hw/misc/src/pvpanic.rs",
    "hw/misc/src/sifive_e_aon.rs",
    "hw/misc/src/sifive_e_prci.rs",
    "hw/misc/src/sifive_u_otp.rs",
    "hw/misc/src/sifive_u_prci.rs",
    "hw/misc/src/unimp.rs",
    "hw/misc/src/virt_ctrl.rs",
    "hw/riscv/src/sifive_test.rs",
    "hw/rtc/src/ds1338.rs",
    "hw/rtc/src/goldfish_rtc.rs",
    "hw/rtc/src/ls7a_rtc.rs",
    "hw/rtc/src/pl031.rs",
    "hw/sd/src/card.rs",
    "hw/sd/src/lib.rs",
    "hw/sd/src/pl181.rs",
    "hw/sd/src/sdhci.rs",
    "hw/sd/src/ssi_sd.rs",
    "hw/sensor/src/tmp105.rs",
    "hw/sensor/src/tmp421.rs",
    "hw/ssi/src/lib.rs",
    "hw/ssi/src/m25p80.rs",
    "hw/ssi/src/pl022.rs",
    "hw/ssi/src/sifive_spi.rs",
    "hw/storage/src/lib.rs",
    "hw/storage/src/pflash.rs",
    "hw/timer/src/lib.rs",
    "hw/timer/src/sifive_pwm.rs",
    "hw/timer/src/sse_counter.rs",
    "hw/timer/src/sse_timer.rs",
    "hw/watchdog/src/lib.rs",
];

#[test]
fn translated_device_sources_do_not_use_unsafe() {
    let repo = repo_root();
    let mut violations = Vec::new();

    for file in DEVICE_SOURCE_FILES {
        let content = std::fs::read_to_string(repo.join(file)).unwrap();
        if contains_token(&content, "unsafe") {
            violations.push(*file);
        }
    }

    assert!(
        violations.is_empty(),
        "translated device sources contain unsafe: {violations:#?}"
    );
}

#[test]
fn translated_device_sources_do_not_embed_qemu_references() {
    let repo = repo_root();
    let mut violations = Vec::new();

    for file in DEVICE_SOURCE_FILES {
        let content = std::fs::read_to_string(repo.join(file)).unwrap();
        if content.contains("QEMU")
            || content.contains("~/qemu")
            || content.contains("qemu/")
            || contains_commit_reference(&content)
            || contains_hex_run(&content, 40)
        {
            violations.push(*file);
        }
    }

    assert!(
        violations.is_empty(),
        "translated device sources embed QEMU references: {violations:#?}"
    );
}

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tests crate has repo parent")
}

fn contains_token(content: &str, token: &str) -> bool {
    content
        .split(|c: char| !(c == '_' || c.is_ascii_alphanumeric()))
        .any(|part| part == token)
}

fn contains_commit_reference(content: &str) -> bool {
    content
        .split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .any(|pair| {
            pair[0] == "commit" && pair[1].len() >= 7 && is_hex(pair[1])
        })
}

fn contains_hex_run(content: &str, len: usize) -> bool {
    let mut run = 0;
    for c in content.chars() {
        if c.is_ascii_hexdigit() {
            run += 1;
            if run >= len {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

fn is_hex(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_hexdigit())
}
