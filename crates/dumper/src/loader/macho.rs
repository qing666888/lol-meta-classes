use anyhow::bail;
use goblin::mach::bind_opcodes::BIND_TYPE_POINTER;
use goblin::mach::constants::*;
use goblin::mach::{load_command::CommandVariant, MachO};
use goblin::mach::exports::ExportInfo;
use mmap::{MapOption, MemoryMap};
use scroll::{Pread, Uleb128};

// Chained fixups pointer format constants
#[allow(dead_code)]
const DYLD_CHAINED_PTR_ARM64E: u16 = 1;
const DYLD_CHAINED_PTR_64: u16 = 2;
const DYLD_CHAINED_PTR_32: u16 = 3;
const DYLD_CHAINED_PTR_32_CACHE: u16 = 4;
const DYLD_CHAINED_PTR_32_FIRMWARE: u16 = 5;
const DYLD_CHAINED_PTR_64_OFFSET: u16 = 6;
#[allow(dead_code)]
const DYLD_CHAINED_PTR_ARM64E_KERNEL: u16 = 7;
#[allow(dead_code)]
const DYLD_CHAINED_PTR_64_KERNEL_CACHE: u16 = 8;
const DYLD_CHAINED_PTR_ARM64E_USERLAND: u16 = 9;
#[allow(dead_code)]
const DYLD_CHAINED_PTR_ARM64E_FIRMWARE: u16 = 10;
#[allow(dead_code)]
const DYLD_CHAINED_PTR_X86_64_KERNEL_CACHE: u16 = 11;
const DYLD_CHAINED_PTR_ARM64E_USERLAND24: u16 = 12;

const DYLD_CHAINED_PTR_START_NONE: u16 = 0xFFFF;
const DYLD_CHAINED_PTR_START_MULTI: u16 = 0x8000;

/// Header for chained fixups data
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct DyldChainedFixupsHeader {
    fixups_version: u32,
    starts_offset: u32,
    imports_offset: u32,
    symbols_offset: u32,
    imports_count: u32,
    imports_format: u32,
    symbols_format: u32,
}

/// Per-segment chained starts info
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct DyldChainedStartsInSegment {
    size: u32,
    page_size: u16,
    pointer_format: u16,
    segment_offset: u64,
    max_valid_pointer: u32,
    page_count: u16,
    // page_start array follows
}

/// Rebase entry from chained fixups
#[derive(Debug, Clone)]
pub struct ChainedRebase {
    pub vmaddr: u64,
    pub target: u64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MachORebase {
    pub kind: u8,
    pub vmaddr: u64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MachOImport {
    pub name: String,
    pub dylib: String,
    pub offset: u64,
    pub size: usize,
    pub address: u64,
    pub addend: i64,
    pub is_lazy: bool,
    pub is_weak: bool,
}

#[derive(Debug, Clone)]
pub enum MachOExportKind {
    Regular {
        address: u64
    },
    ReExport {
        lib: String,
        lib_symbol_name: Option<String>,
    },
    Stub {
        stub_offset: Uleb128,
        resolver_offset: Uleb128,
    },
}

#[derive(Debug, Clone)]
pub struct MachOExport {
    pub name: String,
    pub kind: MachOExportKind,
    pub flags: u64,
    pub size: usize,
    pub offset: u64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MachOSection {
    pub sectname: String,
    pub segname: String,
    pub addr: u64,
    pub size: u64,
    pub offset: u32,
    pub align: u32,
    pub reloff: u32,
    pub nreloc: u32,
    pub flags: u32,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MachOSegment {
    pub segname: String,
    pub vmaddr: u64,
    pub vmsize: u64,
    pub fileoff: u64,
    pub filesize: u64,
    pub maxprot: u32,
    pub initprot: u32,
    pub nsects: u32,
    pub flags: u32,
    pub sections: Vec<MachOSection>,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct MachOTlv {
    pub start: u64,
    pub size: u64,
    pub var_ranges: Vec<(u64, u64)>,
    pub func_ranges: Vec<(u64, u64)>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MachOImage {
    pub vmbase: u64,
    pub vmsize: u64,
    pub segments: Vec<MachOSegment>,
    pub imports: Vec<MachOImport>,
    pub rebase: Vec<MachORebase>,
    pub chained_rebase: Vec<ChainedRebase>,
    pub exports: Vec<MachOExport>,
    pub entry: u64,
    pub tlv: MachOTlv,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct TlvDescriptor {
    pub thunk: extern "C" fn(desc: *const TlvDescriptor) -> usize,
    pub opaq: usize,
    pub offset: usize,
}

extern "C" fn tlv_thunk(desc: *const TlvDescriptor) -> usize {
    unsafe { (*desc).opaq }
}

impl MachOImage {
    pub fn new(data: &[u8]) -> anyhow::Result<Self> {
        let macho = MachO::parse(data, 0)?;

        // Copy out segments and sections
        let mut vmbase = u64::MAX;
        let mut vmsize = u64::MIN;
        let mut segments = Vec::new();
        for seg in macho.segments.iter() {
            // Get lowest loadable address as base
            if seg.filesize != 0 {
                vmbase = vmbase.min(seg.vmaddr);
                vmsize = vmsize.max(seg.vmaddr + seg.vmsize);
            }
            let mut sections = Vec::new();
            for (sect, _) in seg.sections()?.iter() {
                let size = (seg.vmaddr + seg.vmsize).min(sect.addr + sect.size) - sect.addr;
                sections.push(MachOSection {
                    sectname: sect.name()?.to_owned(),
                    segname: sect.segname()?.to_owned(),
                    addr: sect.addr,
                    size: size,
                    offset: sect.offset,
                    align: sect.align,
                    reloff: sect.reloff,
                    nreloc: sect.nreloc,
                    flags: sect.flags,
                })
            }
            segments.push(MachOSegment {
                segname: seg.name()?.to_owned(),
                vmaddr: seg.vmaddr,
                vmsize: seg.vmsize,
                fileoff: seg.fileoff,
                filesize: seg.filesize,
                maxprot: seg.maxprot,
                initprot: seg.initprot,
                nsects: seg.nsects,
                flags: seg.flags,
                sections: sections,
            });
        }

        // Copy out imports information
        let mut imports = Vec::new();
        for i in macho.imports()? {
            imports.push(MachOImport {
                name: i.name.to_owned(),
                dylib: i.dylib.to_owned(),
                offset: i.offset,
                size: i.size,
                address: i.address,
                addend: i.addend,
                is_lazy: i.is_lazy,
                is_weak: i.is_weak,
            });
        }

        let mut exports = Vec::new();
        for e in macho.exports()? {
            exports.push(match e.info{
                ExportInfo::Regular { address, flags } => MachOExport {
                    name: e.name.to_owned(),
                    kind: MachOExportKind::Regular {
                        address,
                    },
                    flags: flags,
                    size: e.size,
                    offset: e.offset,
                },
                ExportInfo::Reexport { lib, lib_symbol_name, flags }  => MachOExport {
                    name: e.name.to_owned(),
                    kind: MachOExportKind::ReExport {
                        lib: lib.to_owned(),
                        lib_symbol_name: lib_symbol_name.map( |x| x.to_owned()),
                    },
                    flags: flags,
                    size: e.size,
                    offset: e.offset,
                },
                ExportInfo::Stub { stub_offset, resolver_offset, flags } => MachOExport {
                    name: e.name.to_owned(),
                    kind: MachOExportKind::Stub {
                        stub_offset,
                        resolver_offset
                    },
                    flags: flags,
                    size: e.size,
                    offset: e.offset,
                }
            });
        }

        let mut tlv = MachOTlv::default();
        for seg in macho.segments.iter() {
            for (sect, _) in seg.sections()?.iter() {
                let kind = sect.flags & 0xFF;
                if kind == 0x11 || kind == 0x12 {
                    if tlv.start == 0 || sect.addr < tlv.start {
                        tlv.start = sect.addr;
                    }
                    let size = ((((sect.addr + sect.size) - tlv.start) + 0x10 - 1) / 0x10) * 0x10;
                    if tlv.size == 0 || size > tlv.size {
                        tlv.size = size;
                    }
                }
                if kind == 0x13 {
                    tlv.var_ranges.push((sect.addr, sect.size))
                }
                if kind == 0x15 {
                    tlv.func_ranges.push((sect.addr, sect.size))
                }
            }
        }

        // Goblin does not read 10.6+ relocations :(
        let mut rebase = Vec::new();
        let mut chained_rebase = Vec::new();
        let mut chained_fixups_offset: Option<u32> = None;

        for c in macho.load_commands.iter() {
            match &c.command {
                CommandVariant::DyldChainedFixups(linkedit) => {
                    chained_fixups_offset = Some(linkedit.dataoff);
                }
                CommandVariant::DyldInfo(info) | CommandVariant::DyldInfoOnly(info) => {
                    let mut i = info.rebase_off as usize;
                    let mut kind = 1u8;
                    let mut segment = 0u8;
                    let mut offset = 0u64;
                    while i < (info.rebase_off + info.rebase_size) as usize {
                        let cmd: u8 = data.gread(&mut i)?;
                        let imm = cmd & 0x0F;
                        match cmd & 0xF0 {
                            0x00 => {
                                // REBASE_OPCODE_DONE
                                // NOTE: this opcode is callde "done" but it does not acutally finish anything
                            }
                            0x10 => {
                                // REBASE_OPCODE_SET_TYPE_IMM
                                kind = imm;
                            }
                            0x20 => {
                                // REBASE_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB
                                segment = imm;
                                offset = Uleb128::read(&data, &mut i)?;
                            }
                            0x30 => {
                                // REBASE_OPCODE_ADD_ADDR_ULEB
                                offset = offset.wrapping_add(Uleb128::read(&data, &mut i)?);
                            }
                            0x40 => {
                                // REBASE_OPCODE_ADD_ADDR_IMM_SCALED
                                offset = offset
                                    .wrapping_add((imm * std::mem::size_of::<usize>() as u8) as _);
                            }
                            0x50 => {
                                // REBASE_OPCODE_DO_REBASE_IMM_TIMES
                                for _ in 0..imm {
                                    let vmaddr =
                                        segments[segment as usize].vmaddr.wrapping_add(offset);
                                    rebase.push(MachORebase { kind, vmaddr });
                                    offset += std::mem::size_of::<usize>() as u64;
                                }
                            }
                            0x60 => {
                                // REBASE_OPCODE_DO_REBASE_ULEB_TIMES
                                let count = Uleb128::read(&data, &mut i)?;
                                for _ in 0..count {
                                    let vmaddr =
                                        segments[segment as usize].vmaddr.wrapping_add(offset);
                                    rebase.push(MachORebase { kind, vmaddr });
                                    offset += std::mem::size_of::<usize>() as u64;
                                }
                            }
                            0x70 => {
                                // REBASE_OPCODE_DO_REBASE_ADD_ADDR_ULEB
                                let skip = Uleb128::read(&data, &mut i)?;
                                let vmaddr = segments[segment as usize].vmaddr.wrapping_add(offset);
                                rebase.push(MachORebase { kind, vmaddr });
                                offset += std::mem::size_of::<usize>() as u64;
                                offset += skip;
                            }
                            0x80 => {
                                // REBASE_OPCODE_DO_REBASE_ULEB_TIMES_SKIPPING_ULEB
                                let count = Uleb128::read(&data, &mut i)?;
                                let skip = Uleb128::read(&data, &mut i)?;
                                for _ in 0..count {
                                    let vmaddr =
                                        segments[segment as usize].vmaddr.wrapping_add(offset);
                                    rebase.push(MachORebase { kind, vmaddr });
                                    offset += std::mem::size_of::<usize>() as u64;
                                    offset += skip;
                                }
                            }
                            _ => bail!("Unknown opcode: {:x}", cmd),
                        }
                    }
                }
                _ => {}
            }
        }

        // Parse chained fixups if present
        if let Some(fixups_off) = chained_fixups_offset {
            chained_rebase = Self::parse_chained_fixups(data, fixups_off as usize, &segments)?;
        }

        let entry = macho.entry;

        Ok(Self {
            vmbase,
            vmsize,
            segments,
            imports,
            rebase,
            chained_rebase,
            exports,
            entry,
            tlv,
        })
    }

    /// Parse chained fixups from LC_DYLD_CHAINED_FIXUPS data
    fn parse_chained_fixups(
        data: &[u8],
        fixups_off: usize,
        segments: &[MachOSegment],
    ) -> anyhow::Result<Vec<ChainedRebase>> {
        let mut rebases = Vec::new();

        // Read the header
        let header: DyldChainedFixupsHeader = unsafe {
            std::ptr::read_unaligned(data.as_ptr().add(fixups_off) as *const _)
        };

        // Get starts_in_image offset (relative to fixups_off)
        let starts_off = fixups_off + header.starts_offset as usize;

        // Read seg_count
        let seg_count: u32 = unsafe {
            std::ptr::read_unaligned(data.as_ptr().add(starts_off) as *const _)
        };

        // Read segment info offsets (array of u32 after seg_count)
        let seg_info_offsets_ptr = unsafe { data.as_ptr().add(starts_off + 4) as *const u32 };

        for seg_idx in 0..seg_count as usize {
            // Get the offset to this segment's starts info
            let seg_info_off: u32 = unsafe { *seg_info_offsets_ptr.add(seg_idx) };
            if seg_info_off == 0 {
                continue; // No fixups in this segment
            }

            let seg_starts_off = starts_off + seg_info_off as usize;

            // Read the segment starts structure
            let seg_starts: DyldChainedStartsInSegment = unsafe {
                std::ptr::read_unaligned(data.as_ptr().add(seg_starts_off) as *const _)
            };

            if seg_starts.page_count == 0 {
                continue;
            }

            // Get segment base address
            let seg_vmaddr = if seg_idx < segments.len() {
                segments[seg_idx].vmaddr
            } else {
                continue;
            };

            // Get pointer stride based on format
            let stride = Self::get_pointer_stride(seg_starts.pointer_format);
            if stride == 0 {
                eprintln!(
                    "Warning: Unsupported pointer format {} in segment {}",
                    seg_starts.pointer_format, seg_idx
                );
                continue;
            }

            // Page starts array follows the fixed-size portion of the struct
            let page_starts_ptr = unsafe {
                data.as_ptr().add(seg_starts_off + 22) as *const u16 // 22 = size of fixed fields
            };

            for page_idx in 0..seg_starts.page_count as usize {
                let page_start: u16 = unsafe { *page_starts_ptr.add(page_idx) };

                if page_start == DYLD_CHAINED_PTR_START_NONE {
                    continue; // No fixups on this page
                }

                if (page_start & DYLD_CHAINED_PTR_START_MULTI) != 0 {
                    // Multiple chain starts on this page - not common, skip for now
                    eprintln!("Warning: Multi-start page not fully implemented");
                    continue;
                }

                // Calculate the starting address in the file for this chain
                let page_content_start = seg_starts.segment_offset as usize
                    + (page_idx * seg_starts.page_size as usize);
                let chain_start = page_content_start + page_start as usize;

                // Walk the chain
                Self::walk_chain(
                    data,
                    chain_start,
                    seg_vmaddr + (page_idx * seg_starts.page_size as usize) as u64
                        + page_start as u64,
                    seg_starts.pointer_format,
                    stride,
                    &mut rebases,
                );
            }
        }

        Ok(rebases)
    }

    /// Get the stride (distance between fixup entries) based on pointer format
    fn get_pointer_stride(pointer_format: u16) -> usize {
        match pointer_format {
            DYLD_CHAINED_PTR_ARM64E
            | DYLD_CHAINED_PTR_ARM64E_KERNEL
            | DYLD_CHAINED_PTR_ARM64E_USERLAND
            | DYLD_CHAINED_PTR_ARM64E_FIRMWARE
            | DYLD_CHAINED_PTR_ARM64E_USERLAND24 => 8, // 8-byte stride
            DYLD_CHAINED_PTR_64 | DYLD_CHAINED_PTR_64_OFFSET => 4, // 4-byte stride
            DYLD_CHAINED_PTR_64_KERNEL_CACHE | DYLD_CHAINED_PTR_X86_64_KERNEL_CACHE => 4,
            DYLD_CHAINED_PTR_32 | DYLD_CHAINED_PTR_32_CACHE | DYLD_CHAINED_PTR_32_FIRMWARE => 4,
            _ => 0, // Unknown format
        }
    }

    /// Walk a chain of fixups starting at the given file offset
    fn walk_chain(
        data: &[u8],
        mut file_offset: usize,
        mut vmaddr: u64,
        pointer_format: u16,
        stride: usize,
        rebases: &mut Vec<ChainedRebase>,
    ) {
        loop {
            if file_offset >= data.len() {
                break;
            }

            // Read the pointer value
            let raw_value: u64 = match pointer_format {
                DYLD_CHAINED_PTR_32 | DYLD_CHAINED_PTR_32_CACHE | DYLD_CHAINED_PTR_32_FIRMWARE => {
                    unsafe { std::ptr::read_unaligned(data.as_ptr().add(file_offset) as *const u32) as u64 }
                }
                _ => unsafe { std::ptr::read_unaligned(data.as_ptr().add(file_offset) as *const u64) },
            };

            // Parse based on format
            let (next, is_bind, target) = match pointer_format {
                DYLD_CHAINED_PTR_64 => {
                    // target:36, high8:8, reserved:7, next:12, bind:1
                    let bind = (raw_value >> 63) & 1;
                    let next = ((raw_value >> 51) & 0xFFF) as usize;
                    let target = raw_value & 0xFFFFFFFFF; // 36 bits
                    let high8 = ((raw_value >> 36) & 0xFF) << 56;
                    (next, bind != 0, target | high8)
                }
                DYLD_CHAINED_PTR_64_OFFSET => {
                    // Same layout as DYLD_CHAINED_PTR_64 but target is an offset, not absolute
                    let bind = (raw_value >> 63) & 1;
                    let next = ((raw_value >> 51) & 0xFFF) as usize;
                    let target = raw_value & 0xFFFFFFFFF;
                    let high8 = ((raw_value >> 36) & 0xFF) << 56;
                    (next, bind != 0, target | high8)
                }
                DYLD_CHAINED_PTR_ARM64E | DYLD_CHAINED_PTR_ARM64E_USERLAND => {
                    // For rebase: target:43, high8:8, next:11, bind:1, auth:1
                    let auth = (raw_value >> 63) & 1;
                    let bind = (raw_value >> 62) & 1;
                    let next = ((raw_value >> 51) & 0x7FF) as usize;
                    if auth != 0 {
                        // Authenticated pointer - skip for now
                        (next, true, 0)
                    } else {
                        let target = raw_value & 0x7FFFFFFFFFF; // 43 bits
                        let high8 = ((raw_value >> 43) & 0xFF) << 56;
                        (next, bind != 0, target | high8)
                    }
                }
                DYLD_CHAINED_PTR_ARM64E_USERLAND24 => {
                    // Similar to ARM64E but with 24-bit ordinal for binds
                    let auth = (raw_value >> 63) & 1;
                    let bind = (raw_value >> 62) & 1;
                    let next = ((raw_value >> 51) & 0x7FF) as usize;
                    if auth != 0 || bind != 0 {
                        (next, true, 0)
                    } else {
                        let target = raw_value & 0x7FFFFFFFFFF;
                        let high8 = ((raw_value >> 43) & 0xFF) << 56;
                        (next, false, target | high8)
                    }
                }
                DYLD_CHAINED_PTR_32 => {
                    // target:26, next:5, bind:1
                    let bind = (raw_value >> 31) & 1;
                    let next = ((raw_value >> 26) & 0x1F) as usize;
                    let target = raw_value & 0x3FFFFFF;
                    (next, bind != 0, target)
                }
                _ => {
                    // Unknown format - stop walking
                    break;
                }
            };

            // Record rebase if this is not a bind
            if !is_bind {
                rebases.push(ChainedRebase { vmaddr, target });
            }

            // Move to next fixup in the chain
            if next == 0 {
                break; // End of chain
            }

            let delta = next * stride;
            file_offset += delta;
            vmaddr += delta as u64;
        }
    }

    pub fn map_image(&self, data: &[u8]) -> anyhow::Result<MemoryMap> {
        // Try to map at prefered address first.
        let map = MemoryMap::new(
            self.vmsize as _,
            &[
                MapOption::MapReadable,
                MapOption::MapWritable,
                MapOption::MapExecutable,
                MapOption::MapAddr(self.vmbase as _),
            ],
        );

        // If prefered address fails, map anywhere.
        let (map, should_rebase) = match map {
            Ok(map) => (map, false),
            Err(_) => (
                MemoryMap::new(
                    self.vmsize as _,
                    &[
                        MapOption::MapReadable,
                        MapOption::MapWritable,
                        MapOption::MapExecutable,
                    ],
                )?,
                true,
            ),
        };

        // Copy contents
        let image = unsafe { &mut *std::ptr::slice_from_raw_parts_mut(map.data(), map.len()) };
        for segment in self.segments.iter() {
            if segment.filesize == 0 {
                continue;
            }
            if segment.vmsize >= segment.filesize {
                let addr = segment.vmaddr - self.vmbase;
                let src = &data[segment.fileoff as _..][..segment.filesize as _];
                let dst = &mut image[addr as _..][..segment.filesize as _];
                dst.copy_from_slice(src);
            }
            for section in segment.sections.iter() {
                match section.flags & SECTION_TYPE {
                    S_ZEROFILL | S_GB_ZEROFILL | S_THREAD_LOCAL_ZEROFILL => {
                        let addr = section.addr - self.vmbase;
                        let dst = &mut image[addr as _..][..section.size as _];
                        dst.fill(0);
                    }
                    _ => {}
                }
            }
        }

        // Do rebase (legacy format)
        if should_rebase {
            for rebase in self.rebase.iter() {
                if rebase.kind != BIND_TYPE_POINTER {
                    continue;
                }
                let addr = rebase.vmaddr - self.vmbase;
                unsafe {
                    let target = image.as_mut_ptr().offset(addr as _).cast::<usize>();
                    let value = target.read_unaligned();
                    target.write_unaligned(
                        value
                            .wrapping_sub(self.vmbase as usize)
                            .wrapping_add(image.as_ptr() as _),
                    );
                };
            }
        }

        // Apply chained fixups rebases
        // These always need to be applied because the file contains encoded values
        for rebase in self.chained_rebase.iter() {
            let addr = rebase.vmaddr - self.vmbase;
            if addr as usize >= image.len() {
                continue;
            }
            unsafe {
                let ptr = image.as_mut_ptr().offset(addr as _).cast::<usize>();
                // The target is an offset from the image base, convert to runtime address
                let runtime_addr = image.as_ptr() as usize + rebase.target as usize;
                ptr.write_unaligned(runtime_addr);
            }
        }

        Ok(map)
    }

    pub fn resolve_imports<R: Fn(&str) -> usize>(&self, image: &mut [u8], resolver: R) {
        for import in self.imports.iter() {
            // dont bind weak symbols
            if import.is_weak {
                continue;
            }
            let value = resolver(&import.name);
            if value == 0 {
                continue;
            }
            let addr = import.address - self.vmbase;
            unsafe {
                let target = image.as_mut_ptr().offset(addr as _).cast::<usize>();
                target.write_unaligned(value.wrapping_add(import.addend as _));
            }
        }
    }

    pub fn resolve_exports<R: Fn(&str, usize) -> bool>(&self, image: &mut [u8], resolver: R) {
        for e in self.exports.iter() {
            match e.kind {
                MachOExportKind::Regular { address } => {
                    resolver(&e.name, image.as_ptr() as usize + (address as usize));
                },
                _ => {},
            }
        }
    }

    pub fn fixup_tlv(&self, image: &mut [u8]) {
        // NOTE: this is horribly wrong
        // This should in fact allocate a new pthread_key and then in thunk:
        // 1. pthread_getspecific to check if there is already data allocated and return early
        // 2. malloc(tlv.size) and pthread_setspecific new data
        // 3. call tlv.func_ranges to init new tread specific data
        // Additionally tlv_thunk should be [[clang::preserveall]].
        for (addr, size) in &self.tlv.var_ranges {
            let mut i = addr - self.vmbase;
            let end = i + size;
            while i < end {
                unsafe { 
                    let p = image.as_mut_ptr().offset(i as _) as usize as *mut TlvDescriptor;
                    let mut desc = (*p).clone();
                    desc.thunk = tlv_thunk;
                    desc.opaq = image.as_ptr().offset(self.tlv.start as _).offset(desc.offset as _) as _;
                    *p = desc;
                };
                i += std::mem::size_of::<TlvDescriptor>() as u64;
            }
        }
    }

    pub fn run_mod_init(&self, image: &[u8]) {
        // Collect all inits.
        let mut init = Vec::<extern "C" fn()>::new();
        for segment in self.segments.iter() {
            for section in segment.sections.iter() {
                if (section.flags & SECTION_TYPE) != S_MOD_INIT_FUNC_POINTERS {
                    continue;
                }
                let mut i = section.addr - self.vmbase;
                let end = i + section.size;
                while i < end {
                    let p = unsafe { *(image.as_ptr().offset(i as _) as *const _) };
                    init.push(p);
                    i += std::mem::size_of::<usize>() as u64;
                }
            }
        }

        // Run initializers
        for f in init {
            f();
        }
    }

    pub fn get_entry(&self, image: &[u8]) -> extern "C" fn() {
        let target = image.as_ptr() as usize + (self.entry - self.vmbase) as usize;
        unsafe { std::mem::transmute(target) }
    }
}
