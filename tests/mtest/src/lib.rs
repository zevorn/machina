#[cfg(test)]
mod tests {
    use machina_core::machine::LoaderSpec;

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
}
