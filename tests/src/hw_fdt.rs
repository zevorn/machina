use machina_hw_core::fdt::FdtBuilder;

/// Read a big-endian u32 from a byte slice at `offset`.
fn read_be_u32(blob: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(blob[offset..offset + 4].try_into().unwrap())
}

#[test]
fn test_fdt_magic() {
    let mut fdt = FdtBuilder::new();
    fdt.begin_node("");
    fdt.end_node();
    let blob = fdt.finish();
    assert_eq!(read_be_u32(&blob, 0), 0xd00d_feed);
}

#[test]
fn test_fdt_simple_node() {
    let mut fdt = FdtBuilder::new();
    fdt.begin_node("");
    fdt.property_u32("#address-cells", 2);
    fdt.property_u32("#size-cells", 2);
    fdt.begin_node("memory@80000000");
    fdt.property_string("device_type", "memory");
    fdt.end_node();
    fdt.end_node();
    let blob = fdt.finish();

    // Verify header fields.
    let magic = read_be_u32(&blob, 0);
    assert_eq!(magic, 0xd00d_feed);

    let totalsize = read_be_u32(&blob, 4) as usize;
    assert_eq!(totalsize, blob.len());

    let version = read_be_u32(&blob, 20);
    assert_eq!(version, 17);
}

#[test]
fn test_fdt_string_property() {
    let mut fdt = FdtBuilder::new();
    fdt.begin_node("");
    fdt.property_string("compatible", "riscv-virtio");
    fdt.end_node();
    let blob = fdt.finish();

    // The string "riscv-virtio\0" must appear in the
    // structure block as the property value.
    let needle = b"riscv-virtio\0";
    let found = blob.windows(needle.len()).any(|w| w == needle);
    assert!(found, "string property value must be null-terminated");
}

// Header field offsets within the FDT header (FDT_VERSION 17).
const HDR_TOTALSIZE: usize = 4;
const HDR_OFF_DT_STRUCT: usize = 8;
const HDR_OFF_DT_STRINGS: usize = 12;
const HDR_SIZE_DT_STRINGS: usize = 32;
const HDR_SIZE_DT_STRUCT: usize = 36;

/// Count non-overlapping occurrences of `needle` in `haystack`.
fn count_occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    let mut count = 0;
    let mut i = 0;
    while i + needle.len() <= haystack.len() {
        if &haystack[i..i + needle.len()] == needle {
            count += 1;
            i += needle.len();
        } else {
            i += 1;
        }
    }
    count
}

#[test]
fn test_fdt_header_offsets_and_sizes_match_blob() {
    // The header advertises blob layout. Verify each offset and
    // size matches the actual blob length and that the strings block
    // ends exactly at totalsize.
    let mut fdt = FdtBuilder::new();
    fdt.begin_node("");
    fdt.property_u32("#address-cells", 2);
    fdt.property_u32("#size-cells", 2);
    fdt.begin_node("memory@80000000");
    fdt.property_string("device_type", "memory");
    fdt.property_u64("reg", 0x8000_0000);
    fdt.end_node();
    fdt.end_node();
    let blob = fdt.finish();

    let totalsize = read_be_u32(&blob, HDR_TOTALSIZE) as usize;
    let off_struct = read_be_u32(&blob, HDR_OFF_DT_STRUCT) as usize;
    let off_strings = read_be_u32(&blob, HDR_OFF_DT_STRINGS) as usize;
    let size_struct = read_be_u32(&blob, HDR_SIZE_DT_STRUCT) as usize;
    let size_strings = read_be_u32(&blob, HDR_SIZE_DT_STRINGS) as usize;

    assert_eq!(totalsize, blob.len(), "totalsize must equal blob.len()");
    assert_eq!(
        off_struct + size_struct,
        off_strings,
        "structure block ends where strings block starts",
    );
    assert_eq!(
        off_strings + size_strings,
        totalsize,
        "strings block ends at totalsize",
    );
}

#[test]
fn test_fdt_struct_block_is_4_byte_aligned_after_various_lengths() {
    // Property values of 1, 2, 3, 5, and 7 bytes each force a
    // different amount of trailing padding in the structure block.
    // Regardless of payload length the block size must remain a
    // multiple of 4 because every FDT token is u32-sized.
    let mut fdt = FdtBuilder::new();
    fdt.begin_node("");
    fdt.property_bytes("p1", &[0xAA]);
    fdt.property_bytes("p2", &[0xAA, 0xBB]);
    fdt.property_bytes("p3", &[0xAA, 0xBB, 0xCC]);
    fdt.property_bytes("p5", &[0xAA, 0xBB, 0xCC, 0xDD, 0xEE]);
    fdt.property_bytes("p7", &[0; 7]);
    fdt.end_node();
    let blob = fdt.finish();

    let off_struct = read_be_u32(&blob, HDR_OFF_DT_STRUCT) as usize;
    let size_struct = read_be_u32(&blob, HDR_SIZE_DT_STRUCT) as usize;

    assert_eq!(off_struct % 4, 0, "off_dt_struct must be 4-byte aligned",);
    assert_eq!(
        size_struct % 4,
        0,
        "size_dt_struct ({size_struct}) must be a multiple of 4",
    );
}

#[test]
fn test_fdt_property_name_is_deduped_in_strings_block() {
    // Two `compatible` properties on two different nodes should
    // share the same nameoff in the strings block: the bytes
    // "compatible\0" must appear exactly once.
    let mut fdt = FdtBuilder::new();
    fdt.begin_node("");
    fdt.property_string("compatible", "vendor,root-1.0");
    fdt.begin_node("child");
    fdt.property_string("compatible", "vendor,child-1.0");
    fdt.end_node();
    fdt.end_node();
    let blob = fdt.finish();

    let off_strings = read_be_u32(&blob, HDR_OFF_DT_STRINGS) as usize;
    let size_strings = read_be_u32(&blob, HDR_SIZE_DT_STRINGS) as usize;
    let strings = &blob[off_strings..off_strings + size_strings];

    assert_eq!(
        count_occurrences(strings, b"compatible\0"),
        1,
        "property names must be interned (compatible should appear once)",
    );
}

#[test]
fn test_fdt_property_u32_list_is_big_endian() {
    // property_u32_list must serialise each value in big-endian
    // order so devicetree consumers parse them correctly.
    let mut fdt = FdtBuilder::new();
    fdt.begin_node("");
    fdt.property_u32_list("foo", &[0xAABB_CCDD, 0x1122_3344]);
    fdt.end_node();
    let blob = fdt.finish();

    let needle = [0xAA, 0xBB, 0xCC, 0xDD, 0x11, 0x22, 0x33, 0x44];
    let found = blob.windows(needle.len()).any(|w| w == needle);
    assert!(
        found,
        "property_u32_list must emit values in big-endian order",
    );
}
