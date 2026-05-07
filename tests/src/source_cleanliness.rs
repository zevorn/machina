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

const MOM_CORE_FILES: &[&str] = &[
    "core/src/mobject.rs",
    "hw/core/src/bus.rs",
    "hw/core/src/mdev.rs",
    "hw/core/src/reset.rs",
    "hw/core/src/typeinfo.rs",
];

const QEMU_C_QOM_TERMS: &[&str] =
    &["ParentField", "ParentInit", "qom_isa", "ObjectType"];

const HAND_WRITTEN_MOM_ACCESSORS: &[&str] = &[
    "pub fn attach_to_bus",
    "pub fn register_mmio",
    "pub fn with_mdevice",
    "pub fn object_info",
    "pub fn realized",
    "pub fn realize(self: &Arc<Self>)",
    "pub fn unrealize(self: &Arc<Self>)",
];

const LOW_LEVEL_MOM_ACCESSOR_HELPERS: &[&str] = &[
    "machina_std_mutex_sysbus_accessors!(",
    "machina_parking_lot_sysbus_accessors!(",
    "machina_direct_sysbus_accessors!(",
    "machina_parking_lot_sysbus_child_accessors!(",
    "machina_std_mutex_mdevice_accessors!(",
    "machina_parking_lot_mdevice_accessors!(",
];

const DIRECT_REGISTER_BANK_FIELDS: &[&str] = &[
    "regs: DeviceRefCell<",
    "regs: Mutex<",
    "regs: parking_lot::Mutex<",
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

#[test]
fn translated_device_registers_use_private_device_regs() {
    let repo = repo_root();
    let mut violations = Vec::new();

    for file in DEVICE_SOURCE_FILES {
        let content = std::fs::read_to_string(repo.join(file)).unwrap();
        for (line_number, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("pub struct ") && trimmed.contains("Regs") {
                violations
                    .push(format!("{file}:{}: {trimmed}", line_number + 1));
            }
            if trimmed.starts_with("pub fn regs")
                || trimmed.starts_with("pub(crate) fn regs")
                || trimmed.starts_with("pub regs:")
                || trimmed.starts_with("pub(crate) regs:")
            {
                violations
                    .push(format!("{file}:{}: {trimmed}", line_number + 1));
            }
            for pattern in DIRECT_REGISTER_BANK_FIELDS {
                if line.contains(pattern) && line.contains("Regs") {
                    violations.push(format!(
                        "{file}:{}: register banks must use DeviceRegs<T>",
                        line_number + 1
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "translated device register banks should be private and use DeviceRegs<T>: {violations:#?}"
    );
}

#[test]
fn mom_core_does_not_use_unsafe_or_qemu_c_qom_terms() {
    let repo = repo_root();
    let mut violations = Vec::new();

    for file in MOM_CORE_FILES {
        let content = std::fs::read_to_string(repo.join(file)).unwrap();
        if contains_token(&content, "unsafe") {
            violations.push(format!("{file}: unsafe"));
        }
        for term in QEMU_C_QOM_TERMS {
            if content.contains(term) {
                violations.push(format!("{file}: {term}"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "MOM core must stay safe Rust and avoid QEMU C QOM binding terms: {violations:#?}"
    );
}

#[test]
fn ref_machine_mom_object_infos_use_object_tree_snapshot() {
    let repo = repo_root();
    let content =
        std::fs::read_to_string(repo.join("hw/riscv/src/ref_machine.rs"))
            .unwrap();
    let start = content
        .find("fn mom_object_infos")
        .expect("RefMachine has mom_object_infos");
    let rest = &content[start..];
    let end = rest
        .find("\n    fn object_matches")
        .expect("mom_object_infos is followed by object_matches");
    let body = &rest[..end];

    assert!(
        body.contains("self.mom_tree.infos()"),
        "RefMachine mom_object_infos must be generated from MObjectTree snapshot"
    );
    assert!(
        !body.contains("if let Some("),
        "RefMachine mom_object_infos must not hand-collect each optional device"
    );
}

#[test]
fn translated_devices_use_mom_accessor_helpers() {
    let repo = repo_root();
    let mut violations = Vec::new();

    for file in DEVICE_SOURCE_FILES
        .iter()
        .copied()
        .chain(["hw/virtio/src/mmio.rs"])
    {
        let content = std::fs::read_to_string(repo.join(file)).unwrap();
        for accessor in HAND_WRITTEN_MOM_ACCESSORS {
            if content.contains(accessor) {
                violations.push(format!("{file}: {accessor}"));
            }
        }
        for helper in LOW_LEVEL_MOM_ACCESSOR_HELPERS {
            if content.contains(helper) {
                violations.push(format!("{file}: {helper}"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "translated device sources should use MOM derive/attribute wrappers instead of hand-written forwarding or low-level accessor helpers: {violations:#?}"
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
