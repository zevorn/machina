use std::collections::BTreeMap;
use std::sync::Mutex;

use machina_hw_firmware::{
    keys, FwCfg, FwCfgDataGenerator, FwCfgDmaAccess, FwCfgDmaDescriptor,
};

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
fn test_fw_cfg_read_data_byte() {
    let fw = FwCfg::new(10);
    fw.add_bytes(0x0003, vec![0x40, 0x00, 0x00, 0x00]);
    fw.set_selector(0x0003);

    assert_eq!(fw.read_data_byte(), 0x40);
    assert_eq!(fw.read_data_byte(), 0x00);
    assert_eq!(fw.read_data_byte(), 0x00);
    assert_eq!(fw.read_data_byte(), 0x00);
}

#[test]
fn test_fw_cfg_read_data_byte_past_end() {
    let fw = FwCfg::new(10);
    fw.add_bytes(0x0003, vec![0x01, 0x02]);
    fw.set_selector(0x0003);

    assert_eq!(fw.read_data_byte(), 0x01);
    assert_eq!(fw.read_data_byte(), 0x02);
    assert_eq!(fw.read_data_byte(), 0);
    assert_eq!(fw.read_data_byte(), 0);
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
    assert_eq!(&dir[0..4], 2u32.to_be_bytes().as_slice());
    assert_eq!(dir.len(), 4 + 2 * 64);
}

#[test]
fn test_fw_cfg_build_file_dir_empty() {
    let fw = FwCfg::new(10);
    let dir = fw.build_file_dir();
    assert_eq!(&dir[0..4], 0u32.to_be_bytes().as_slice());
    assert_eq!(dir.len(), 4);
}

// -- IO selector register tests --

#[test]
fn test_fw_cfg_write_selector_two_bytes() {
    let fw = FwCfg::new(10);
    // Write selector 0x1234 as two bytes (big-endian IO): hi=0x12, lo=0x34
    fw.write_selector_byte(0x12); // lo byte (first write)
    fw.write_selector_byte(0x34); // hi byte (second write, commits)
    assert_eq!(fw.selector(), 0x1234);
}

#[test]
fn test_fw_cfg_write_selector_resets_offset() {
    let fw = FwCfg::new(10);
    fw.add_bytes(0x0042, vec![0xAA, 0xBB]);
    fw.set_selector(0x0042);
    assert_eq!(fw.read_data_byte(), 0xAA);

    // Setting new selector resets offset
    fw.set_selector(0x0042);
    assert_eq!(fw.read_data_byte(), 0xAA); // back to first byte
}

// -- Multi-byte data read tests --

#[test]
fn test_fw_cfg_read_data_word() {
    let fw = FwCfg::new(10);
    fw.add_bytes(0x0004, vec![0x01, 0x02, 0x03, 0x04]);
    fw.set_selector(0x0004);

    assert_eq!(fw.read_data_word(), 0x0201); // LE
    assert_eq!(fw.read_data_word(), 0x0403); // LE
}

#[test]
fn test_fw_cfg_read_data_dword() {
    let fw = FwCfg::new(10);
    fw.add_bytes(0x0004, vec![0x78, 0x56, 0x34, 0x12]);
    fw.set_selector(0x0004);

    assert_eq!(fw.read_data_dword(), 0x12345678);
}

#[test]
fn test_fw_cfg_read_data_word_past_end_padded() {
    let fw = FwCfg::new(10);
    fw.add_bytes(0x0004, vec![0xAA]); // only 1 byte
    fw.set_selector(0x0004);

    // Reading a word past end: first byte valid, second byte zero-padded
    assert_eq!(fw.read_data_word(), 0x00AA);
}

#[test]
fn test_fw_cfg_read_data_dword_past_end_padded() {
    let fw = FwCfg::new(10);
    fw.add_bytes(0x0004, vec![0x11, 0x22]); // only 2 bytes
    fw.set_selector(0x0004);

    // Reading dword past end: first 2 bytes valid, rest zero-padded
    assert_eq!(fw.read_data_dword(), 0x00002211);
}

// -- DMA descriptor tests --

#[test]
fn test_fw_cfg_dma_descriptor_encode_decode() {
    let desc = FwCfgDmaDescriptor {
        control: 0x00000006, // SELECT | READ
        length: 0x1000,
        address: 0x8000_0000,
    };
    let bytes = desc.encode();
    assert_eq!(bytes.len(), 16);

    let decoded = FwCfgDmaDescriptor::decode(&bytes).unwrap();
    assert_eq!(decoded.control, 6);
    assert_eq!(decoded.length, 0x1000);
    assert_eq!(decoded.address, 0x8000_0000);
}

#[test]
fn test_fw_cfg_dma_descriptor_decode_short() {
    assert!(FwCfgDmaDescriptor::decode(&[0u8; 8]).is_none());
}

// -- DMA transfer tests --

struct MockDmaAccess {
    mem: Mutex<BTreeMap<u64, Vec<u8>>>,
}

impl MockDmaAccess {
    fn new() -> Self {
        Self {
            mem: Mutex::new(BTreeMap::new()),
        }
    }

    fn write_mem(&self, addr: u64, data: &[u8]) {
        self.mem.lock().unwrap().insert(addr, data.to_vec());
    }

    fn read_mem(&self, addr: u64, len: usize) -> Vec<u8> {
        self.mem
            .lock()
            .unwrap()
            .get(&addr)
            .cloned()
            .unwrap_or_else(|| vec![0u8; len])
    }
}

impl FwCfgDmaAccess for MockDmaAccess {
    fn dma_read(&self, addr: u64, buf: &mut [u8]) -> Result<usize, String> {
        let mem = self.mem.lock().unwrap();
        if let Some(data) = mem.get(&addr) {
            let n = buf.len().min(data.len());
            buf[..n].copy_from_slice(&data[..n]);
            Ok(n)
        } else {
            Ok(0)
        }
    }

    fn dma_write(&self, addr: u64, buf: &[u8]) -> Result<usize, String> {
        self.mem.lock().unwrap().insert(addr, buf.to_vec());
        Ok(buf.len())
    }
}

#[test]
fn test_fw_cfg_dma_read() {
    let fw = FwCfg::new(10);
    fw.add_bytes(0x0003, vec![0x40, 0x00, 0x00, 0x00]);
    fw.set_dma_enabled(true);

    let access = MockDmaAccess::new();
    // Descriptor at addr 0x100: SELECT|READ, length=4, select_addr=0x200
    let desc = FwCfgDmaDescriptor {
        control: 0x0000_0006, // SELECT | READ
        length: 4,
        address: 0x200, // where to read selector key from
    };
    access.write_mem(0x100, &desc.encode());
    // Write selector key at address 0x200: 0x0003 in big-endian
    access.write_mem(0x200, &[0x00, 0x03]);

    fw.do_dma(0x100, &access).unwrap();

    // DMA should have written 4 bytes of the entry to address 0x200+0=0x200
    // (the address field is reused: first it's the selector source, then
    //  the data destination for reads)
    let result = access.read_mem(0x200, 4);
    assert_eq!(result, vec![0x40, 0x00, 0x00, 0x00]);
}

#[test]
fn test_fw_cfg_dma_skip() {
    let fw = FwCfg::new(10);
    fw.add_bytes(0x0003, vec![0x01, 0x02, 0x03, 0x04, 0x05]);
    fw.set_selector(0x0003);

    // Skip first 2 bytes
    let access = MockDmaAccess::new();
    let desc = FwCfgDmaDescriptor {
        control: 0x0000_0008, // SKIP only
        length: 2,
        address: 0,
    };
    access.write_mem(0x100, &desc.encode());

    fw.do_dma(0x100, &access).unwrap();

    // Offset advanced by 2, next read gets byte 2
    assert_eq!(fw.read_data_byte(), 0x03);
}

#[test]
fn test_fw_cfg_dma_write_denied() {
    let fw = FwCfg::new(10);
    fw.set_dma_enabled(true);

    let access = MockDmaAccess::new();
    let desc = FwCfgDmaDescriptor {
        control: 0x0000_0010, // WRITE
        length: 4,
        address: 0x1000,
    };
    access.write_mem(0x100, &desc.encode());

    let result = fw.do_dma(0x100, &access);
    assert!(result.is_err());
}

// -- File directory auto-registration --

#[test]
fn test_fw_cfg_file_dir_auto_build() {
    let fw = FwCfg::new(10);
    fw.add_file(0x8000, "etc/test", vec![0x01, 0x02]);

    // Selecting FILE_DIR triggers auto-build
    fw.set_selector(keys::FILE_DIR);
    assert!(fw.has_entry(keys::FILE_DIR));

    // Should be able to read the auto-built directory
    let b0 = fw.read_data_byte();
    assert_eq!(b0, 0); // count hi byte (1 file = 0x00000001 in BE)
}

// -- Negative Tests --

#[test]
fn test_fw_cfg_read_data_byte_no_entry() {
    let fw = FwCfg::new(10);
    fw.set_selector(0xFFFF);
    assert_eq!(fw.read_data_byte(), 0);
}

#[test]
fn test_fw_cfg_read_data_word_no_entry() {
    let fw = FwCfg::new(10);
    fw.set_selector(0xFFFF);
    assert_eq!(fw.read_data_word(), 0);
}

#[test]
fn test_fw_cfg_read_data_dword_no_entry() {
    let fw = FwCfg::new(10);
    fw.set_selector(0xFFFF);
    assert_eq!(fw.read_data_dword(), 0);
}

#[test]
fn test_fw_cfg_read_data_byte_nonexistent_entry() {
    let fw = FwCfg::new(10);
    fw.set_selector(0xFFFF);
    assert_eq!(fw.read_data_byte(), 0);
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
