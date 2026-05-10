// Firmware / kernel loader utilities.

use machina_core::address::GPA;
use machina_memory::AddressSpace;

/// Information returned after a successful load.
#[derive(Debug)]
pub struct LoadInfo {
    /// Entry point address.
    pub entry: GPA,
    /// Total bytes loaded.
    pub size: u64,
    /// Lowest guest physical address written.
    pub low_addr: u64,
    /// Highest guest physical address written
    /// (load_addr + p_memsz of the last segment).
    pub high_addr: u64,
    /// Load bias applied for ET_DYN images (None for ET_EXEC).
    pub bias: Option<u64>,
}

/// Load a raw binary image at the given guest physical address.
pub fn load_binary(
    data: &[u8],
    addr: GPA,
    as_: &AddressSpace,
) -> Result<LoadInfo, String> {
    write_bytes(as_, addr, data);
    let end = addr.0 + data.len() as u64;
    Ok(LoadInfo {
        entry: addr,
        size: data.len() as u64,
        low_addr: addr.0,
        high_addr: end,
        bias: None,
    })
}

/// Write `data` into `as_` starting at `base`, using 4-byte
/// writes for aligned chunks and single-byte writes for the
/// trailing remainder so that no bytes beyond `data.len()`
/// are overwritten.
fn write_bytes(as_: &AddressSpace, base: GPA, data: &[u8]) {
    let full = data.len() / 4;
    for i in 0..full {
        let off = (i * 4) as u64;
        let val =
            u32::from_le_bytes(data[i * 4..i * 4 + 4].try_into().unwrap());
        as_.write_u32(GPA::new(base.0 + off), val);
    }
    let rem_start = full * 4;
    for (j, &b) in data[rem_start..].iter().enumerate() {
        let off = (rem_start + j) as u64;
        as_.write(GPA::new(base.0 + off), 1, b as u64);
    }
}

const EI_MAG: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
const PT_LOAD: u32 = 1;
const SHT_SYMTAB: u32 = 2;

const ELF64_EHDR_SIZE: usize = 64;
const ELF64_PHDR_SIZE: usize = 56;
const ELF64_SHDR_SIZE: usize = 64;
const ELF64_SYM_SIZE: usize = 24;

struct ElfHeader {
    e_type: u16,
    e_entry: u64,
    e_phoff: usize,
    e_phentsize: usize,
    e_phnum: usize,
}

fn parse_elf_header(data: &[u8]) -> Result<ElfHeader, String> {
    if data.len() < ELF64_EHDR_SIZE {
        return Err("data too small for ELF header".into());
    }
    if data[0..4] != EI_MAG {
        return Err("bad ELF magic".into());
    }
    if data[4] != ELFCLASS64 {
        return Err("not ELF-64".into());
    }

    let e_type = u16::from_le_bytes(data[16..18].try_into().unwrap());
    if e_type != ET_EXEC && e_type != ET_DYN {
        return Err(format!(
            "unsupported ELF type {e_type} \
             (need ET_EXEC or ET_DYN)"
        ));
    }

    let e_entry = u64::from_le_bytes(data[24..32].try_into().unwrap());
    let e_phoff = u64::from_le_bytes(data[32..40].try_into().unwrap()) as usize;
    let e_phentsize =
        u16::from_le_bytes(data[54..56].try_into().unwrap()) as usize;
    let e_phnum = u16::from_le_bytes(data[56..58].try_into().unwrap()) as usize;

    if e_phentsize < ELF64_PHDR_SIZE {
        return Err(format!("phentsize {e_phentsize} < {ELF64_PHDR_SIZE}"));
    }

    Ok(ElfHeader {
        e_type,
        e_entry,
        e_phoff,
        e_phentsize,
        e_phnum,
    })
}

/// Pre-validated PT_LOAD descriptor produced by the first pass
/// of `load_elf`. Once one of these exists every offset is known
/// to be in bounds and every address sum is known not to wrap, so
/// the second pass can write without re-checking.
struct PtLoadSeg {
    load_addr: u64,
    p_offset: usize,
    p_filesz: usize,
    p_memsz: u64,
}

/// Load an ELF-64 binary into the address space and return
/// the entry point.
///
/// For ET_EXEC segments are loaded at their `p_paddr`.
/// For ET_DYN (PIE) segments are loaded relative to
/// `base_addr`.  The `LoadInfo.bias` field carries the
/// offset so the caller can relocate the entry address.
///
/// All PT_LOAD segments are validated in a first pass — header
/// bounds, file-data bounds, `p_filesz <= p_memsz`, and address
/// arithmetic are all checked — before any byte is written, so a
/// malformed ELF cannot leave guest memory in a partially loaded
/// state.
pub fn load_elf(
    data: &[u8],
    base_addr: u64,
    as_: &AddressSpace,
) -> Result<LoadInfo, String> {
    let hdr = parse_elf_header(data)?;

    let is_dyn = hdr.e_type == ET_DYN;

    let mut segs: Vec<PtLoadSeg> = Vec::new();
    let mut total_memsz: u64 = 0;
    let mut low_addr: u64 = u64::MAX;
    let mut high_addr: u64 = 0;

    // Pass 1: parse and validate every PT_LOAD segment.
    for i in 0..hdr.e_phnum {
        let stride = i
            .checked_mul(hdr.e_phentsize)
            .ok_or_else(|| format!("phdr {i} offset arithmetic overflow"))?;
        let off = hdr
            .e_phoff
            .checked_add(stride)
            .ok_or_else(|| format!("phdr {i} offset arithmetic overflow"))?;
        let off_end = off
            .checked_add(ELF64_PHDR_SIZE)
            .ok_or_else(|| format!("phdr {i} extends past usize"))?;
        if off_end > data.len() {
            return Err(format!("phdr {i} out of bounds"));
        }

        let p_type = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
        if p_type != PT_LOAD {
            continue;
        }

        let p_offset_u64 =
            u64::from_le_bytes(data[off + 8..off + 16].try_into().unwrap());
        let p_vaddr =
            u64::from_le_bytes(data[off + 16..off + 24].try_into().unwrap());
        let p_paddr =
            u64::from_le_bytes(data[off + 24..off + 32].try_into().unwrap());
        let p_filesz_u64 =
            u64::from_le_bytes(data[off + 32..off + 40].try_into().unwrap());
        let p_memsz =
            u64::from_le_bytes(data[off + 40..off + 48].try_into().unwrap());

        if p_filesz_u64 > p_memsz {
            return Err(format!("PT_LOAD segment {i}: p_filesz > p_memsz"));
        }

        let p_offset = usize::try_from(p_offset_u64).map_err(|_| {
            format!("PT_LOAD segment {i}: p_offset exceeds usize")
        })?;
        let p_filesz = usize::try_from(p_filesz_u64).map_err(|_| {
            format!("PT_LOAD segment {i}: p_filesz exceeds usize")
        })?;
        let file_end = p_offset.checked_add(p_filesz).ok_or_else(|| {
            format!("PT_LOAD segment {i}: file range arithmetic overflow")
        })?;
        if file_end > data.len() {
            return Err(format!(
                "PT_LOAD segment {i}: segment file data out of bounds"
            ));
        }

        let load_addr = if is_dyn {
            base_addr.checked_add(p_vaddr).ok_or_else(|| {
                format!("PT_LOAD segment {i}: base_addr + p_vaddr overflow")
            })?
        } else {
            p_paddr
        };
        let seg_end = load_addr.checked_add(p_memsz).ok_or_else(|| {
            format!("PT_LOAD segment {i}: load_addr + p_memsz overflow")
        })?;

        if load_addr < low_addr {
            low_addr = load_addr;
        }
        if seg_end > high_addr {
            high_addr = seg_end;
        }
        total_memsz = total_memsz.checked_add(p_memsz).ok_or_else(|| {
            format!("PT_LOAD segment {i}: total memsz overflow")
        })?;

        segs.push(PtLoadSeg {
            load_addr,
            p_offset,
            p_filesz,
            p_memsz,
        });
    }

    // Pass 2: write all validated segments. Arithmetic here cannot
    // overflow because pass 1 already vetted every address.
    for s in &segs {
        let seg = &data[s.p_offset..s.p_offset + s.p_filesz];
        write_bytes(as_, GPA::new(s.load_addr), seg);

        // BSS: zero-fill [p_filesz .. p_memsz)
        let bss_start = s.load_addr + s.p_filesz as u64;
        let bss_end = s.load_addr + s.p_memsz;
        let mut cur = bss_start;
        while cur < bss_end {
            let remain = bss_end - cur;
            if remain >= 4 {
                as_.write_u32(GPA::new(cur), 0);
                cur += 4;
            } else {
                as_.write(GPA::new(cur), 1, 0);
                cur += 1;
            }
        }
    }

    // For ET_DYN the actual entry = base_addr + e_entry.
    let entry = if is_dyn {
        base_addr
            .checked_add(hdr.e_entry)
            .ok_or_else(|| "entry address overflow".to_string())?
    } else {
        hdr.e_entry
    };

    let bias = if is_dyn { Some(base_addr) } else { None };

    Ok(LoadInfo {
        entry: GPA::new(entry),
        size: total_memsz,
        low_addr: if low_addr == u64::MAX { 0 } else { low_addr },
        high_addr,
        bias,
    })
}

/// Check if an ELF-64 binary is ET_DYN (position-independent).
pub fn elf_is_dyn(data: &[u8]) -> bool {
    if data.len() < ELF64_EHDR_SIZE {
        return false;
    }
    parse_elf_header(data)
        .map(|h| h.e_type == ET_DYN)
        .unwrap_or(false)
}

/// Find a named symbol in an ELF-64 binary and return its
/// value (address).  Returns `None` if the symbol is not
/// found or the ELF lacks a symbol table.
pub fn elf_find_symbol(data: &[u8], name: &str) -> Option<u64> {
    if data.len() < ELF64_EHDR_SIZE
        || data[0..4] != EI_MAG
        || data[4] != ELFCLASS64
    {
        return None;
    }

    let e_shoff = u64::from_le_bytes(data[40..48].try_into().unwrap()) as usize;
    let e_shentsize =
        u16::from_le_bytes(data[58..60].try_into().unwrap()) as usize;
    let e_shnum = u16::from_le_bytes(data[60..62].try_into().unwrap()) as usize;

    if e_shentsize < ELF64_SHDR_SIZE {
        return None;
    }

    // Walk section headers to find SHT_SYMTAB.
    for i in 0..e_shnum {
        let sh = e_shoff + i * e_shentsize;
        if sh + ELF64_SHDR_SIZE > data.len() {
            break;
        }

        let sh_type =
            u32::from_le_bytes(data[sh + 4..sh + 8].try_into().unwrap());
        if sh_type != SHT_SYMTAB {
            continue;
        }

        // ELF64_Shdr layout:
        //   0: sh_name(4), 4: sh_type(4),
        //   8: sh_flags(8), 16: sh_addr(8),
        //  24: sh_offset(8), 32: sh_size(8),
        //  40: sh_link(4), 44: sh_info(4),
        //  48: sh_addralign(8), 56: sh_entsize(8)
        let sym_offset =
            u64::from_le_bytes(data[sh + 24..sh + 32].try_into().unwrap())
                as usize;
        let sym_size =
            u64::from_le_bytes(data[sh + 32..sh + 40].try_into().unwrap())
                as usize;
        let strtab_idx =
            u32::from_le_bytes(data[sh + 40..sh + 44].try_into().unwrap())
                as usize;
        let sym_entsize =
            u64::from_le_bytes(data[sh + 56..sh + 64].try_into().unwrap())
                as usize;
        let ent = if sym_entsize >= ELF64_SYM_SIZE {
            sym_entsize
        } else {
            ELF64_SYM_SIZE
        };

        // Locate the string table section.
        let str_sh = e_shoff + strtab_idx * e_shentsize;
        if str_sh + ELF64_SHDR_SIZE > data.len() {
            return None;
        }
        let str_off = u64::from_le_bytes(
            data[str_sh + 24..str_sh + 32].try_into().unwrap(),
        ) as usize;
        let str_size = u64::from_le_bytes(
            data[str_sh + 32..str_sh + 40].try_into().unwrap(),
        ) as usize;

        // Iterate symbols.
        let nsym = sym_size / ent;
        for j in 0..nsym {
            let s = sym_offset + j * ent;
            if s + ELF64_SYM_SIZE > data.len() {
                break;
            }
            // Elf64_Sym: st_name(4), st_info(1),
            //   st_other(1), st_shndx(2), st_value(8),
            //   st_size(8)
            let st_name =
                u32::from_le_bytes(data[s..s + 4].try_into().unwrap()) as usize;
            let st_value =
                u64::from_le_bytes(data[s + 8..s + 16].try_into().unwrap());

            // Resolve name from strtab.
            let name_start = str_off + st_name;
            if name_start >= str_off + str_size {
                continue;
            }
            let name_end = data[name_start..]
                .iter()
                .position(|&b| b == 0)
                .map(|p| name_start + p)
                .unwrap_or(data.len());
            let sym_name =
                std::str::from_utf8(&data[name_start..name_end]).unwrap_or("");
            if sym_name == name {
                return Some(st_value);
            }
        }
    }

    None
}

/// Convert an ET_EXEC virtual entry address to its physical
/// counterpart by applying the entry's PT_LOAD segment offset
/// to p_paddr.
pub fn elf_phys_entry(data: &[u8], virt_entry: u64) -> Option<u64> {
    let hdr = parse_elf_header(data).ok()?;
    if hdr.e_type == ET_DYN {
        return None;
    }
    for i in 0..hdr.e_phnum {
        let stride = i.checked_mul(hdr.e_phentsize)?;
        let off = hdr.e_phoff.checked_add(stride)?;
        let off_end = off.checked_add(ELF64_PHDR_SIZE)?;
        if off_end > data.len() {
            return None;
        }
        let p_type = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
        if p_type != PT_LOAD {
            continue;
        }
        let p_vaddr =
            u64::from_le_bytes(data[off + 16..off + 24].try_into().unwrap());
        let p_paddr =
            u64::from_le_bytes(data[off + 24..off + 32].try_into().unwrap());
        let p_memsz =
            u64::from_le_bytes(data[off + 40..off + 48].try_into().unwrap());
        if virt_entry >= p_vaddr && virt_entry < p_vaddr.saturating_add(p_memsz)
        {
            let offset = virt_entry.checked_sub(p_vaddr)?;
            return p_paddr.checked_add(offset);
        }
    }
    None
}
