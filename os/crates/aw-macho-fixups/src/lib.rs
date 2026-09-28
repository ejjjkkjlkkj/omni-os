#![no_std]
#![forbid(unsafe_code)]

use aw_macho::MachSlice;

const MACH_HEADER_64_SIZE: usize = 32;
const LOAD_COMMAND_HEADER_SIZE: usize = 8;
const DYLD_INFO_COMMAND_SIZE: usize = 48;
const LINKEDIT_DATA_COMMAND_SIZE: usize = 16;
const CHAINED_FIXUPS_HEADER_SIZE: usize = 28;
const CHAINED_STARTS_HEADER_SIZE: usize = 4;
const CHAINED_SEGMENT_HEADER_SIZE: usize = 22;

const LC_REQ_DYLD: u32 = 0x8000_0000;
const LC_DYLD_INFO: u32 = 0x22;
const LC_DYLD_INFO_ONLY: u32 = LC_DYLD_INFO | LC_REQ_DYLD;
const LC_DYLD_EXPORTS_TRIE: u32 = 0x33 | LC_REQ_DYLD;
const LC_DYLD_CHAINED_FIXUPS: u32 = 0x34 | LC_REQ_DYLD;

const DYLD_CHAINED_IMPORT: u32 = 1;
const DYLD_CHAINED_IMPORT_ADDEND: u32 = 2;
const DYLD_CHAINED_IMPORT_ADDEND64: u32 = 3;
const DYLD_CHAINED_SYMBOLS_UNCOMPRESSED: u32 = 0;
const DYLD_CHAINED_SYMBOLS_ZLIB: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixupError {
    IntegerOverflow,
    SliceOutOfBounds,
    LoadCommandOutOfBounds {
        index: u32,
    },
    LoadCommandTooSmall {
        index: u32,
        size: u32,
    },
    LoadCommandMisaligned {
        index: u32,
        size: u32,
    },
    LoadCommandRegionMismatch,
    CommandTooSmall {
        index: u32,
        command: u32,
        size: usize,
        minimum: usize,
    },
    LinkeditRangeOutOfBounds {
        index: u32,
        offset: u32,
        size: u32,
    },
    DuplicateDyldInfo,
    DuplicateExportsTrie,
    DuplicateChainedFixups,
    ChainedHeaderTooShort {
        size: usize,
    },
    UnsupportedFixupsVersion(u32),
    UnsupportedImportsFormat(u32),
    UnsupportedSymbolsFormat(u32),
    ChainedOffsetOutOfBounds {
        offset: u32,
    },
    ImportsTableOutOfBounds,
    StartsTableOutOfBounds,
    SegmentIndexOutOfBounds {
        index: u32,
        count: u32,
    },
    SegmentInfoOffsetInvalid {
        index: u32,
        offset: u32,
    },
    SegmentInfoTooSmall {
        index: u32,
        size: u32,
        minimum: usize,
    },
    SegmentInfoOutOfBounds {
        index: u32,
    },
    UnsupportedPageSize {
        index: u32,
        page_size: u16,
    },
    PageIndexOutOfBounds {
        index: u16,
        count: u16,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkeditRange {
    pub offset: u32,
    pub size: u32,
}

impl LinkeditRange {
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.size == 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DyldInfo {
    pub command_index: u32,
    pub rebase: LinkeditRange,
    pub bind: LinkeditRange,
    pub weak_bind: LinkeditRange,
    pub lazy_bind: LinkeditRange,
    pub exports: LinkeditRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkeditData {
    pub command_index: u32,
    pub range: LinkeditRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixupMetadata {
    pub dyld_info: Option<DyldInfo>,
    pub exports_trie: Option<LinkeditData>,
    pub chained_fixups: Option<LinkeditData>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChainedFixupsHeader {
    pub fixups_version: u32,
    pub starts_offset: u32,
    pub imports_offset: u32,
    pub symbols_offset: u32,
    pub imports_count: u32,
    pub imports_format: u32,
    pub symbols_format: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChainedSegmentInfo<'a> {
    pub segment_index: u32,
    pub size: u32,
    pub page_size: u16,
    pub pointer_format: u16,
    pub segment_offset: u64,
    pub max_valid_pointer: u32,
    page_starts: &'a [u8],
    page_count: u16,
}

impl ChainedSegmentInfo<'_> {
    #[must_use]
    pub const fn page_count(&self) -> u16 {
        self.page_count
    }

    pub fn page_start(&self, index: u16) -> Result<u16, FixupError> {
        if index >= self.page_count {
            return Err(FixupError::PageIndexOutOfBounds {
                index,
                count: self.page_count,
            });
        }
        read_u16_le(self.page_starts, usize::from(index) * 2)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChainedFixups<'a> {
    pub header: ChainedFixupsHeader,
    payload: &'a [u8],
    starts_base: usize,
    segment_count: u32,
    segment_table_bytes: usize,
}

impl<'a> ChainedFixups<'a> {
    #[must_use]
    pub const fn segment_count(&self) -> u32 {
        self.segment_count
    }

    #[must_use]
    pub const fn payload(&self) -> &'a [u8] {
        self.payload
    }

    pub fn segment(&self, index: u32) -> Result<Option<ChainedSegmentInfo<'a>>, FixupError> {
        if index >= self.segment_count {
            return Err(FixupError::SegmentIndexOutOfBounds {
                index,
                count: self.segment_count,
            });
        }

        let table_offset = self
            .starts_base
            .checked_add(CHAINED_STARTS_HEADER_SIZE)
            .and_then(|value| value.checked_add((index as usize).checked_mul(4)?))
            .ok_or(FixupError::IntegerOverflow)?;
        let relative = read_u32_le(self.payload, table_offset)?;
        if relative == 0 {
            return Ok(None);
        }
        if (relative as usize) < self.segment_table_bytes {
            return Err(FixupError::SegmentInfoOffsetInvalid {
                index,
                offset: relative,
            });
        }

        let record = self
            .starts_base
            .checked_add(relative as usize)
            .ok_or(FixupError::IntegerOverflow)?;
        let fixed_end = checked_end(record, CHAINED_SEGMENT_HEADER_SIZE)?;
        if fixed_end > self.payload.len() {
            return Err(FixupError::SegmentInfoOutOfBounds { index });
        }

        let size = read_u32_le(self.payload, record)?;
        let page_count = read_u16_le(self.payload, record + 20)?;
        let page_bytes = (page_count as usize)
            .checked_mul(2)
            .ok_or(FixupError::IntegerOverflow)?;
        let minimum = CHAINED_SEGMENT_HEADER_SIZE
            .checked_add(page_bytes)
            .ok_or(FixupError::IntegerOverflow)?;
        if (size as usize) < minimum {
            return Err(FixupError::SegmentInfoTooSmall {
                index,
                size,
                minimum,
            });
        }
        let record_end = checked_end(record, size as usize)?;
        if record_end > self.payload.len() {
            return Err(FixupError::SegmentInfoOutOfBounds { index });
        }

        let page_size = read_u16_le(self.payload, record + 4)?;
        if page_size != 0x1000 && page_size != 0x4000 {
            return Err(FixupError::UnsupportedPageSize { index, page_size });
        }
        let page_starts_end = checked_end(fixed_end, page_bytes)?;
        let page_starts = self
            .payload
            .get(fixed_end..page_starts_end)
            .ok_or(FixupError::SegmentInfoOutOfBounds { index })?;

        Ok(Some(ChainedSegmentInfo {
            segment_index: index,
            size,
            page_size,
            pointer_format: read_u16_le(self.payload, record + 6)?,
            segment_offset: read_u64_le(self.payload, record + 8)?,
            max_valid_pointer: read_u32_le(self.payload, record + 16)?,
            page_starts,
            page_count,
        }))
    }
}

fn checked_end(offset: usize, size: usize) -> Result<usize, FixupError> {
    offset.checked_add(size).ok_or(FixupError::IntegerOverflow)
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Result<u16, FixupError> {
    let end = checked_end(offset, 2)?;
    let data = bytes.get(offset..end).ok_or(FixupError::SliceOutOfBounds)?;
    Ok(u16::from_le_bytes([data[0], data[1]]))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, FixupError> {
    let end = checked_end(offset, 4)?;
    let data = bytes.get(offset..end).ok_or(FixupError::SliceOutOfBounds)?;
    Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}

fn read_u64_le(bytes: &[u8], offset: usize) -> Result<u64, FixupError> {
    let end = checked_end(offset, 8)?;
    let data = bytes.get(offset..end).ok_or(FixupError::SliceOutOfBounds)?;
    Ok(u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]))
}

fn selected_image(bytes: &[u8], slice: MachSlice) -> Result<&[u8], FixupError> {
    let end = checked_end(slice.offset, slice.size)?;
    bytes
        .get(slice.offset..end)
        .ok_or(FixupError::SliceOutOfBounds)
}

fn validate_range(
    image_len: usize,
    index: u32,
    offset: u32,
    size: u32,
) -> Result<LinkeditRange, FixupError> {
    let end = u64::from(offset)
        .checked_add(u64::from(size))
        .ok_or(FixupError::IntegerOverflow)?;
    if end > image_len as u64 {
        return Err(FixupError::LinkeditRangeOutOfBounds {
            index,
            offset,
            size,
        });
    }
    Ok(LinkeditRange { offset, size })
}

fn parse_dyld_info(
    image_len: usize,
    index: u32,
    command: u32,
    bytes: &[u8],
) -> Result<DyldInfo, FixupError> {
    if bytes.len() < DYLD_INFO_COMMAND_SIZE {
        return Err(FixupError::CommandTooSmall {
            index,
            command,
            size: bytes.len(),
            minimum: DYLD_INFO_COMMAND_SIZE,
        });
    }

    Ok(DyldInfo {
        command_index: index,
        rebase: validate_range(
            image_len,
            index,
            read_u32_le(bytes, 8)?,
            read_u32_le(bytes, 12)?,
        )?,
        bind: validate_range(
            image_len,
            index,
            read_u32_le(bytes, 16)?,
            read_u32_le(bytes, 20)?,
        )?,
        weak_bind: validate_range(
            image_len,
            index,
            read_u32_le(bytes, 24)?,
            read_u32_le(bytes, 28)?,
        )?,
        lazy_bind: validate_range(
            image_len,
            index,
            read_u32_le(bytes, 32)?,
            read_u32_le(bytes, 36)?,
        )?,
        exports: validate_range(
            image_len,
            index,
            read_u32_le(bytes, 40)?,
            read_u32_le(bytes, 44)?,
        )?,
    })
}

fn parse_linkedit_data(
    image_len: usize,
    index: u32,
    command: u32,
    bytes: &[u8],
) -> Result<LinkeditData, FixupError> {
    if bytes.len() < LINKEDIT_DATA_COMMAND_SIZE {
        return Err(FixupError::CommandTooSmall {
            index,
            command,
            size: bytes.len(),
            minimum: LINKEDIT_DATA_COMMAND_SIZE,
        });
    }
    Ok(LinkeditData {
        command_index: index,
        range: validate_range(
            image_len,
            index,
            read_u32_le(bytes, 8)?,
            read_u32_le(bytes, 12)?,
        )?,
    })
}

pub fn metadata(bytes: &[u8], slice: MachSlice) -> Result<FixupMetadata, FixupError> {
    let image = selected_image(bytes, slice)?;
    let commands_end = checked_end(MACH_HEADER_64_SIZE, slice.header.command_bytes as usize)?;
    if commands_end > image.len() {
        return Err(FixupError::SliceOutOfBounds);
    }

    let mut cursor = MACH_HEADER_64_SIZE;
    let mut dyld_info = None;
    let mut exports_trie = None;
    let mut chained_fixups = None;

    for index in 0..slice.header.command_count {
        let header_end = checked_end(cursor, LOAD_COMMAND_HEADER_SIZE)?;
        if header_end > commands_end {
            return Err(FixupError::LoadCommandOutOfBounds { index });
        }
        let command = read_u32_le(image, cursor)?;
        let size = read_u32_le(image, cursor + 4)?;
        if size < LOAD_COMMAND_HEADER_SIZE as u32 {
            return Err(FixupError::LoadCommandTooSmall { index, size });
        }
        if !size.is_multiple_of(8) {
            return Err(FixupError::LoadCommandMisaligned { index, size });
        }
        let end = checked_end(cursor, size as usize)?;
        if end > commands_end {
            return Err(FixupError::LoadCommandOutOfBounds { index });
        }
        let command_bytes = image
            .get(cursor..end)
            .ok_or(FixupError::LoadCommandOutOfBounds { index })?;

        match command {
            LC_DYLD_INFO | LC_DYLD_INFO_ONLY => {
                if dyld_info.is_some() {
                    return Err(FixupError::DuplicateDyldInfo);
                }
                dyld_info = Some(parse_dyld_info(image.len(), index, command, command_bytes)?);
            }
            LC_DYLD_EXPORTS_TRIE => {
                if exports_trie.is_some() {
                    return Err(FixupError::DuplicateExportsTrie);
                }
                exports_trie = Some(parse_linkedit_data(
                    image.len(),
                    index,
                    command,
                    command_bytes,
                )?);
            }
            LC_DYLD_CHAINED_FIXUPS => {
                if chained_fixups.is_some() {
                    return Err(FixupError::DuplicateChainedFixups);
                }
                chained_fixups = Some(parse_linkedit_data(
                    image.len(),
                    index,
                    command,
                    command_bytes,
                )?);
            }
            _ => {}
        }
        cursor = end;
    }

    if cursor != commands_end {
        return Err(FixupError::LoadCommandRegionMismatch);
    }

    Ok(FixupMetadata {
        dyld_info,
        exports_trie,
        chained_fixups,
    })
}

pub fn linkedit_payload(
    bytes: &[u8],
    slice: MachSlice,
    data: LinkeditData,
) -> Result<&[u8], FixupError> {
    let image = selected_image(bytes, slice)?;
    let start = data.range.offset as usize;
    let end = checked_end(start, data.range.size as usize)?;
    image
        .get(start..end)
        .ok_or(FixupError::LinkeditRangeOutOfBounds {
            index: data.command_index,
            offset: data.range.offset,
            size: data.range.size,
        })
}

pub fn dyld_payload(
    bytes: &[u8],
    slice: MachSlice,
    command_index: u32,
    range: LinkeditRange,
) -> Result<&[u8], FixupError> {
    let image = selected_image(bytes, slice)?;
    let start = range.offset as usize;
    let end = checked_end(start, range.size as usize)?;
    image
        .get(start..end)
        .ok_or(FixupError::LinkeditRangeOutOfBounds {
            index: command_index,
            offset: range.offset,
            size: range.size,
        })
}

fn import_entry_size(format: u32) -> Result<usize, FixupError> {
    match format {
        DYLD_CHAINED_IMPORT => Ok(4),
        DYLD_CHAINED_IMPORT_ADDEND => Ok(8),
        DYLD_CHAINED_IMPORT_ADDEND64 => Ok(16),
        _ => Err(FixupError::UnsupportedImportsFormat(format)),
    }
}

fn validate_chained_offset(payload_len: usize, offset: u32) -> Result<usize, FixupError> {
    let value = offset as usize;
    if value > payload_len {
        return Err(FixupError::ChainedOffsetOutOfBounds { offset });
    }
    Ok(value)
}

pub fn parse_chained_fixups<'a>(
    bytes: &'a [u8],
    slice: MachSlice,
    data: LinkeditData,
) -> Result<ChainedFixups<'a>, FixupError> {
    let payload = linkedit_payload(bytes, slice, data)?;
    if payload.len() < CHAINED_FIXUPS_HEADER_SIZE {
        return Err(FixupError::ChainedHeaderTooShort {
            size: payload.len(),
        });
    }

    let header = ChainedFixupsHeader {
        fixups_version: read_u32_le(payload, 0)?,
        starts_offset: read_u32_le(payload, 4)?,
        imports_offset: read_u32_le(payload, 8)?,
        symbols_offset: read_u32_le(payload, 12)?,
        imports_count: read_u32_le(payload, 16)?,
        imports_format: read_u32_le(payload, 20)?,
        symbols_format: read_u32_le(payload, 24)?,
    };
    if header.fixups_version != 0 {
        return Err(FixupError::UnsupportedFixupsVersion(header.fixups_version));
    }
    if header.symbols_format != DYLD_CHAINED_SYMBOLS_UNCOMPRESSED
        && header.symbols_format != DYLD_CHAINED_SYMBOLS_ZLIB
    {
        return Err(FixupError::UnsupportedSymbolsFormat(header.symbols_format));
    }

    let starts_base = validate_chained_offset(payload.len(), header.starts_offset)?;
    let imports_base = validate_chained_offset(payload.len(), header.imports_offset)?;
    let symbols_base = validate_chained_offset(payload.len(), header.symbols_offset)?;

    let import_size = import_entry_size(header.imports_format)?;
    let imports_bytes = (header.imports_count as usize)
        .checked_mul(import_size)
        .ok_or(FixupError::IntegerOverflow)?;
    let imports_end = checked_end(imports_base, imports_bytes)?;
    if imports_end > payload.len() || (header.imports_count != 0 && symbols_base >= payload.len()) {
        return Err(FixupError::ImportsTableOutOfBounds);
    }

    let starts_count_end = checked_end(starts_base, CHAINED_STARTS_HEADER_SIZE)?;
    if starts_count_end > payload.len() {
        return Err(FixupError::StartsTableOutOfBounds);
    }
    let segment_count = read_u32_le(payload, starts_base)?;
    let offsets_bytes = (segment_count as usize)
        .checked_mul(4)
        .ok_or(FixupError::IntegerOverflow)?;
    let segment_table_bytes = CHAINED_STARTS_HEADER_SIZE
        .checked_add(offsets_bytes)
        .ok_or(FixupError::IntegerOverflow)?;
    let starts_table_end = checked_end(starts_base, segment_table_bytes)?;
    if starts_table_end > payload.len() {
        return Err(FixupError::StartsTableOutOfBounds);
    }

    let parsed = ChainedFixups {
        header,
        payload,
        starts_base,
        segment_count,
        segment_table_bytes,
    };

    for index in 0..segment_count {
        let _ = parsed.segment(index)?;
    }

    Ok(parsed)
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use aw_macho::{CpuType, select_mach_o_64};
    use std::vec;
    use std::vec::Vec;

    const MH_MAGIC_64: u32 = 0xfeed_facf;
    const MH_EXECUTE: u32 = 0x2;
    const CPU_TYPE_X86_64: u32 = 0x0100_0007;

    fn set_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn set_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn set_u64(bytes: &mut [u8], offset: usize, value: u64) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn command(command: u32, size: usize) -> Vec<u8> {
        let mut out = vec![0u8; size];
        set_u32(&mut out, 0, command);
        set_u32(&mut out, 4, size as u32);
        out
    }

    fn linkedit_command(command_id: u32, offset: u32, size: u32) -> Vec<u8> {
        let mut out = command(command_id, LINKEDIT_DATA_COMMAND_SIZE);
        set_u32(&mut out, 8, offset);
        set_u32(&mut out, 12, size);
        out
    }

    fn fixture(commands: &[Vec<u8>], payload: &[u8]) -> Vec<u8> {
        let command_bytes: usize = commands.iter().map(Vec::len).sum();
        let mut out = Vec::with_capacity(MACH_HEADER_64_SIZE + command_bytes + payload.len());
        out.extend_from_slice(&MH_MAGIC_64.to_le_bytes());
        out.extend_from_slice(&CPU_TYPE_X86_64.to_le_bytes());
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(&MH_EXECUTE.to_le_bytes());
        out.extend_from_slice(&(commands.len() as u32).to_le_bytes());
        out.extend_from_slice(&(command_bytes as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        for item in commands {
            out.extend_from_slice(item);
        }
        out.extend_from_slice(payload);
        out
    }

    fn selected(image: &[u8]) -> MachSlice {
        select_mach_o_64(image, CpuType::X86_64).unwrap()
    }

    #[test]
    fn parses_legacy_dyld_info_and_modern_linkedit_commands() {
        let payload_start =
            MACH_HEADER_64_SIZE + DYLD_INFO_COMMAND_SIZE + 2 * LINKEDIT_DATA_COMMAND_SIZE;
        let mut dyld = command(LC_DYLD_INFO_ONLY, DYLD_INFO_COMMAND_SIZE);
        set_u32(&mut dyld, 8, payload_start as u32);
        set_u32(&mut dyld, 12, 4);
        set_u32(&mut dyld, 16, (payload_start + 4) as u32);
        set_u32(&mut dyld, 20, 4);
        set_u32(&mut dyld, 24, (payload_start + 8) as u32);
        set_u32(&mut dyld, 28, 2);
        set_u32(&mut dyld, 32, (payload_start + 10) as u32);
        set_u32(&mut dyld, 36, 2);
        set_u32(&mut dyld, 40, (payload_start + 12) as u32);
        set_u32(&mut dyld, 44, 4);

        let exports = linkedit_command(LC_DYLD_EXPORTS_TRIE, (payload_start + 12) as u32, 4);
        let chained = linkedit_command(LC_DYLD_CHAINED_FIXUPS, (payload_start + 16) as u32, 28);
        let image = fixture(&[dyld, exports, chained], &[0u8; 44]);
        let parsed = metadata(&image, selected(&image)).unwrap();

        assert_eq!(parsed.dyld_info.unwrap().bind.size, 4);
        assert_eq!(parsed.exports_trie.unwrap().range.size, 4);
        assert_eq!(parsed.chained_fixups.unwrap().range.size, 28);
    }

    #[test]
    fn rejects_duplicate_chained_fixups() {
        let payload_start = MACH_HEADER_64_SIZE + 2 * LINKEDIT_DATA_COMMAND_SIZE;
        let first = linkedit_command(LC_DYLD_CHAINED_FIXUPS, payload_start as u32, 0);
        let second = linkedit_command(LC_DYLD_CHAINED_FIXUPS, payload_start as u32, 0);
        let image = fixture(&[first, second], &[]);
        assert_eq!(
            metadata(&image, selected(&image)),
            Err(FixupError::DuplicateChainedFixups)
        );
    }

    #[test]
    fn rejects_linkedit_range_out_of_slice() {
        let payload_start = MACH_HEADER_64_SIZE + LINKEDIT_DATA_COMMAND_SIZE;
        let chained = linkedit_command(LC_DYLD_CHAINED_FIXUPS, payload_start as u32, 64);
        let image = fixture(&[chained], &[0u8; 8]);
        assert_eq!(
            metadata(&image, selected(&image)),
            Err(FixupError::LinkeditRangeOutOfBounds {
                index: 0,
                offset: payload_start as u32,
                size: 64,
            })
        );
    }

    fn chained_payload() -> Vec<u8> {
        let starts_offset = CHAINED_FIXUPS_HEADER_SIZE;
        let starts_table_size = 12usize;
        let segment_offset = starts_offset + starts_table_size;
        let segment_size = CHAINED_SEGMENT_HEADER_SIZE + 4;
        let imports_offset = segment_offset + segment_size;
        let symbols_offset = imports_offset + 4;
        let mut payload = vec![0u8; symbols_offset + 4];

        set_u32(&mut payload, 0, 0);
        set_u32(&mut payload, 4, starts_offset as u32);
        set_u32(&mut payload, 8, imports_offset as u32);
        set_u32(&mut payload, 12, symbols_offset as u32);
        set_u32(&mut payload, 16, 1);
        set_u32(&mut payload, 20, DYLD_CHAINED_IMPORT);
        set_u32(&mut payload, 24, DYLD_CHAINED_SYMBOLS_UNCOMPRESSED);

        set_u32(&mut payload, starts_offset, 2);
        set_u32(&mut payload, starts_offset + 4, 0);
        set_u32(&mut payload, starts_offset + 8, starts_table_size as u32);

        set_u32(&mut payload, segment_offset, segment_size as u32);
        set_u16(&mut payload, segment_offset + 4, 0x1000);
        set_u16(&mut payload, segment_offset + 6, 2);
        set_u64(&mut payload, segment_offset + 8, 0x4000);
        set_u32(&mut payload, segment_offset + 16, 0);
        set_u16(&mut payload, segment_offset + 20, 2);
        set_u16(&mut payload, segment_offset + 22, 0x20);
        set_u16(&mut payload, segment_offset + 24, 0xffff);

        payload[symbols_offset..symbols_offset + 4].copy_from_slice(b"foo\0");
        payload
    }

    #[test]
    fn parses_bounded_chained_fixups_header_and_segment_starts() {
        let payload = chained_payload();
        let payload_start = MACH_HEADER_64_SIZE + LINKEDIT_DATA_COMMAND_SIZE;
        let chained = linkedit_command(
            LC_DYLD_CHAINED_FIXUPS,
            payload_start as u32,
            payload.len() as u32,
        );
        let image = fixture(&[chained], &payload);
        let slice = selected(&image);
        let data = metadata(&image, slice).unwrap().chained_fixups.unwrap();
        let parsed = parse_chained_fixups(&image, slice, data).unwrap();

        assert_eq!(parsed.segment_count(), 2);
        assert!(parsed.segment(0).unwrap().is_none());
        let segment = parsed.segment(1).unwrap().unwrap();
        assert_eq!(segment.page_size, 0x1000);
        assert_eq!(segment.pointer_format, 2);
        assert_eq!(segment.segment_offset, 0x4000);
        assert_eq!(segment.page_count(), 2);
        assert_eq!(segment.page_start(0).unwrap(), 0x20);
        assert_eq!(segment.page_start(1).unwrap(), 0xffff);
    }

    #[test]
    fn rejects_unknown_chained_fixups_version() {
        let mut payload = chained_payload();
        set_u32(&mut payload, 0, 1);
        let payload_start = MACH_HEADER_64_SIZE + LINKEDIT_DATA_COMMAND_SIZE;
        let chained = linkedit_command(
            LC_DYLD_CHAINED_FIXUPS,
            payload_start as u32,
            payload.len() as u32,
        );
        let image = fixture(&[chained], &payload);
        let slice = selected(&image);
        let data = metadata(&image, slice).unwrap().chained_fixups.unwrap();
        assert_eq!(
            parse_chained_fixups(&image, slice, data),
            Err(FixupError::UnsupportedFixupsVersion(1))
        );
    }

    #[test]
    fn rejects_segment_info_inside_starts_table() {
        let mut payload = chained_payload();
        set_u32(&mut payload, CHAINED_FIXUPS_HEADER_SIZE + 8, 4);
        let payload_start = MACH_HEADER_64_SIZE + LINKEDIT_DATA_COMMAND_SIZE;
        let chained = linkedit_command(
            LC_DYLD_CHAINED_FIXUPS,
            payload_start as u32,
            payload.len() as u32,
        );
        let image = fixture(&[chained], &payload);
        let slice = selected(&image);
        let data = metadata(&image, slice).unwrap().chained_fixups.unwrap();
        assert_eq!(
            parse_chained_fixups(&image, slice, data),
            Err(FixupError::SegmentInfoOffsetInvalid {
                index: 1,
                offset: 4,
            })
        );
    }
}
