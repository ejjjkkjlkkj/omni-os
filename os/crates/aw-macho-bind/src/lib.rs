#![no_std]
#![forbid(unsafe_code)]

pub mod legacy;

use aw_macho_fixups::ChainedFixups;

const DYLD_CHAINED_IMPORT: u32 = 1;
const DYLD_CHAINED_IMPORT_ADDEND: u32 = 2;
const DYLD_CHAINED_IMPORT_ADDEND64: u32 = 3;
const DYLD_CHAINED_SYMBOLS_UNCOMPRESSED: u32 = 0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindError {
    IntegerOverflow,
    UnsupportedImportsFormat(u32),
    UnsupportedSymbolsFormat(u32),
    ImportsTableOutOfBounds,
    ImportsOverlapSymbols,
    ImportIndexOutOfBounds { index: u32, count: u32 },
    ReservedBitsNonZero { index: u32, bits: u16 },
    SymbolOffsetOutOfBounds { index: u32, offset: u32 },
    UnterminatedSymbol { index: u32 },
    EmptySymbol { index: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportAddend {
    None,
    Signed32(i32),
    Raw64(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChainedImport<'a> {
    pub index: u32,
    pub lib_ordinal: i16,
    pub weak_import: bool,
    pub name: &'a [u8],
    pub addend: ImportAddend,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChainedImportIter<'a> {
    fixups: ChainedFixups<'a>,
    next_index: u32,
    failed: bool,
}

fn checked_end(offset: usize, size: usize) -> Result<usize, BindError> {
    offset.checked_add(size).ok_or(BindError::IntegerOverflow)
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, BindError> {
    let end = checked_end(offset, 4)?;
    let data = bytes
        .get(offset..end)
        .ok_or(BindError::ImportsTableOutOfBounds)?;
    Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}

fn read_u64_le(bytes: &[u8], offset: usize) -> Result<u64, BindError> {
    let end = checked_end(offset, 8)?;
    let data = bytes
        .get(offset..end)
        .ok_or(BindError::ImportsTableOutOfBounds)?;
    Ok(u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]))
}

fn entry_size(format: u32) -> Result<usize, BindError> {
    match format {
        DYLD_CHAINED_IMPORT => Ok(4),
        DYLD_CHAINED_IMPORT_ADDEND => Ok(8),
        DYLD_CHAINED_IMPORT_ADDEND64 => Ok(16),
        _ => Err(BindError::UnsupportedImportsFormat(format)),
    }
}

fn validated_layout(fixups: ChainedFixups<'_>) -> Result<(usize, usize, usize), BindError> {
    if fixups.header.symbols_format != DYLD_CHAINED_SYMBOLS_UNCOMPRESSED {
        return Err(BindError::UnsupportedSymbolsFormat(
            fixups.header.symbols_format,
        ));
    }

    let size = entry_size(fixups.header.imports_format)?;
    let imports_base = fixups.header.imports_offset as usize;
    let symbols_base = fixups.header.symbols_offset as usize;
    let imports_bytes = (fixups.header.imports_count as usize)
        .checked_mul(size)
        .ok_or(BindError::IntegerOverflow)?;
    let imports_end = checked_end(imports_base, imports_bytes)?;
    if imports_end > fixups.payload().len() || symbols_base > fixups.payload().len() {
        return Err(BindError::ImportsTableOutOfBounds);
    }
    if imports_end > symbols_base {
        return Err(BindError::ImportsOverlapSymbols);
    }
    Ok((imports_base, symbols_base, size))
}

fn symbol_name(
    payload: &[u8],
    symbols_base: usize,
    index: u32,
    name_offset: u32,
) -> Result<&[u8], BindError> {
    let start = symbols_base
        .checked_add(name_offset as usize)
        .ok_or(BindError::IntegerOverflow)?;
    let tail = payload
        .get(start..)
        .ok_or(BindError::SymbolOffsetOutOfBounds {
            index,
            offset: name_offset,
        })?;
    let end = tail
        .iter()
        .position(|byte| *byte == 0)
        .ok_or(BindError::UnterminatedSymbol { index })?;
    if end == 0 {
        return Err(BindError::EmptySymbol { index });
    }
    Ok(&tail[..end])
}

pub fn chained_import<'a>(
    fixups: ChainedFixups<'a>,
    index: u32,
) -> Result<ChainedImport<'a>, BindError> {
    if index >= fixups.header.imports_count {
        return Err(BindError::ImportIndexOutOfBounds {
            index,
            count: fixups.header.imports_count,
        });
    }

    let (imports_base, symbols_base, size) = validated_layout(fixups)?;
    let entry_offset = imports_base
        .checked_add(
            (index as usize)
                .checked_mul(size)
                .ok_or(BindError::IntegerOverflow)?,
        )
        .ok_or(BindError::IntegerOverflow)?;
    let payload = fixups.payload();

    let (lib_ordinal, weak_import, name_offset, addend) = match fixups.header.imports_format {
        DYLD_CHAINED_IMPORT | DYLD_CHAINED_IMPORT_ADDEND => {
            let raw = read_u32_le(payload, entry_offset)?;
            let lib_ordinal = (raw as u8 as i8) as i16;
            let weak_import = ((raw >> 8) & 1) != 0;
            let name_offset = raw >> 9;
            let addend = if fixups.header.imports_format == DYLD_CHAINED_IMPORT_ADDEND {
                ImportAddend::Signed32(read_u32_le(payload, entry_offset + 4)? as i32)
            } else {
                ImportAddend::None
            };
            (lib_ordinal, weak_import, name_offset, addend)
        }
        DYLD_CHAINED_IMPORT_ADDEND64 => {
            let raw = read_u64_le(payload, entry_offset)?;
            let reserved = ((raw >> 17) & 0x7fff) as u16;
            if reserved != 0 {
                return Err(BindError::ReservedBitsNonZero {
                    index,
                    bits: reserved,
                });
            }
            let lib_ordinal = (raw as u16) as i16;
            let weak_import = ((raw >> 16) & 1) != 0;
            let name_offset = (raw >> 32) as u32;
            let addend = ImportAddend::Raw64(read_u64_le(payload, entry_offset + 8)?);
            (lib_ordinal, weak_import, name_offset, addend)
        }
        format => return Err(BindError::UnsupportedImportsFormat(format)),
    };

    Ok(ChainedImport {
        index,
        lib_ordinal,
        weak_import,
        name: symbol_name(payload, symbols_base, index, name_offset)?,
        addend,
    })
}

pub fn chained_imports(fixups: ChainedFixups<'_>) -> Result<ChainedImportIter<'_>, BindError> {
    let _ = validated_layout(fixups)?;
    Ok(ChainedImportIter {
        fixups,
        next_index: 0,
        failed: false,
    })
}

impl<'a> Iterator for ChainedImportIter<'a> {
    type Item = Result<ChainedImport<'a>, BindError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.next_index >= self.fixups.header.imports_count {
            return None;
        }
        let index = self.next_index;
        self.next_index += 1;
        let result = chained_import(self.fixups, index);
        if result.is_err() {
            self.failed = true;
        }
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use aw_macho::{CpuType, select_mach_o_64};
    use aw_macho_fixups::{LinkeditData, LinkeditRange, parse_chained_fixups};
    use std::vec;
    use std::vec::Vec;

    const MH_MAGIC_64: u32 = 0xfeed_facf;
    const MH_EXECUTE: u32 = 0x2;
    const CPU_TYPE_X86_64: u32 = 0x0100_0007;
    const HEADER_SIZE: usize = 32;
    const FIXUPS_HEADER_SIZE: usize = 28;

    fn set_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn set_u64(bytes: &mut [u8], offset: usize, value: u64) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn image_with_fixups(
        format: u32,
        symbols_format: u32,
        entries: &[u8],
        symbols: &[u8],
    ) -> Vec<u8> {
        let starts_offset = FIXUPS_HEADER_SIZE;
        let starts_size = 4usize;
        let imports_offset = starts_offset + starts_size;
        let symbols_offset = imports_offset + entries.len();
        let mut payload = vec![0u8; symbols_offset + symbols.len()];
        set_u32(&mut payload, 0, 0);
        set_u32(&mut payload, 4, starts_offset as u32);
        set_u32(&mut payload, 8, imports_offset as u32);
        set_u32(&mut payload, 12, symbols_offset as u32);
        set_u32(
            &mut payload,
            16,
            (entries.len() / entry_size(format).unwrap()) as u32,
        );
        set_u32(&mut payload, 20, format);
        set_u32(&mut payload, 24, symbols_format);
        set_u32(&mut payload, starts_offset, 0);
        payload[imports_offset..symbols_offset].copy_from_slice(entries);
        payload[symbols_offset..].copy_from_slice(symbols);

        let mut image = Vec::with_capacity(HEADER_SIZE + payload.len());
        image.extend_from_slice(&MH_MAGIC_64.to_le_bytes());
        image.extend_from_slice(&CPU_TYPE_X86_64.to_le_bytes());
        image.extend_from_slice(&3u32.to_le_bytes());
        image.extend_from_slice(&MH_EXECUTE.to_le_bytes());
        image.extend_from_slice(&0u32.to_le_bytes());
        image.extend_from_slice(&0u32.to_le_bytes());
        image.extend_from_slice(&0u32.to_le_bytes());
        image.extend_from_slice(&0u32.to_le_bytes());
        image.extend_from_slice(&payload);
        image
    }

    fn parsed(image: &[u8]) -> ChainedFixups<'_> {
        let slice = select_mach_o_64(image, CpuType::X86_64).unwrap();
        parse_chained_fixups(
            image,
            slice,
            LinkeditData {
                command_index: 0,
                range: LinkeditRange {
                    offset: HEADER_SIZE as u32,
                    size: (image.len() - HEADER_SIZE) as u32,
                },
            },
        )
        .unwrap()
    }

    #[test]
    fn decodes_plain_import_and_signed_ordinal() {
        let raw = u32::from(0xffu8) | (1 << 8);
        let image = image_with_fixups(DYLD_CHAINED_IMPORT, 0, &raw.to_le_bytes(), b"foo\0");
        let import = chained_import(parsed(&image), 0).unwrap();
        assert_eq!(import.lib_ordinal, -1);
        assert!(import.weak_import);
        assert_eq!(import.name, b"foo");
        assert_eq!(import.addend, ImportAddend::None);
    }

    #[test]
    fn decodes_signed32_addend_and_symbol_offset() {
        let first = b"skip\0";
        let raw = 2u32 | ((first.len() as u32) << 9);
        let mut entry = vec![0u8; 8];
        set_u32(&mut entry, 0, raw);
        set_u32(&mut entry, 4, (-7i32) as u32);
        let image = image_with_fixups(DYLD_CHAINED_IMPORT_ADDEND, 0, &entry, b"skip\0target\0");
        let import = chained_import(parsed(&image), 0).unwrap();
        assert_eq!(import.lib_ordinal, 2);
        assert_eq!(import.name, b"target");
        assert_eq!(import.addend, ImportAddend::Signed32(-7));
    }

    #[test]
    fn decodes_addend64_import() {
        let raw = u64::from(0xfffeu16) | (1 << 16);
        let mut entry = vec![0u8; 16];
        set_u64(&mut entry, 0, raw);
        set_u64(&mut entry, 8, 0x1122_3344_5566_7788);
        let image = image_with_fixups(DYLD_CHAINED_IMPORT_ADDEND64, 0, &entry, b"symbol64\0");
        let import = chained_import(parsed(&image), 0).unwrap();
        assert_eq!(import.lib_ordinal, -2);
        assert!(import.weak_import);
        assert_eq!(import.name, b"symbol64");
        assert_eq!(import.addend, ImportAddend::Raw64(0x1122_3344_5566_7788));
    }

    #[test]
    fn rejects_reserved_bits_in_addend64() {
        let raw = 1u64 | (1 << 17);
        let mut entry = vec![0u8; 16];
        set_u64(&mut entry, 0, raw);
        let image = image_with_fixups(DYLD_CHAINED_IMPORT_ADDEND64, 0, &entry, b"x\0");
        assert_eq!(
            chained_import(parsed(&image), 0),
            Err(BindError::ReservedBitsNonZero { index: 0, bits: 1 })
        );
    }

    #[test]
    fn rejects_compressed_symbol_pool_until_supported() {
        let raw = 1u32;
        let image = image_with_fixups(DYLD_CHAINED_IMPORT, 1, &raw.to_le_bytes(), b"x\0");
        assert_eq!(
            chained_imports(parsed(&image)),
            Err(BindError::UnsupportedSymbolsFormat(1))
        );
    }

    #[test]
    fn iterator_preserves_import_order() {
        let first = 1u32;
        let second = 2u32 | (2 << 9);
        let mut entries = vec![0u8; 8];
        set_u32(&mut entries, 0, first);
        set_u32(&mut entries, 4, second);
        let image = image_with_fixups(DYLD_CHAINED_IMPORT, 0, &entries, b"a\0b\0");
        let parsed = parsed(&image);
        let imports: Vec<_> = chained_imports(parsed)
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].name, b"a");
        assert_eq!(imports[1].name, b"b");
        assert_eq!(imports[1].lib_ordinal, 2);
    }
}
