use machina_hw_firmware::{keys, FwCfg, FwCfgDataGenerator};

// -- Positive Tests --

#[test]
fn test_fw_cfg_new_has_signature() {
    let fw = FwCfg::new(10);
    assert!(fw.has_entry(keys::SIGNATURE));
    let entry = fw.get_entry(keys::SIGNATURE).unwrap();
    assert_eq!(entry.data, b"QEMU".to_vec());
}

#[test]
fn test_fw_cfg_add_bytes() {
    let fw = FwCfg::new(10);
    fw.add_bytes(0x1000, vec![0x01, 0x02, 0x03]);
    assert!(fw.has_entry(0x1000));
    let entry = fw.get_entry(0x1000).unwrap();
    assert_eq!(entry.data, vec![0x01, 0x02, 0x03]);
}

#[test]
fn test_fw_cfg_add_string() {
    let fw = FwCfg::new(10);
    fw.add_string(0x0009, "console=ttyS0");
    let entry = fw.get_entry(0x0009).unwrap();
    // NUL-terminated
    assert_eq!(entry.data, b"console=ttyS0\0".to_vec());
}

#[test]
fn test_fw_cfg_add_file() {
    let fw = FwCfg::new(10);
    fw.add_file(0x8000, "etc/foo", vec![0xAA, 0xBB, 0xCC]);
    let entry = fw.get_entry(0x8000).unwrap();
    assert_eq!(entry.data, vec![0xAA, 0xBB, 0xCC]);
    assert!(entry.file.is_some());
    let file = entry.file.unwrap();
    assert_eq!(file.size, 3);
    assert_eq!(file.select, 0x8000);
    assert_eq!(file.name, "etc/foo");
}

#[test]
fn test_fw_cfg_add_i16() {
    let fw = FwCfg::new(10);
    fw.add_i16(0x1000, 0x1234);
    let entry = fw.get_entry(0x1000).unwrap();
    assert_eq!(entry.data, 0x1234u16.to_le_bytes().to_vec());
}

#[test]
fn test_fw_cfg_add_i32() {
    let fw = FwCfg::new(10);
    fw.add_i32(0x1000, 0x12345678);
    let entry = fw.get_entry(0x1000).unwrap();
    assert_eq!(entry.data, 0x12345678u32.to_le_bytes().to_vec());
}

#[test]
fn test_fw_cfg_add_i64() {
    let fw = FwCfg::new(10);
    fw.add_i64(0x1000, 0x123456789ABCDEF0);
    let entry = fw.get_entry(0x1000).unwrap();
    assert_eq!(entry.data, 0x123456789ABCDEF0u64.to_le_bytes().to_vec());
}

#[test]
fn test_fw_cfg_read_byte() {
    let fw = FwCfg::new(10);
    fw.add_bytes(0x0003, vec![0x40, 0x00, 0x00, 0x00]); // 64 MB LE
    fw.set_selector(0x0003);

    assert_eq!(fw.read_byte(), 0x40);
    assert_eq!(fw.read_byte(), 0x00);
    assert_eq!(fw.read_byte(), 0x00);
    assert_eq!(fw.read_byte(), 0x00);
}

#[test]
fn test_fw_cfg_read_byte_past_end() {
    let fw = FwCfg::new(10);
    fw.add_bytes(0x0003, vec![0x01, 0x02]);
    fw.set_selector(0x0003);

    assert_eq!(fw.read_byte(), 0x01);
    assert_eq!(fw.read_byte(), 0x02);
    // Past end returns 0
    assert_eq!(fw.read_byte(), 0);
    assert_eq!(fw.read_byte(), 0);
}

#[test]
fn test_fw_cfg_selector() {
    let fw = FwCfg::new(10);
    fw.set_selector(0x0019);
    assert_eq!(fw.selector(), 0x0019);
    fw.set_selector(0x0000);
    assert_eq!(fw.selector(), 0x0000);
}

#[test]
fn test_fw_cfg_entry_count() {
    let fw = FwCfg::new(10);
    // Always has SIGNATURE
    let base = fw.entry_count();
    fw.add_bytes(0x1000, vec![0x01]);
    assert_eq!(fw.entry_count(), base + 1);
}

#[test]
fn test_fw_cfg_dma_enabled() {
    let fw = FwCfg::new(10);
    assert!(!fw.dma_enabled());
    fw.set_dma_enabled(true);
    assert!(fw.dma_enabled());
    fw.set_dma_enabled(false);
    assert!(!fw.dma_enabled());
}

#[test]
fn test_fw_cfg_build_file_dir() {
    let fw = FwCfg::new(10);
    fw.add_file(0x8000, "etc/acpi/rsdp", vec![0x01]);
    fw.add_file(0x8001, "etc/table-loader", vec![0x02, 0x03]);

    let dir = fw.build_file_dir();
    // First 4 bytes: file count (big-endian)
    assert_eq!(&dir[0..4], 2u32.to_be_bytes().as_slice());

    // Each entry: size(4) + select(2) + reserved(2) + name(56) = 64 bytes
    assert_eq!(dir.len(), 4 + 2 * 64);
}

#[test]
fn test_fw_cfg_build_file_dir_empty() {
    let fw = FwCfg::new(10);
    // No file entries added
    let dir = fw.build_file_dir();
    assert_eq!(&dir[0..4], 0u32.to_be_bytes().as_slice());
    assert_eq!(dir.len(), 4);
}

// -- Negative Tests --

#[test]
fn test_fw_cfg_read_byte_no_entry() {
    let fw = FwCfg::new(10);
    // Set selector to a key with no entry; reads 0
    fw.set_selector(0xFFFF);
    assert_eq!(fw.read_byte(), 0);
}

#[test]
fn test_fw_cfg_read_byte_nonexistent_entry() {
    let fw = FwCfg::new(10);
    fw.set_selector(0xFFFF); // No entry at this key
    assert_eq!(fw.read_byte(), 0);
}

#[test]
fn test_fw_cfg_get_entry_nonexistent() {
    let fw = FwCfg::new(10);
    assert!(fw.get_entry(0xDEAD).is_none());
}

#[test]
fn test_fw_cfg_has_entry_nonexistent() {
    let fw = FwCfg::new(10);
    assert!(!fw.has_entry(0xDEAD));
}

#[test]
fn test_fw_cfg_overwrite_entry() {
    let fw = FwCfg::new(10);
    fw.add_bytes(0x1000, vec![0x01, 0x02]);
    fw.add_bytes(0x1000, vec![0x03]);
    let entry = fw.get_entry(0x1000).unwrap();
    assert_eq!(entry.data, vec![0x03]);
}

// -- FwCfgDataGenerator trait test --

struct ConstDataGen(Vec<u8>);

impl FwCfgDataGenerator for ConstDataGen {
    fn get_data(&self) -> Result<Option<Vec<u8>>, String> {
        Ok(Some(self.0.clone()))
    }
}

struct EmptyDataGen;

impl FwCfgDataGenerator for EmptyDataGen {
    fn get_data(&self) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }
}

struct ErrorDataGen;

impl FwCfgDataGenerator for ErrorDataGen {
    fn get_data(&self) -> Result<Option<Vec<u8>>, String> {
        Err("generator failed".to_string())
    }
}

#[test]
fn test_fw_cfg_data_generator_const() {
    let gen = ConstDataGen(vec![0xDE, 0xAD]);
    let data = gen.get_data().unwrap().unwrap();
    assert_eq!(data, vec![0xDE, 0xAD]);
}

#[test]
fn test_fw_cfg_data_generator_empty() {
    let gen = EmptyDataGen;
    let data = gen.get_data().unwrap();
    assert!(data.is_none());
}

#[test]
fn test_fw_cfg_data_generator_error() {
    let gen = ErrorDataGen;
    let result = gen.get_data();
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "generator failed");
}
