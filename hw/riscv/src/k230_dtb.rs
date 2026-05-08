use machina_hw_core::fdt::FdtBuilder;

const FDT_MAGIC: u32 = 0xd00d_feed;
const FDT_BEGIN_NODE: u32 = 0x0000_0001;
const FDT_END_NODE: u32 = 0x0000_0002;
const FDT_PROP: u32 = 0x0000_0003;
const FDT_NOP: u32 = 0x0000_0004;
const FDT_END: u32 = 0x0000_0009;

#[derive(Clone)]
struct FdtProp {
    name: String,
    data: Vec<u8>,
}

#[derive(Clone)]
struct FdtNode {
    name: String,
    props: Vec<FdtProp>,
    children: Vec<FdtNode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdtReservation {
    pub address: u64,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdtMemoryRegion {
    pub base: u64,
    pub size: u64,
}

struct FdtBlocks<'a> {
    structure: &'a [u8],
    strings: &'a [u8],
}

pub fn fixup_k230_dtb(
    blob: &[u8],
    initrd: Option<(u64, u64)>,
    cmdline: Option<&str>,
) -> Result<Vec<u8>, String> {
    let reservations = dtb_mem_reservations(blob)?;
    let mut root = parse_dtb(blob)?;

    let chosen = ensure_child(&mut root, "chosen");
    if let Some(cmdline) = cmdline {
        set_string_prop(chosen, "bootargs", cmdline);
    }
    if let Some((start, end)) = initrd {
        set_u64_prop(chosen, "linux,initrd-start", start);
        set_u64_prop(chosen, "linux,initrd-end", end);
    }

    for path in ["/soc/sdhci0@91580000", "/soc/sdhci1@91581000"] {
        if let Some(node) = find_node_mut(&mut root, path) {
            set_string_prop(node, "status", "disabled");
        }
    }

    let mut builder = FdtBuilder::new();
    for reservation in reservations {
        builder.reserve_memory(reservation.address, reservation.size);
    }
    emit_node(&mut builder, &root);
    Ok(builder.finish())
}

pub fn dtb_node_status(
    blob: &[u8],
    path: &str,
) -> Result<Option<String>, String> {
    let root = parse_dtb(blob)?;
    let Some(node) = find_node(&root, path) else {
        return Ok(None);
    };
    let Some(prop) = node.props.iter().find(|prop| prop.name == "status")
    else {
        return Ok(None);
    };
    Ok(Some(prop_as_string(&prop.data)?))
}

pub fn dtb_chosen_bootargs(blob: &[u8]) -> Result<Option<String>, String> {
    let root = parse_dtb(blob)?;
    let Some(node) = find_node(&root, "/chosen") else {
        return Ok(None);
    };
    let Some(prop) = node.props.iter().find(|prop| prop.name == "bootargs")
    else {
        return Ok(None);
    };
    Ok(Some(prop_as_string(&prop.data)?))
}

pub fn dtb_mem_reservations(
    blob: &[u8],
) -> Result<Vec<FdtReservation>, String> {
    let off_mem_rsvmap = read_be32(blob, 16)? as usize;
    let mut reservations = Vec::new();
    let mut cursor = off_mem_rsvmap;

    loop {
        let address = read_be64(blob, cursor)?;
        let size_offset = cursor
            .checked_add(8)
            .ok_or("DTB reservation map offset overflow")?;
        let size = read_be64(blob, size_offset)?;
        cursor = cursor
            .checked_add(16)
            .ok_or("DTB reservation map offset overflow")?;
        if address == 0 && size == 0 {
            break;
        }
        reservations.push(FdtReservation { address, size });
    }

    Ok(reservations)
}

pub fn dtb_first_memory_region(
    blob: &[u8],
) -> Result<Option<FdtMemoryRegion>, String> {
    let root = parse_dtb(blob)?;
    let address_cells = node_u32_prop(&root, "#address-cells")?.unwrap_or(2);
    let size_cells = node_u32_prop(&root, "#size-cells")?.unwrap_or(1);
    if !(1..=2).contains(&address_cells) || !(1..=2).contains(&size_cells) {
        return Err("unsupported DTB memory address or size cells".into());
    }

    find_first_memory_region(&root, address_cells, size_cells)
}

pub fn test_fixture_dtb_with_sdhci_nodes() -> Vec<u8> {
    test_fixture_dtb_with_sdhci_nodes_and_bootargs("")
}

pub fn test_fixture_dtb_with_sdhci_nodes_and_bootargs(
    bootargs: &str,
) -> Vec<u8> {
    test_fixture_dtb_with_sdhci_nodes_bootargs_and_reservations(bootargs, &[])
}

pub fn test_fixture_dtb_with_sdhci_nodes_bootargs_and_reservations(
    bootargs: &str,
    reservations: &[FdtReservation],
) -> Vec<u8> {
    let mut fdt = FdtBuilder::new();
    for reservation in reservations {
        fdt.reserve_memory(reservation.address, reservation.size);
    }
    fdt.begin_node("");
    fdt.property_u32("#address-cells", 2);
    fdt.property_u32("#size-cells", 2);

    fdt.begin_node("chosen");
    fdt.property_string("bootargs", bootargs);
    fdt.end_node();

    fdt.begin_node("soc");
    fdt.property_u32("#address-cells", 2);
    fdt.property_u32("#size-cells", 2);
    fdt.property_bytes("ranges", &[]);

    fdt.begin_node("sdhci0@91580000");
    fdt.property_string("status", "okay");
    fdt.end_node();

    fdt.begin_node("sdhci1@91581000");
    fdt.property_string("status", "okay");
    fdt.end_node();

    fdt.end_node();
    fdt.end_node();
    fdt.finish()
}

fn parse_dtb(blob: &[u8]) -> Result<FdtNode, String> {
    let magic = read_be32(blob, 0)?;
    if magic != FDT_MAGIC {
        return Err("invalid DTB magic".to_string());
    }
    let off_dt_struct = read_be32(blob, 8)? as usize;
    let off_dt_strings = read_be32(blob, 12)? as usize;
    let size_dt_strings = read_be32(blob, 32)? as usize;
    let size_dt_struct = read_be32(blob, 36)? as usize;

    let structure = checked_slice(blob, off_dt_struct, size_dt_struct)?;
    let strings = checked_slice(blob, off_dt_strings, size_dt_strings)?;
    let blocks = FdtBlocks { structure, strings };
    let mut cursor = 0usize;
    let token = read_token(&blocks, &mut cursor)?;
    if token != FDT_BEGIN_NODE {
        return Err("DTB structure does not start with root node".to_string());
    }
    parse_node_after_begin(&blocks, &mut cursor)
}

fn parse_node_after_begin(
    blocks: &FdtBlocks<'_>,
    cursor: &mut usize,
) -> Result<FdtNode, String> {
    let name = read_struct_string(blocks.structure, cursor)?;
    let mut node = FdtNode {
        name,
        props: Vec::new(),
        children: Vec::new(),
    };

    loop {
        let token = read_token(blocks, cursor)?;
        match token {
            FDT_BEGIN_NODE => {
                node.children.push(parse_node_after_begin(blocks, cursor)?);
            }
            FDT_END_NODE => return Ok(node),
            FDT_PROP => {
                let len = read_token(blocks, cursor)? as usize;
                let nameoff = read_token(blocks, cursor)? as usize;
                let name = read_string_block(blocks.strings, nameoff)?;
                let data = checked_slice(blocks.structure, *cursor, len)?;
                node.props.push(FdtProp {
                    name,
                    data: data.to_vec(),
                });
                *cursor = align4(*cursor + len);
            }
            FDT_NOP => {}
            FDT_END => return Ok(node),
            other => return Err(format!("unsupported FDT token {other:#x}")),
        }
    }
}

fn emit_node(builder: &mut FdtBuilder, node: &FdtNode) {
    builder.begin_node(&node.name);
    for prop in &node.props {
        builder.property_bytes(&prop.name, &prop.data);
    }
    for child in &node.children {
        emit_node(builder, child);
    }
    builder.end_node();
}

fn find_node<'a>(node: &'a FdtNode, path: &str) -> Option<&'a FdtNode> {
    let mut current = node;
    for part in path.split('/').filter(|part| !part.is_empty()) {
        current = current.children.iter().find(|child| child.name == part)?;
    }
    Some(current)
}

fn find_node_mut<'a>(
    node: &'a mut FdtNode,
    path: &str,
) -> Option<&'a mut FdtNode> {
    let mut current = node;
    for part in path.split('/').filter(|part| !part.is_empty()) {
        current = current
            .children
            .iter_mut()
            .find(|child| child.name == part)?;
    }
    Some(current)
}

fn find_first_memory_region(
    node: &FdtNode,
    address_cells: u32,
    size_cells: u32,
) -> Result<Option<FdtMemoryRegion>, String> {
    if node.name.split('@').next() == Some("memory") {
        if let Some(prop) = node.props.iter().find(|prop| prop.name == "reg") {
            let tuple_cells = address_cells + size_cells;
            if prop.data.len() < tuple_cells as usize * 4 {
                return Ok(None);
            }
            let mut cursor = 0;
            let base = read_cells(&prop.data, &mut cursor, address_cells)?;
            let size = read_cells(&prop.data, &mut cursor, size_cells)?;
            if size != 0 {
                return Ok(Some(FdtMemoryRegion { base, size }));
            }
        }
    }

    for child in &node.children {
        if let Some(region) =
            find_first_memory_region(child, address_cells, size_cells)?
        {
            return Ok(Some(region));
        }
    }

    Ok(None)
}

fn node_u32_prop(node: &FdtNode, name: &str) -> Result<Option<u32>, String> {
    let Some(prop) = node.props.iter().find(|prop| prop.name == name) else {
        return Ok(None);
    };
    if prop.data.len() != 4 {
        return Err(format!("DTB property {name} must be a u32"));
    }
    Ok(Some(u32::from_be_bytes(prop.data[..4].try_into().unwrap())))
}

fn read_cells(
    data: &[u8],
    cursor: &mut usize,
    cells: u32,
) -> Result<u64, String> {
    let mut value = 0u64;
    for _ in 0..cells {
        let word = checked_slice(data, *cursor, 4)?;
        value =
            (value << 32) | u32::from_be_bytes(word.try_into().unwrap()) as u64;
        *cursor = cursor
            .checked_add(4)
            .ok_or("DTB memory reg offset overflow")?;
    }
    Ok(value)
}

fn ensure_child<'a>(node: &'a mut FdtNode, name: &str) -> &'a mut FdtNode {
    if let Some(index) =
        node.children.iter().position(|child| child.name == name)
    {
        return &mut node.children[index];
    }
    node.children.push(FdtNode {
        name: name.to_string(),
        props: Vec::new(),
        children: Vec::new(),
    });
    node.children.last_mut().unwrap()
}

fn set_string_prop(node: &mut FdtNode, name: &str, value: &str) {
    let mut data = value.as_bytes().to_vec();
    data.push(0);
    set_prop(node, name, data);
}

fn set_u64_prop(node: &mut FdtNode, name: &str, value: u64) {
    set_prop(node, name, value.to_be_bytes().to_vec());
}

fn set_prop(node: &mut FdtNode, name: &str, data: Vec<u8>) {
    if let Some(prop) = node.props.iter_mut().find(|prop| prop.name == name) {
        prop.data = data;
    } else {
        node.props.push(FdtProp {
            name: name.to_string(),
            data,
        });
    }
}

fn prop_as_string(data: &[u8]) -> Result<String, String> {
    let end = data
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(data.len());
    std::str::from_utf8(&data[..end])
        .map(|value| value.to_string())
        .map_err(|error| format!("invalid DTB string property: {error}"))
}

fn read_be32(data: &[u8], offset: usize) -> Result<u32, String> {
    let bytes = checked_slice(data, offset, 4)?;
    Ok(u32::from_be_bytes(bytes.try_into().unwrap()))
}

fn read_be64(data: &[u8], offset: usize) -> Result<u64, String> {
    let bytes = checked_slice(data, offset, 8)?;
    Ok(u64::from_be_bytes(bytes.try_into().unwrap()))
}

fn read_token(
    blocks: &FdtBlocks<'_>,
    cursor: &mut usize,
) -> Result<u32, String> {
    let value = read_be32(blocks.structure, *cursor)?;
    *cursor += 4;
    Ok(value)
}

fn read_struct_string(
    data: &[u8],
    cursor: &mut usize,
) -> Result<String, String> {
    let start = *cursor;
    let rest = data
        .get(start..)
        .ok_or("DTB string cursor out of range".to_string())?;
    let len = rest
        .iter()
        .position(|&byte| byte == 0)
        .ok_or("unterminated DTB node name".to_string())?;
    let name = std::str::from_utf8(&rest[..len])
        .map_err(|error| format!("invalid DTB node name: {error}"))?
        .to_string();
    *cursor = align4(start + len + 1);
    Ok(name)
}

fn read_string_block(strings: &[u8], offset: usize) -> Result<String, String> {
    let rest = strings
        .get(offset..)
        .ok_or("DTB string offset out of range".to_string())?;
    let len = rest
        .iter()
        .position(|&byte| byte == 0)
        .ok_or("unterminated DTB property name".to_string())?;
    std::str::from_utf8(&rest[..len])
        .map(|value| value.to_string())
        .map_err(|error| format!("invalid DTB property name: {error}"))
}

fn checked_slice(
    data: &[u8],
    offset: usize,
    len: usize,
) -> Result<&[u8], String> {
    let end = offset
        .checked_add(len)
        .ok_or("DTB slice range overflows".to_string())?;
    data.get(offset..end)
        .ok_or("DTB slice range out of bounds".to_string())
}

fn align4(value: usize) -> usize {
    (value + 3) & !3
}
