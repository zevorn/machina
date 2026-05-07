#[cfg(test)]
mod tests {
    use machina_core::machine::{LoaderSpec, Machine, MachineOpts};
    use machina_hw_riscv::k230::K230Machine;

    #[test]
    fn loader_spec_parses_qemu_loader_syntax() {
        let spec = LoaderSpec::parse(
            "loader,file=/tmp/fw.uImage,addr=0x0c100000,force-raw=on",
        )
        .unwrap();
        assert_eq!(spec.file.to_str(), Some("/tmp/fw.uImage"));
        assert_eq!(spec.addr, 0x0c10_0000);
        assert!(spec.force_raw);
    }

    #[test]
    fn loader_spec_rejects_missing_file() {
        let err =
            LoaderSpec::parse("loader,addr=0x1000,force-raw=on").unwrap_err();
        assert!(err.contains("missing file="));
    }

    fn k230_opts() -> MachineOpts {
        MachineOpts {
            ram_size: 0x8000_0000,
            cpu_count: 1,
            kernel: None,
            bios: Some("none".into()),
            bios_builtin: false,
            append: None,
            nographic: true,
            drive: None,
            initrd: None,
            dtb: None,
            loaders: Vec::new(),
            netdev: None,
        }
    }

    #[test]
    fn k230_direct_boot_rejects_initrd_without_dtb() {
        let dir = tempfile::tempdir().unwrap();
        let initrd = dir.path().join("rootfs.cpio.gz");
        std::fs::write(&initrd, b"initrd").unwrap();

        let mut machine = K230Machine::new();
        let opts = MachineOpts {
            initrd: Some(initrd),
            ..k230_opts()
        };
        machine.init(&opts).unwrap();

        let err = machine.boot().unwrap_err().to_string();
        assert!(err.contains("-initrd requires -dtb for the k230 machine"));
    }

    #[test]
    fn k230_dtb_fixup_disables_sdk_sdhci_nodes() {
        let mut blob =
            machina_hw_riscv::k230_dtb::test_fixture_dtb_with_sdhci_nodes();
        blob = machina_hw_riscv::k230_dtb::fixup_k230_dtb(
            &blob,
            Some((0x0a10_0000, 0x0a20_0000)),
            Some("console=ttyS0,115200 earlycon=sbi cma=0"),
        )
        .unwrap();

        assert_eq!(
            machina_hw_riscv::k230_dtb::dtb_node_status(
                &blob,
                "/soc/sdhci0@91580000"
            )
            .unwrap(),
            Some("disabled".to_string()),
        );
        assert_eq!(
            machina_hw_riscv::k230_dtb::dtb_node_status(
                &blob,
                "/soc/sdhci1@91581000"
            )
            .unwrap(),
            Some("disabled".to_string()),
        );
        assert!(blob
            .windows(b"console=ttyS0,115200 earlycon=sbi cma=0".len())
            .any(|w| w == b"console=ttyS0,115200 earlycon=sbi cma=0"));
    }
}
