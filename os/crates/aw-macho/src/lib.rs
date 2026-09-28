#![no_std]
#![forbid(unsafe_code)]

const MACH_HEADER_64_SIZE: usize = 32;
const LOAD_COMMAND_HEADER_SIZE: usize = 8;
const FAT_HEADER_SIZE: usize = 8;
const FAT_ARCH_SIZE: usize = 20;
const MAX_FAT_ARCHES: u32 = 128;

const MH_MAGIC_64: u32 = 0xfeed_facf;
const MH_CIGAM_64: u32 = 0xcffa_edfe;
const FAT_MAGIC: u32 = 0xcafe_babe;
const FAT_CIGAM: u32 = 0xbeba_feca;
const MH_EXECUTE: u32 = 0x2;
const MH_OBJECT: u32 = 0x1;
const MH_DYLIB: u32 = 0x6;
const MH_DYLINKER: u32 = 0x7;
const MH_BUNDLE: u32 = 0x8;
const MH_FILESET: u32 = 0xc;
const MH_ALLOW_STACK_EXECUTION: u32 = 0x0002_0000;

const CPU_TYPE_X86_64: u32 = 0x0100_0007;
const CPU_TYPE_ARM64: u32 = 0x0100_000c;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuType {
    X86_64,
    Arm64,
}

impl CpuType {
    #[must_use]
    pub const fn raw(self) -> u32 {
        match self {
            Self::X86_64 => CPU_TYPE_X86_64,
            Self::Arm64 => CPU_TYPE_ARM64,
        }
    }

    fn from_raw(raw: u32) -> Result<Self, ParseError> {
        match raw {
            CPU_TYPE_X86_64 => Ok(Self::X86_64),
            CPU_TYPE_ARM64 => Ok(Self::Arm64),
            _ => Err(ParseError::UnsupportedCpu(raw)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MachFileType {
    Object,
    Execute,
    Dylib,
    Dylinker,
    Bundle,
    Fileset,
    Other(u32),
}

impl MachFileType {
    #[must_use]
    pub const fn raw(self) -> u32 {
        match self {
            Self::Object => MH_OBJECT,
            Self::Execute => MH_EXECUTE,
            Self::Dylib => MH_DYLIB,
            Self::Dylinker => MH_DYLINKER,
            Self::Bundle => MH_BUNDLE,
            Self::Fileset => MH_FILESET,
            Self::Other(raw) => raw,
        }
    }

    const fn from_raw(raw: u32) -> Self {
        match raw {
            MH_OBJECT => Self::Object,
            MH_EXECUTE => Self::Execute,
            MH_DYLIB => Self::Dylib,
            MH_DYLINKER => Self::Dylinker,
            MH_BUNDLE => Self::Bundle,
            MH_FILESET => Self::Fileset,
            _ => Self::Other(raw),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MachHeader64 {
    pub cpu: CpuType,
    pub cpu_subtype: u32,
    pub file_type: MachFileType,
    pub command_count: u32,
    pub command_bytes: u32,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MachSlice {
    pub offset: usize,
    pub size: usize,
    pub header: MachHeader64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseError {
    TooShort,
    IntegerOverflow,
    UnsupportedMagic(u32),
    UnsupportedEndian,
    UnsupportedCpu(u32),
    LoadCommandsOutOfBounds,
    TooManyLoadCommands,
    LoadCommandTruncated { index: u32 },
    LoadCommandTooSmall { index: u32, size: u32 },
    LoadCommandMisaligned { index: u32, size: u32 },
    LoadCommandRegionMismatch,
    NotExecutableFileType(u32),
    ExecutableStackRequested,
    FatHasNoArchitectures,
    FatTooManyArchitectures(u32),
    FatTableOutOfBounds,
    FatSliceOutOfBounds { index: u32 },
    FatSliceOverlapsTable { index: u32 },
    FatSliceMisaligned { index: u32, align_power: u32 },
    FatSlicesOverlap { first: u32, second: u32 },
    RequestedArchitectureMissing(CpuType),
    AmbiguousArchitecture(CpuType),
    FatArchitectureMismatch { expected: CpuType, actual: CpuType },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FatArch {
    cpu_raw: u32,
    offset: usize,
    size: usize,
    align_power: u32,
}

fn checked_end(offset: usize, size: usize) -> Result<usize, ParseError> {
    offset.checked_add(size).ok_or(ParseError::IntegerOverflow)
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, ParseError> {
    let end = checked_end(offset, 4)?;
    let data = bytes.get(offset..end).ok_or(ParseError::TooShort)?;
    Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}

fn read_u32_be(bytes: &[u8], offset: usize) -> Result<u32, ParseError> {
    let end = checked_end(offset, 4)?;
    let data = bytes.get(offset..end).ok_or(ParseError::TooShort)?;
    Ok(u32::from_be_bytes([data[0], data[1], data[2], data[3]]))
}

pub fn parse_mach_o_64(bytes: &[u8]) -> Result<MachHeader64, ParseError> {
    if bytes.len() < MACH_HEADER_64_SIZE {
        return Err(ParseError::TooShort);
    }

    let magic = read_u32_le(bytes, 0)?;
    if magic == MH_CIGAM_64 {
        return Err(ParseError::UnsupportedEndian);
    }
    if magic != MH_MAGIC_64 {
        return Err(ParseError::UnsupportedMagic(magic));
    }

    let cpu = CpuType::from_raw(read_u32_le(bytes, 4)?)?;
    let cpu_subtype = read_u32_le(bytes, 8)?;
    let file_type = MachFileType::from_raw(read_u32_le(bytes, 12)?);
    let command_count = read_u32_le(bytes, 16)?;
    let command_bytes = read_u32_le(bytes, 20)?;
    let flags = read_u32_le(bytes, 24)?;

    let command_bytes_usize = command_bytes as usize;
    let commands_end = checked_end(MACH_HEADER_64_SIZE, command_bytes_usize)?;
    if commands_end > bytes.len() {
        return Err(ParseError::LoadCommandsOutOfBounds);
    }

    let minimum_command_bytes = (command_count as usize)
        .checked_mul(LOAD_COMMAND_HEADER_SIZE)
        .ok_or(ParseError::IntegerOverflow)?;
    if minimum_command_bytes > command_bytes_usize {
        return Err(ParseError::TooManyLoadCommands);
    }

    let mut cursor = MACH_HEADER_64_SIZE;
    for index in 0..command_count {
        let header_end = checked_end(cursor, LOAD_COMMAND_HEADER_SIZE)?;
        if header_end > commands_end {
            return Err(ParseError::LoadCommandTruncated { index });
        }
        let command_size = read_u32_le(bytes, cursor + 4)?;
        if command_size < LOAD_COMMAND_HEADER_SIZE as u32 {
            return Err(ParseError::LoadCommandTooSmall {
                index,
                size: command_size,
            });
        }
        if !command_size.is_multiple_of(8) {
            return Err(ParseError::LoadCommandMisaligned {
                index,
                size: command_size,
            });
        }
        let next = checked_end(cursor, command_size as usize)?;
        if next > commands_end {
            return Err(ParseError::LoadCommandTruncated { index });
        }
        cursor = next;
    }

    if cursor != commands_end {
        return Err(ParseError::LoadCommandRegionMismatch);
    }

    Ok(MachHeader64 {
        cpu,
        cpu_subtype,
        file_type,
        command_count,
        command_bytes,
        flags,
    })
}

pub fn validate_main_executable(header: MachHeader64) -> Result<(), ParseError> {
    if header.file_type != MachFileType::Execute {
        return Err(ParseError::NotExecutableFileType(header.file_type.raw()));
    }
    if header.flags & MH_ALLOW_STACK_EXECUTION != 0 {
        return Err(ParseError::ExecutableStackRequested);
    }
    Ok(())
}

fn read_fat_arch(bytes: &[u8], index: u32, table_end: usize) -> Result<FatArch, ParseError> {
    let record_offset = FAT_HEADER_SIZE
        .checked_add(
            (index as usize)
                .checked_mul(FAT_ARCH_SIZE)
                .ok_or(ParseError::IntegerOverflow)?,
        )
        .ok_or(ParseError::IntegerOverflow)?;
    let record_end = checked_end(record_offset, FAT_ARCH_SIZE)?;
    if record_end > table_end {
        return Err(ParseError::FatTableOutOfBounds);
    }

    let cpu_raw = read_u32_be(bytes, record_offset)?;
    let offset = read_u32_be(bytes, record_offset + 8)? as usize;
    let size = read_u32_be(bytes, record_offset + 12)? as usize;
    let align_power = read_u32_be(bytes, record_offset + 16)?;
    let slice_end = checked_end(offset, size)?;

    if size == 0 || slice_end > bytes.len() {
        return Err(ParseError::FatSliceOutOfBounds { index });
    }
    if offset < table_end {
        return Err(ParseError::FatSliceOverlapsTable { index });
    }
    if align_power >= usize::BITS {
        return Err(ParseError::FatSliceMisaligned { index, align_power });
    }
    let alignment = 1usize << align_power;
    if !offset.is_multiple_of(alignment) {
        return Err(ParseError::FatSliceMisaligned { index, align_power });
    }

    Ok(FatArch {
        cpu_raw,
        offset,
        size,
        align_power,
    })
}

fn fat_table(bytes: &[u8]) -> Result<(u32, usize), ParseError> {
    if bytes.len() < FAT_HEADER_SIZE {
        return Err(ParseError::TooShort);
    }
    let magic = read_u32_be(bytes, 0)?;
    if magic == FAT_CIGAM {
        return Err(ParseError::UnsupportedEndian);
    }
    if magic != FAT_MAGIC {
        return Err(ParseError::UnsupportedMagic(magic));
    }
    let count = read_u32_be(bytes, 4)?;
    if count == 0 {
        return Err(ParseError::FatHasNoArchitectures);
    }
    if count > MAX_FAT_ARCHES {
        return Err(ParseError::FatTooManyArchitectures(count));
    }
    let table_bytes = (count as usize)
        .checked_mul(FAT_ARCH_SIZE)
        .ok_or(ParseError::IntegerOverflow)?;
    let table_end = checked_end(FAT_HEADER_SIZE, table_bytes)?;
    if table_end > bytes.len() {
        return Err(ParseError::FatTableOutOfBounds);
    }
    Ok((count, table_end))
}

fn validate_fat_layout(bytes: &[u8], count: u32, table_end: usize) -> Result<(), ParseError> {
    for first in 0..count {
        let first_arch = read_fat_arch(bytes, first, table_end)?;
        let first_end = checked_end(first_arch.offset, first_arch.size)?;
        for second in (first + 1)..count {
            let second_arch = read_fat_arch(bytes, second, table_end)?;
            let second_end = checked_end(second_arch.offset, second_arch.size)?;
            if first_arch.offset < second_end && second_arch.offset < first_end {
                return Err(ParseError::FatSlicesOverlap { first, second });
            }
        }
    }
    Ok(())
}

fn select_fat_slice(bytes: &[u8], target: CpuType) -> Result<MachSlice, ParseError> {
    let (count, table_end) = fat_table(bytes)?;
    validate_fat_layout(bytes, count, table_end)?;

    let mut selected: Option<FatArch> = None;
    for index in 0..count {
        let arch = read_fat_arch(bytes, index, table_end)?;
        if arch.cpu_raw == target.raw() {
            if selected.is_some() {
                return Err(ParseError::AmbiguousArchitecture(target));
            }
            selected = Some(arch);
        }
    }

    let arch = selected.ok_or(ParseError::RequestedArchitectureMissing(target))?;
    let end = checked_end(arch.offset, arch.size)?;
    let slice = bytes
        .get(arch.offset..end)
        .ok_or(ParseError::FatSliceOutOfBounds { index: 0 })?;
    let header = parse_mach_o_64(slice)?;
    if header.cpu != target {
        return Err(ParseError::FatArchitectureMismatch {
            expected: target,
            actual: header.cpu,
        });
    }

    Ok(MachSlice {
        offset: arch.offset,
        size: arch.size,
        header,
    })
}

pub fn select_mach_o_64(bytes: &[u8], target: CpuType) -> Result<MachSlice, ParseError> {
    if bytes.len() < 4 {
        return Err(ParseError::TooShort);
    }

    let thin_magic = read_u32_le(bytes, 0)?;
    if thin_magic == MH_MAGIC_64 || thin_magic == MH_CIGAM_64 {
        let header = parse_mach_o_64(bytes)?;
        if header.cpu != target {
            return Err(ParseError::RequestedArchitectureMissing(target));
        }
        return Ok(MachSlice {
            offset: 0,
            size: bytes.len(),
            header,
        });
    }

    select_fat_slice(bytes, target)
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::vec::Vec;

    fn push_le(value: u32, out: &mut Vec<u8>) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_be(value: u32, out: &mut Vec<u8>) {
        out.extend_from_slice(&value.to_be_bytes());
    }

    fn thin(cpu: CpuType, flags: u32) -> Vec<u8> {
        let mut out = Vec::new();
        push_le(MH_MAGIC_64, &mut out);
        push_le(cpu.raw(), &mut out);
        push_le(0, &mut out);
        push_le(MH_EXECUTE, &mut out);
        push_le(1, &mut out);
        push_le(8, &mut out);
        push_le(flags, &mut out);
        push_le(0, &mut out);
        push_le(0x2, &mut out);
        push_le(8, &mut out);
        out
    }

    fn fat_two(x86: &[u8], arm: &[u8]) -> Vec<u8> {
        let x86_offset = 48u32;
        let arm_offset = x86_offset + x86.len() as u32;
        assert!(x86_offset.is_multiple_of(4));
        assert!(arm_offset.is_multiple_of(4));

        let mut out = Vec::new();
        push_be(FAT_MAGIC, &mut out);
        push_be(2, &mut out);
        push_be(CpuType::X86_64.raw(), &mut out);
        push_be(0, &mut out);
        push_be(x86_offset, &mut out);
        push_be(x86.len() as u32, &mut out);
        push_be(2, &mut out);
        push_be(CpuType::Arm64.raw(), &mut out);
        push_be(0, &mut out);
        push_be(arm_offset, &mut out);
        push_be(arm.len() as u32, &mut out);
        push_be(2, &mut out);
        out.extend_from_slice(x86);
        out.extend_from_slice(arm);
        out
    }

    #[test]
    fn parses_bounded_x86_64_thin_image() {
        let image = thin(CpuType::X86_64, 0);
        let parsed = parse_mach_o_64(&image).unwrap();
        assert_eq!(parsed.cpu, CpuType::X86_64);
        assert_eq!(parsed.file_type, MachFileType::Execute);
        assert_eq!(parsed.command_count, 1);
        assert_eq!(parsed.command_bytes, 8);
        assert_eq!(validate_main_executable(parsed), Ok(()));
    }

    #[test]
    fn rejects_executable_stack_request() {
        let image = thin(CpuType::Arm64, MH_ALLOW_STACK_EXECUTION);
        let parsed = parse_mach_o_64(&image).unwrap();
        assert_eq!(
            validate_main_executable(parsed),
            Err(ParseError::ExecutableStackRequested)
        );
    }

    #[test]
    fn rejects_command_region_past_end_of_file() {
        let mut image = thin(CpuType::X86_64, 0);
        image[20..24].copy_from_slice(&16u32.to_le_bytes());
        assert_eq!(
            parse_mach_o_64(&image),
            Err(ParseError::LoadCommandsOutOfBounds)
        );
    }

    #[test]
    fn rejects_misaligned_load_command_size() {
        let mut image = thin(CpuType::X86_64, 0);
        image.extend_from_slice(&[0; 4]);
        image[20..24].copy_from_slice(&12u32.to_le_bytes());
        image[36..40].copy_from_slice(&12u32.to_le_bytes());
        assert_eq!(
            parse_mach_o_64(&image),
            Err(ParseError::LoadCommandMisaligned { index: 0, size: 12 })
        );
    }

    #[test]
    fn selects_arm64_from_fat_wrapper() {
        let x86 = thin(CpuType::X86_64, 0);
        let arm = thin(CpuType::Arm64, 0);
        let fat = fat_two(&x86, &arm);
        let selected = select_mach_o_64(&fat, CpuType::Arm64).unwrap();
        assert_eq!(selected.header.cpu, CpuType::Arm64);
        assert_eq!(selected.offset, 48 + x86.len());
        assert_eq!(selected.size, arm.len());
    }

    #[test]
    fn rejects_overlapping_fat_slices() {
        let x86 = thin(CpuType::X86_64, 0);
        let arm = thin(CpuType::Arm64, 0);
        let mut fat = fat_two(&x86, &arm);
        fat[36..40].copy_from_slice(&52u32.to_be_bytes());
        assert_eq!(
            select_mach_o_64(&fat, CpuType::Arm64),
            Err(ParseError::FatSlicesOverlap {
                first: 0,
                second: 1
            })
        );
    }

    #[test]
    fn rejects_fat_entry_that_lies_about_its_cpu() {
        let x86 = thin(CpuType::X86_64, 0);
        let arm = thin(CpuType::Arm64, 0);
        let mut fat = fat_two(&x86, &arm);
        fat[28..32].copy_from_slice(&CpuType::X86_64.raw().to_be_bytes());
        assert_eq!(
            select_mach_o_64(&fat, CpuType::X86_64),
            Err(ParseError::AmbiguousArchitecture(CpuType::X86_64))
        );
    }
}
