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
    assert_eq!(entry.data, vec![0x51, 0x45, 0x4D, 0x55]);
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

    assert_eq!(fw.read_data_word(), 0x0102); // BE
    assert_eq!(fw.read_data_word(), 0x0304); // BE
}

#[test]
fn test_fw_cfg_read_data_dword() {
    let fw = FwCfg::new(10);
    fw.add_bytes(0x0004, vec![0x01, 0x02, 0x03, 0x04]);
    fw.set_selector(0x0004);

    assert_eq!(fw.read_data_dword(), 0x01020304);
}

#[test]
fn test_fw_cfg_read_data_word_past_end_padded() {
    let fw = FwCfg::new(10);
    fw.add_bytes(0x0004, vec![0xAA]); // only 1 byte
    fw.set_selector(0x0004);

    // BE: [0xAA] as word = 0xAA00 (right-zero padding)
    assert_eq!(fw.read_data_word(), 0xAA00);
}

#[test]
fn test_fw_cfg_read_data_dword_past_end_padded() {
    let fw = FwCfg::new(10);
    fw.add_bytes(0x0004, vec![0x11, 0x22]); // only 2 bytes
    fw.set_selector(0x0004);

    // BE: [0x11, 0x22] as dword = 0x11220000 (right-zero padding)
    assert_eq!(fw.read_data_dword(), 0x11220000);
}

// -- DMA descriptor tests --

#[test]
fn test_fw_cfg_dma_descriptor_encode_decode() {
    let desc = FwCfgDmaDescriptor {
        control: 0x0000_000A, // SELECT | READ
        length: 0x1000,
        address: 0x8000_0000,
    };
    let bytes = desc.encode();
    assert_eq!(bytes.len(), 16);

    let decoded = FwCfgDmaDescriptor::decode(&bytes).unwrap();
    assert_eq!(decoded.control, 0x0A);
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
            .map(|v| {
                let n = len.min(v.len());
                v[..n].to_vec()
            })
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
    // Descriptor at addr 0x100: SELECT|READ, selector=0x0003 in high 16 bits
    let desc = FwCfgDmaDescriptor {
        control: 0x0003_000A, // SELECT | READ, selector=0x0003
        length: 4,
        address: 0x200, // data transfer address
    };
    access.write_mem(0x100, &desc.encode());

    fw.do_dma(0x100, &access).unwrap();

    // DMA should have written 4 bytes of entry 0x0003 to address 0x200
    let result = access.read_mem(0x200, 4);
    assert_eq!(result, vec![0x40, 0x00, 0x00, 0x00]);

    // Descriptor writeback should clear control to 0 (success)
    let status = access.read_mem(0x100, 4);
    assert_eq!(status, vec![0x00, 0x00, 0x00, 0x00]);
}

#[test]
fn test_fw_cfg_dma_skip() {
    let fw = FwCfg::new(10);
    fw.add_bytes(0x0003, vec![0x01, 0x02, 0x03, 0x04, 0x05]);
    fw.set_selector(0x0003);

    // Skip first 2 bytes
    let access = MockDmaAccess::new();
    let desc = FwCfgDmaDescriptor {
        control: 0x0000_0004, // SKIP only
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

    // Descriptor should have ERROR bit set in control (big-endian)
    let status = access.read_mem(0x100, 4);
    assert_eq!(status, vec![0x00, 0x00, 0x00, 0x01]); // ERROR=0x01
}

#[test]
fn test_fw_cfg_dma_read_past_end_zero_fill() {
    let fw = FwCfg::new(10);
    fw.add_bytes(0x0003, vec![0xAA, 0xBB]); // only 2 bytes
    fw.set_dma_enabled(true);

    let access = MockDmaAccess::new();
    // Read 4 bytes from 2-byte entry: first 2 valid, rest zero-filled
    let desc = FwCfgDmaDescriptor {
        control: 0x0003_000A, // SELECT | READ, selector=0x0003
        length: 4,
        address: 0x200,
    };
    access.write_mem(0x100, &desc.encode());

    fw.do_dma(0x100, &access).unwrap();

    let result = access.read_mem(0x200, 4);
    assert_eq!(result, vec![0xAA, 0xBB, 0x00, 0x00]);
}

#[test]
fn test_fw_cfg_dma_read_no_entry_zero_fill() {
    let fw = FwCfg::new(10);
    // No entry at key 0xDEAD
    fw.set_dma_enabled(true);

    let access = MockDmaAccess::new();
    let desc = FwCfgDmaDescriptor {
        control: 0xDEAD_000A, // SELECT | READ, selector=0xDEAD
        length: 4,
        address: 0x300,
    };
    access.write_mem(0x100, &desc.encode());

    fw.do_dma(0x100, &access).unwrap();

    // Should zero-fill when no entry exists
    let result = access.read_mem(0x300, 4);
    assert_eq!(result, vec![0x00, 0x00, 0x00, 0x00]);
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
