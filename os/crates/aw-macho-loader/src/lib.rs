#![no_std]
#![forbid(unsafe_code)]

use aw_macho::{CpuType, MachSlice, ParseError, select_mach_o_64, validate_main_executable};

const MACH_HEADER_64_SIZE: usize = 32;
const LOAD_COMMAND_HEADER_SIZE: usize = 8;
const SEGMENT_COMMAND_64_SIZE: usize = 72;
const SECTION_64_SIZE: usize = 80;
const ENTRY_POINT_COMMAND_SIZE: usize = 24;

const LC_REQ_DYLD: u32 = 0x8000_0000;
const LC_SEGMENT_64: u32 = 0x19;
const LC_MAIN: u32 = 0x28 | LC_REQ_DYLD;

const VM_PROT_WRITE: u32 = 0x2;
const VM_PROT_EXECUTE: u32 = 0x4;
const MH_PIE: u32 = 0x0020_0000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoaderError {
    Mach(ParseError),
    IntegerOverflow,
    SliceOutOfBounds,
    LoadCommandOutOfBounds {
        index: u32,
    },
    LoadCommandTooSmall {
        index: u32,
        size: u32,
    },
    SegmentCommandTooSmall {
        index: u32,
        size: u32,
    },
    SegmentCommandSizeMismatch {
        index: u32,
        declared: u32,
        expected: usize,
    },
    SegmentFileOutOfBounds {
        index: u32,
    },
    SegmentFileLargerThanVm {
        index: u32,
    },
    SegmentVmRangeOverflow {
        index: u32,
    },
    SegmentProtectionEscalation {
        index: u32,
    },
    WritableExecutableSegment {
        index: u32,
    },
    SegmentFileOverlap {
        first: u32,
        second: u32,
    },
    SegmentVmOverlap {
        first: u32,
        second: u32,
    },
    MainCommandTooSmall {
        index: u32,
        size: u32,
    },
    DuplicateMainCommand,
    MissingMainCommand,
    EntryPointOutOfBounds,
    EntryPointNotExecutable,
    AmbiguousEntryPoint,
}

impl From<ParseError> for LoaderError {
    fn from(value: ParseError) -> Self {
        Self::Mach(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Segment64 {
    pub command_index: u32,
    pub name: [u8; 16],
    pub vm_address: u64,
    pub vm_size: u64,
    pub file_offset: u64,
    pub file_size: u64,
    pub max_protection: u32,
    pub initial_protection: u32,
    pub section_count: u32,
    pub flags: u32,
}

impl Segment64 {
    #[must_use]
    pub const fn is_executable(self) -> bool {
        self.initial_protection & VM_PROT_EXECUTE != 0
    }

    #[must_use]
    pub const fn is_writable(self) -> bool {
        self.initial_protection & VM_PROT_WRITE != 0
    }

    #[must_use]
    pub fn contains_file_offset(self, offset: u64) -> bool {
        if self.file_size == 0 {
            return false;
        }
        self.file_offset
            .checked_add(self.file_size)
            .is_some_and(|end| offset >= self.file_offset && offset < end)
    }

    #[must_use]
    pub fn entry_vm_address(self, file_offset: u64) -> Option<u64> {
        if !self.contains_file_offset(file_offset) {
            return None;
        }
        let delta = file_offset.checked_sub(self.file_offset)?;
        self.vm_address.checked_add(delta)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutablePlan {
    pub slice: MachSlice,
    pub entry_file_offset: u64,
    pub entry_vm_address: u64,
    pub stack_size: u64,
    pub segment_count: u32,
    pub position_independent: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct SegmentIter<'a> {
    image: &'a [u8],
    cursor: usize,
    commands_end: usize,
    next_index: u32,
    command_count: u32,
    failed: bool,
}

fn checked_end(offset: usize, size: usize) -> Result<usize, LoaderError> {
    offset.checked_add(size).ok_or(LoaderError::IntegerOverflow)
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, LoaderError> {
    let end = checked_end(offset, 4)?;
    let data = bytes
        .get(offset..end)
        .ok_or(LoaderError::SliceOutOfBounds)?;
    Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}

fn read_u64_le(bytes: &[u8], offset: usize) -> Result<u64, LoaderError> {
    let end = checked_end(offset, 8)?;
    let data = bytes
        .get(offset..end)
        .ok_or(LoaderError::SliceOutOfBounds)?;
    Ok(u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]))
}

fn selected_image(bytes: &[u8], slice: MachSlice) -> Result<&[u8], LoaderError> {
    let end = checked_end(slice.offset, slice.size)?;
    bytes
        .get(slice.offset..end)
        .ok_or(LoaderError::SliceOutOfBounds)
}

fn command_bounds(
    image: &[u8],
    cursor: usize,
    commands_end: usize,
    index: u32,
) -> Result<(u32, u32, usize), LoaderError> {
    let header_end = checked_end(cursor, LOAD_COMMAND_HEADER_SIZE)?;
    if header_end > commands_end {
        return Err(LoaderError::LoadCommandOutOfBounds { index });
    }
    let command = read_u32_le(image, cursor)?;
    let size = read_u32_le(image, cursor + 4)?;
    if size < LOAD_COMMAND_HEADER_SIZE as u32 {
        return Err(LoaderError::LoadCommandTooSmall { index, size });
    }
    let end = checked_end(cursor, size as usize)?;
    if end > commands_end {
        return Err(LoaderError::LoadCommandOutOfBounds { index });
    }
    Ok((command, size, end))
}

fn parse_segment(image: &[u8], cursor: usize, index: u32) -> Result<Segment64, LoaderError> {
    let size = read_u32_le(image, cursor + 4)?;
    if size < SEGMENT_COMMAND_64_SIZE as u32 {
        return Err(LoaderError::SegmentCommandTooSmall { index, size });
    }

    let section_count = read_u32_le(image, cursor + 64)?;
    let section_bytes = (section_count as usize)
        .checked_mul(SECTION_64_SIZE)
        .ok_or(LoaderError::IntegerOverflow)?;
    let expected = SEGMENT_COMMAND_64_SIZE
        .checked_add(section_bytes)
        .ok_or(LoaderError::IntegerOverflow)?;
    if size as usize != expected {
        return Err(LoaderError::SegmentCommandSizeMismatch {
            index,
            declared: size,
            expected,
        });
    }

    let mut name = [0u8; 16];
    let name_end = checked_end(cursor + 8, name.len())?;
    let source = image
        .get(cursor + 8..name_end)
        .ok_or(LoaderError::LoadCommandOutOfBounds { index })?;
    name.copy_from_slice(source);

    let segment = Segment64 {
        command_index: index,
        name,
        vm_address: read_u64_le(image, cursor + 24)?,
        vm_size: read_u64_le(image, cursor + 32)?,
        file_offset: read_u64_le(image, cursor + 40)?,
        file_size: read_u64_le(image, cursor + 48)?,
        max_protection: read_u32_le(image, cursor + 56)?,
        initial_protection: read_u32_le(image, cursor + 60)?,
        section_count,
        flags: read_u32_le(image, cursor + 68)?,
    };

    validate_segment(image, segment)?;
    Ok(segment)
}

fn validate_segment(image: &[u8], segment: Segment64) -> Result<(), LoaderError> {
    let index = segment.command_index;
    let file_end = segment
        .file_offset
        .checked_add(segment.file_size)
        .ok_or(LoaderError::IntegerOverflow)?;
    if file_end > image.len() as u64 {
        return Err(LoaderError::SegmentFileOutOfBounds { index });
    }
    if segment.file_size > segment.vm_size {
        return Err(LoaderError::SegmentFileLargerThanVm { index });
    }
    segment
        .vm_address
        .checked_add(segment.vm_size)
        .ok_or(LoaderError::SegmentVmRangeOverflow { index })?;
    if segment.is_executable() && segment.is_writable() {
        return Err(LoaderError::WritableExecutableSegment { index });
    }
    if segment.initial_protection & !segment.max_protection != 0 {
        return Err(LoaderError::SegmentProtectionEscalation { index });
    }
    Ok(())
}

pub fn segments<'a>(bytes: &'a [u8], slice: MachSlice) -> Result<SegmentIter<'a>, LoaderError> {
    let image = selected_image(bytes, slice)?;
    let commands_end = checked_end(MACH_HEADER_64_SIZE, slice.header.command_bytes as usize)?;
    if commands_end > image.len() {
        return Err(LoaderError::SliceOutOfBounds);
    }
    Ok(SegmentIter {
        image,
        cursor: MACH_HEADER_64_SIZE,
        commands_end,
        next_index: 0,
        command_count: slice.header.command_count,
        failed: false,
    })
}

impl Iterator for SegmentIter<'_> {
    type Item = Result<Segment64, LoaderError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }

        while self.next_index < self.command_count {
            let index = self.next_index;
            let bounds = command_bounds(self.image, self.cursor, self.commands_end, index);
            let (command, _size, end) = match bounds {
                Ok(value) => value,
                Err(error) => {
                    self.failed = true;
                    return Some(Err(error));
                }
            };
            let cursor = self.cursor;
            self.cursor = end;
            self.next_index += 1;

            if command == LC_SEGMENT_64 {
                return Some(parse_segment(self.image, cursor, index));
            }
        }
        None
    }
}

fn ranges_overlap(start_a: u64, size_a: u64, start_b: u64, size_b: u64) -> bool {
    if size_a == 0 || size_b == 0 {
        return false;
    }
    let Some(end_a) = start_a.checked_add(size_a) else {
        return true;
    };
    let Some(end_b) = start_b.checked_add(size_b) else {
        return true;
    };
    start_a < end_b && start_b < end_a
}

fn validate_segment_overlaps(bytes: &[u8], slice: MachSlice) -> Result<(), LoaderError> {
    for first in segments(bytes, slice)? {
        let first = first?;
        for second in segments(bytes, slice)? {
            let second = second?;
            if second.command_index <= first.command_index {
                continue;
            }
            if ranges_overlap(
                first.file_offset,
                first.file_size,
                second.file_offset,
                second.file_size,
            ) {
                return Err(LoaderError::SegmentFileOverlap {
                    first: first.command_index,
                    second: second.command_index,
                });
            }
            if ranges_overlap(
                first.vm_address,
                first.vm_size,
                second.vm_address,
                second.vm_size,
            ) {
                return Err(LoaderError::SegmentVmOverlap {
                    first: first.command_index,
                    second: second.command_index,
                });
            }
        }
    }
    Ok(())
}

fn parse_main_command(
    image: &[u8],
    cursor: usize,
    index: u32,
    size: u32,
) -> Result<(u64, u64), LoaderError> {
    if size < ENTRY_POINT_COMMAND_SIZE as u32 {
        return Err(LoaderError::MainCommandTooSmall { index, size });
    }
    Ok((
        read_u64_le(image, cursor + 8)?,
        read_u64_le(image, cursor + 16)?,
    ))
}

pub fn plan_executable(bytes: &[u8], target: CpuType) -> Result<ExecutablePlan, LoaderError> {
    let slice = select_mach_o_64(bytes, target)?;
    validate_main_executable(slice.header)?;
    let image = selected_image(bytes, slice)?;
    validate_segment_overlaps(bytes, slice)?;

    let commands_end = checked_end(MACH_HEADER_64_SIZE, slice.header.command_bytes as usize)?;
    if commands_end > image.len() {
        return Err(LoaderError::SliceOutOfBounds);
    }

    let mut cursor = MACH_HEADER_64_SIZE;
    let mut main = None;
    let mut segment_count = 0u32;

    for index in 0..slice.header.command_count {
        let (command, size, end) = command_bounds(image, cursor, commands_end, index)?;
        if command == LC_SEGMENT_64 {
            parse_segment(image, cursor, index)?;
            segment_count = segment_count
                .checked_add(1)
                .ok_or(LoaderError::IntegerOverflow)?;
        } else if command == LC_MAIN {
            if main.is_some() {
                return Err(LoaderError::DuplicateMainCommand);
            }
            main = Some(parse_main_command(image, cursor, index, size)?);
        }
        cursor = end;
    }

    let (entry_file_offset, stack_size) = main.ok_or(LoaderError::MissingMainCommand)?;
    if entry_file_offset >= image.len() as u64 {
        return Err(LoaderError::EntryPointOutOfBounds);
    }

    let mut entry_vm_address = None;
    for segment in segments(bytes, slice)? {
        let segment = segment?;
        if !segment.is_executable() || !segment.contains_file_offset(entry_file_offset) {
            continue;
        }
        let address = segment
            .entry_vm_address(entry_file_offset)
            .ok_or(LoaderError::EntryPointOutOfBounds)?;
        if entry_vm_address.replace(address).is_some() {
            return Err(LoaderError::AmbiguousEntryPoint);
        }
    }

    let entry_vm_address = entry_vm_address.ok_or(LoaderError::EntryPointNotExecutable)?;

    Ok(ExecutablePlan {
        slice,
        entry_file_offset,
        entry_vm_address,
        stack_size,
        segment_count,
        position_independent: slice.header.flags & MH_PIE != 0,
    })
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::vec::Vec;

    const MH_MAGIC_64: u32 = 0xfeed_facf;
    const MH_EXECUTE: u32 = 0x2;
    const CPU_TYPE_X86_64: u32 = 0x0100_0007;
    const VM_PROT_READ: u32 = 0x1;

    fn push_u32(value: u32, out: &mut Vec<u8>) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u64(value: u64, out: &mut Vec<u8>) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn fixture(main_count: u32, initial_protection: u32, entryoff: u64) -> Vec<u8> {
        let command_count = 1 + main_count;
        let command_bytes =
            SEGMENT_COMMAND_64_SIZE as u32 + main_count * ENTRY_POINT_COMMAND_SIZE as u32;

        let mut out = Vec::new();
        push_u32(MH_MAGIC_64, &mut out);
        push_u32(CPU_TYPE_X86_64, &mut out);
        push_u32(3, &mut out);
        push_u32(MH_EXECUTE, &mut out);
        push_u32(command_count, &mut out);
        push_u32(command_bytes, &mut out);
        push_u32(MH_PIE, &mut out);
        push_u32(0, &mut out);

        push_u32(LC_SEGMENT_64, &mut out);
        push_u32(SEGMENT_COMMAND_64_SIZE as u32, &mut out);
        let mut name = [0u8; 16];
        name[..6].copy_from_slice(b"__TEXT");
        out.extend_from_slice(&name);
        push_u64(0x1_0000_0000, &mut out);
        push_u64(0x1000, &mut out);
        push_u64(0, &mut out);
        push_u64(0x1000, &mut out);
        push_u32(VM_PROT_READ | VM_PROT_EXECUTE, &mut out);
        push_u32(initial_protection, &mut out);
        push_u32(0, &mut out);
        push_u32(0, &mut out);

        for _ in 0..main_count {
            push_u32(LC_MAIN, &mut out);
            push_u32(ENTRY_POINT_COMMAND_SIZE as u32, &mut out);
            push_u64(entryoff, &mut out);
            push_u64(0, &mut out);
        }

        out.resize(0x1000, 0);
        out
    }

    #[test]
    fn plans_x86_64_main_entrypoint() {
        let image = fixture(1, VM_PROT_READ | VM_PROT_EXECUTE, 0x100);
        let plan = plan_executable(&image, CpuType::X86_64).unwrap();
        assert_eq!(plan.entry_file_offset, 0x100);
        assert_eq!(plan.entry_vm_address, 0x1_0000_0100);
        assert_eq!(plan.segment_count, 1);
        assert!(plan.position_independent);
    }

    #[test]
    fn exposes_validated_segments() {
        let image = fixture(1, VM_PROT_READ | VM_PROT_EXECUTE, 0x100);
        let slice = select_mach_o_64(&image, CpuType::X86_64).unwrap();
        let items: Vec<_> = segments(&image, slice).unwrap().collect();
        assert_eq!(items.len(), 1);
        let segment = items[0].unwrap();
        assert_eq!(&segment.name[..6], b"__TEXT");
        assert!(segment.is_executable());
        assert!(!segment.is_writable());
    }

    #[test]
    fn rejects_writable_executable_segment() {
        let image = fixture(1, VM_PROT_READ | VM_PROT_WRITE | VM_PROT_EXECUTE, 0x100);
        assert_eq!(
            plan_executable(&image, CpuType::X86_64),
            Err(LoaderError::WritableExecutableSegment { index: 0 })
        );
    }

    #[test]
    fn rejects_missing_main_command() {
        let image = fixture(0, VM_PROT_READ | VM_PROT_EXECUTE, 0x100);
        assert_eq!(
            plan_executable(&image, CpuType::X86_64),
            Err(LoaderError::MissingMainCommand)
        );
    }

    #[test]
    fn rejects_duplicate_main_command() {
        let image = fixture(2, VM_PROT_READ | VM_PROT_EXECUTE, 0x100);
        assert_eq!(
            plan_executable(&image, CpuType::X86_64),
            Err(LoaderError::DuplicateMainCommand)
        );
    }

    #[test]
    fn rejects_entrypoint_outside_executable_segment() {
        let image = fixture(1, VM_PROT_READ, 0x100);
        assert_eq!(
            plan_executable(&image, CpuType::X86_64),
            Err(LoaderError::EntryPointNotExecutable)
        );
    }

    #[test]
    fn rejects_segment_file_range_past_image() {
        let mut image = fixture(1, VM_PROT_READ | VM_PROT_EXECUTE, 0x100);
        image[80..88].copy_from_slice(&0x2000u64.to_le_bytes());
        assert_eq!(
            plan_executable(&image, CpuType::X86_64),
            Err(LoaderError::SegmentFileOutOfBounds { index: 0 })
        );
    }

    #[test]
    fn rejects_segment_protection_above_maximum() {
        let mut image = fixture(1, VM_PROT_READ | VM_PROT_EXECUTE, 0x100);
        image[88..92].copy_from_slice(&VM_PROT_READ.to_le_bytes());
        assert_eq!(
            plan_executable(&image, CpuType::X86_64),
            Err(LoaderError::SegmentProtectionEscalation { index: 0 })
        );
    }
}
