//! Bounded decoder for legacy `LC_DYLD_INFO[_ONLY]` rebase and bind bytecode.
//!
//! The decoder never mutates image memory and never resolves symbols. It only
//! turns the byte streams described by `dyld_info_command` into typed opcodes.
//! Applying those opcodes is intentionally a separate validation step.

const OPCODE_MASK: u8 = 0xf0;
const IMMEDIATE_MASK: u8 = 0x0f;

const REBASE_OPCODE_DONE: u8 = 0x00;
const REBASE_OPCODE_SET_TYPE_IMM: u8 = 0x10;
const REBASE_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB: u8 = 0x20;
const REBASE_OPCODE_ADD_ADDR_ULEB: u8 = 0x30;
const REBASE_OPCODE_ADD_ADDR_IMM_SCALED: u8 = 0x40;
const REBASE_OPCODE_DO_REBASE_IMM_TIMES: u8 = 0x50;
const REBASE_OPCODE_DO_REBASE_ULEB_TIMES: u8 = 0x60;
const REBASE_OPCODE_DO_REBASE_ADD_ADDR_ULEB: u8 = 0x70;
const REBASE_OPCODE_DO_REBASE_ULEB_TIMES_SKIPPING_ULEB: u8 = 0x80;

const BIND_OPCODE_DONE: u8 = 0x00;
const BIND_OPCODE_SET_DYLIB_ORDINAL_IMM: u8 = 0x10;
const BIND_OPCODE_SET_DYLIB_ORDINAL_ULEB: u8 = 0x20;
const BIND_OPCODE_SET_DYLIB_SPECIAL_IMM: u8 = 0x30;
const BIND_OPCODE_SET_SYMBOL_TRAILING_FLAGS_IMM: u8 = 0x40;
const BIND_OPCODE_SET_TYPE_IMM: u8 = 0x50;
const BIND_OPCODE_SET_ADDEND_SLEB: u8 = 0x60;
const BIND_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB: u8 = 0x70;
const BIND_OPCODE_ADD_ADDR_ULEB: u8 = 0x80;
const BIND_OPCODE_DO_BIND: u8 = 0x90;
const BIND_OPCODE_DO_BIND_ADD_ADDR_ULEB: u8 = 0xa0;
const BIND_OPCODE_DO_BIND_ADD_ADDR_IMM_SCALED: u8 = 0xb0;
const BIND_OPCODE_DO_BIND_ULEB_TIMES_SKIPPING_ULEB: u8 = 0xc0;

/// Maximum padding accepted after `REBASE_OPCODE_DONE`, matching dyld's
/// bounded tolerance for historical 16-byte alignment.
const MAX_REBASE_PADDING: usize = 15;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LegacyError {
    UnexpectedEnd { offset: usize },
    UlebOverflow { offset: usize },
    SlebOverflow { offset: usize },
    UnterminatedSymbol { offset: usize },
    EmptySymbol { offset: usize },
    UnknownRebaseOpcode { offset: usize, opcode: u8 },
    UnknownBindOpcode { offset: usize, opcode: u8 },
    ExcessiveRebasePadding { offset: usize, remaining: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Decoded<T> {
    pub offset: usize,
    pub opcode: T,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RebaseOpcode {
    Done,
    SetType { kind: u8 },
    SetSegmentAndOffset { segment: u8, offset: u64 },
    AddAddress { amount: u64 },
    AddAddressScaled { scale: u8 },
    DoRebaseImmediateTimes { count: u8 },
    DoRebaseUlebTimes { count: u64 },
    DoRebaseAddAddress { amount: u64 },
    DoRebaseUlebTimesSkipping { count: u64, skip: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindOpcode<'a> {
    Done,
    SetDylibOrdinalImmediate { ordinal: u8 },
    SetDylibOrdinalUleb { ordinal: u64 },
    SetDylibSpecialImmediate { ordinal: i8 },
    SetSymbolTrailingFlags { flags: u8, symbol: &'a [u8] },
    SetType { kind: u8 },
    SetAddend { addend: i64 },
    SetSegmentAndOffset { segment: u8, offset: u64 },
    AddAddress { amount: u64 },
    DoBind,
    DoBindAddAddress { amount: u64 },
    DoBindAddAddressScaled { scale: u8 },
    DoBindUlebTimesSkipping { count: u64, skip: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RebaseOpcodeIter<'a> {
    bytes: &'a [u8],
    cursor: usize,
    failed: bool,
    done: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BindOpcodeIter<'a> {
    bytes: &'a [u8],
    cursor: usize,
    failed: bool,
}

#[must_use]
pub const fn rebase_opcodes(bytes: &[u8]) -> RebaseOpcodeIter<'_> {
    RebaseOpcodeIter {
        bytes,
        cursor: 0,
        failed: false,
        done: false,
    }
}

#[must_use]
pub const fn bind_opcodes(bytes: &[u8]) -> BindOpcodeIter<'_> {
    BindOpcodeIter {
        bytes,
        cursor: 0,
        failed: false,
    }
}

fn read_byte(bytes: &[u8], cursor: &mut usize) -> Result<u8, LegacyError> {
    let offset = *cursor;
    let byte = *bytes
        .get(offset)
        .ok_or(LegacyError::UnexpectedEnd { offset })?;
    *cursor = offset + 1;
    Ok(byte)
}

fn read_uleb(bytes: &[u8], cursor: &mut usize) -> Result<u64, LegacyError> {
    let start = *cursor;
    let mut value = 0u64;
    let mut shift = 0u32;

    for _ in 0..10 {
        let byte = read_byte(bytes, cursor)?;
        let payload = u64::from(byte & 0x7f);
        if shift == 63 && payload > 1 {
            return Err(LegacyError::UlebOverflow { offset: start });
        }
        value |= payload
            .checked_shl(shift)
            .ok_or(LegacyError::UlebOverflow { offset: start })?;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        if shift >= 63 {
            return Err(LegacyError::UlebOverflow { offset: start });
        }
        shift += 7;
    }

    Err(LegacyError::UlebOverflow { offset: start })
}

fn read_sleb(bytes: &[u8], cursor: &mut usize) -> Result<i64, LegacyError> {
    let start = *cursor;
    let mut value = 0i128;
    let mut shift = 0u32;

    for _ in 0..10 {
        let byte = read_byte(bytes, cursor)?;
        value |= i128::from(byte & 0x7f) << shift;
        shift += 7;

        if byte & 0x80 == 0 {
            if byte & 0x40 != 0 {
                value |= (!0i128) << shift;
            }
            return i64::try_from(value).map_err(|_| LegacyError::SlebOverflow { offset: start });
        }
    }

    Err(LegacyError::SlebOverflow { offset: start })
}

fn read_symbol<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    opcode_offset: usize,
) -> Result<&'a [u8], LegacyError> {
    let start = *cursor;
    let tail = bytes
        .get(start..)
        .ok_or(LegacyError::UnexpectedEnd { offset: start })?;
    let length =
        tail.iter()
            .position(|byte| *byte == 0)
            .ok_or(LegacyError::UnterminatedSymbol {
                offset: opcode_offset,
            })?;
    if length == 0 {
        return Err(LegacyError::EmptySymbol {
            offset: opcode_offset,
        });
    }
    *cursor = start + length + 1;
    Ok(&tail[..length])
}

impl Iterator for RebaseOpcodeIter<'_> {
    type Item = Result<Decoded<RebaseOpcode>, LegacyError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.done || self.cursor >= self.bytes.len() {
            return None;
        }

        let offset = self.cursor;
        let byte = match read_byte(self.bytes, &mut self.cursor) {
            Ok(byte) => byte,
            Err(error) => {
                self.failed = true;
                return Some(Err(error));
            }
        };
        let opcode = byte & OPCODE_MASK;
        let immediate = byte & IMMEDIATE_MASK;

        let decoded = match opcode {
            REBASE_OPCODE_DONE => {
                let remaining = self.bytes.len() - self.cursor;
                if remaining > MAX_REBASE_PADDING {
                    Err(LegacyError::ExcessiveRebasePadding { offset, remaining })
                } else {
                    self.done = true;
                    Ok(RebaseOpcode::Done)
                }
            }
            REBASE_OPCODE_SET_TYPE_IMM => Ok(RebaseOpcode::SetType { kind: immediate }),
            REBASE_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB => read_uleb(self.bytes, &mut self.cursor)
                .map(|stream_offset| RebaseOpcode::SetSegmentAndOffset {
                    segment: immediate,
                    offset: stream_offset,
                }),
            REBASE_OPCODE_ADD_ADDR_ULEB => read_uleb(self.bytes, &mut self.cursor)
                .map(|amount| RebaseOpcode::AddAddress { amount }),
            REBASE_OPCODE_ADD_ADDR_IMM_SCALED => {
                Ok(RebaseOpcode::AddAddressScaled { scale: immediate })
            }
            REBASE_OPCODE_DO_REBASE_IMM_TIMES => {
                Ok(RebaseOpcode::DoRebaseImmediateTimes { count: immediate })
            }
            REBASE_OPCODE_DO_REBASE_ULEB_TIMES => read_uleb(self.bytes, &mut self.cursor)
                .map(|count| RebaseOpcode::DoRebaseUlebTimes { count }),
            REBASE_OPCODE_DO_REBASE_ADD_ADDR_ULEB => read_uleb(self.bytes, &mut self.cursor)
                .map(|amount| RebaseOpcode::DoRebaseAddAddress { amount }),
            REBASE_OPCODE_DO_REBASE_ULEB_TIMES_SKIPPING_ULEB => {
                match read_uleb(self.bytes, &mut self.cursor) {
                    Ok(count) => read_uleb(self.bytes, &mut self.cursor)
                        .map(|skip| RebaseOpcode::DoRebaseUlebTimesSkipping { count, skip }),
                    Err(error) => Err(error),
                }
            }
            _ => Err(LegacyError::UnknownRebaseOpcode { offset, opcode }),
        };

        if decoded.is_err() {
            self.failed = true;
        }
        Some(decoded.map(|opcode| Decoded { offset, opcode }))
    }
}

impl<'a> Iterator for BindOpcodeIter<'a> {
    type Item = Result<Decoded<BindOpcode<'a>>, LegacyError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.cursor >= self.bytes.len() {
            return None;
        }

        let offset = self.cursor;
        let byte = match read_byte(self.bytes, &mut self.cursor) {
            Ok(byte) => byte,
            Err(error) => {
                self.failed = true;
                return Some(Err(error));
            }
        };
        let opcode = byte & OPCODE_MASK;
        let immediate = byte & IMMEDIATE_MASK;

        let decoded =
            match opcode {
                BIND_OPCODE_DONE => Ok(BindOpcode::Done),
                BIND_OPCODE_SET_DYLIB_ORDINAL_IMM => {
                    Ok(BindOpcode::SetDylibOrdinalImmediate { ordinal: immediate })
                }
                BIND_OPCODE_SET_DYLIB_ORDINAL_ULEB => read_uleb(self.bytes, &mut self.cursor)
                    .map(|ordinal| BindOpcode::SetDylibOrdinalUleb { ordinal }),
                BIND_OPCODE_SET_DYLIB_SPECIAL_IMM => {
                    let ordinal = if immediate == 0 {
                        0
                    } else {
                        (OPCODE_MASK | immediate) as i8
                    };
                    Ok(BindOpcode::SetDylibSpecialImmediate { ordinal })
                }
                BIND_OPCODE_SET_SYMBOL_TRAILING_FLAGS_IMM => {
                    read_symbol(self.bytes, &mut self.cursor, offset).map(|symbol| {
                        BindOpcode::SetSymbolTrailingFlags {
                            flags: immediate,
                            symbol,
                        }
                    })
                }
                BIND_OPCODE_SET_TYPE_IMM => Ok(BindOpcode::SetType { kind: immediate }),
                BIND_OPCODE_SET_ADDEND_SLEB => read_sleb(self.bytes, &mut self.cursor)
                    .map(|addend| BindOpcode::SetAddend { addend }),
                BIND_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB => read_uleb(self.bytes, &mut self.cursor)
                    .map(|stream_offset| BindOpcode::SetSegmentAndOffset {
                        segment: immediate,
                        offset: stream_offset,
                    }),
                BIND_OPCODE_ADD_ADDR_ULEB => read_uleb(self.bytes, &mut self.cursor)
                    .map(|amount| BindOpcode::AddAddress { amount }),
                BIND_OPCODE_DO_BIND => Ok(BindOpcode::DoBind),
                BIND_OPCODE_DO_BIND_ADD_ADDR_ULEB => read_uleb(self.bytes, &mut self.cursor)
                    .map(|amount| BindOpcode::DoBindAddAddress { amount }),
                BIND_OPCODE_DO_BIND_ADD_ADDR_IMM_SCALED => {
                    Ok(BindOpcode::DoBindAddAddressScaled { scale: immediate })
                }
                BIND_OPCODE_DO_BIND_ULEB_TIMES_SKIPPING_ULEB => {
                    match read_uleb(self.bytes, &mut self.cursor) {
                        Ok(count) => read_uleb(self.bytes, &mut self.cursor)
                            .map(|skip| BindOpcode::DoBindUlebTimesSkipping { count, skip }),
                        Err(error) => Err(error),
                    }
                }
                _ => Err(LegacyError::UnknownBindOpcode { offset, opcode }),
            };

        if decoded.is_err() {
            self.failed = true;
        }
        Some(decoded.map(|opcode| Decoded { offset, opcode }))
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::vec;
    use std::vec::Vec;

    #[test]
    fn decodes_rebase_sequence_and_stops_at_done() {
        let bytes = [0x11, 0x22, 0x81, 0x01, 0x30, 0x10, 0x53, 0x00, 0xaa, 0xbb];
        let decoded: Vec<_> = rebase_opcodes(&bytes).map(Result::unwrap).collect();
        assert_eq!(decoded.len(), 5);
        assert_eq!(decoded[0].opcode, RebaseOpcode::SetType { kind: 1 });
        assert_eq!(
            decoded[1].opcode,
            RebaseOpcode::SetSegmentAndOffset {
                segment: 2,
                offset: 129,
            }
        );
        assert_eq!(decoded[2].opcode, RebaseOpcode::AddAddress { amount: 16 });
        assert_eq!(
            decoded[3].opcode,
            RebaseOpcode::DoRebaseImmediateTimes { count: 3 }
        );
        assert_eq!(decoded[4].opcode, RebaseOpcode::Done);
    }

    #[test]
    fn rejects_excessive_rebase_padding() {
        let bytes = vec![0u8; 17];
        assert_eq!(
            rebase_opcodes(&bytes).next(),
            Some(Err(LegacyError::ExcessiveRebasePadding {
                offset: 0,
                remaining: 16,
            }))
        );
    }

    #[test]
    fn decodes_bind_symbol_special_ordinal_and_negative_addend() {
        let bytes = [
            0x3f, 0x41, b'f', b'o', b'o', 0, 0x60, 0x7e, 0x72, 0x05, 0x90, 0x00,
        ];
        let decoded: Vec<_> = bind_opcodes(&bytes).map(Result::unwrap).collect();
        assert_eq!(decoded.len(), 6);
        assert_eq!(
            decoded[0].opcode,
            BindOpcode::SetDylibSpecialImmediate { ordinal: -1 }
        );
        assert_eq!(
            decoded[1].opcode,
            BindOpcode::SetSymbolTrailingFlags {
                flags: 1,
                symbol: b"foo",
            }
        );
        assert_eq!(decoded[2].opcode, BindOpcode::SetAddend { addend: -2 });
        assert_eq!(
            decoded[3].opcode,
            BindOpcode::SetSegmentAndOffset {
                segment: 2,
                offset: 5,
            }
        );
        assert_eq!(decoded[4].opcode, BindOpcode::DoBind);
        assert_eq!(decoded[5].opcode, BindOpcode::Done);
    }

    #[test]
    fn lazy_bind_done_does_not_hide_following_entry() {
        let bytes = [0x11, 0x00, 0x12, 0x00];
        let decoded: Vec<_> = bind_opcodes(&bytes).map(Result::unwrap).collect();
        assert_eq!(decoded.len(), 4);
        assert_eq!(
            decoded[0].opcode,
            BindOpcode::SetDylibOrdinalImmediate { ordinal: 1 }
        );
        assert_eq!(decoded[1].opcode, BindOpcode::Done);
        assert_eq!(
            decoded[2].opcode,
            BindOpcode::SetDylibOrdinalImmediate { ordinal: 2 }
        );
        assert_eq!(decoded[3].opcode, BindOpcode::Done);
    }

    #[test]
    fn rejects_truncated_and_overflowing_uleb() {
        let truncated = [BIND_OPCODE_SET_DYLIB_ORDINAL_ULEB, 0x80];
        assert_eq!(
            bind_opcodes(&truncated).next(),
            Some(Err(LegacyError::UnexpectedEnd { offset: 2 }))
        );

        let mut overflow = vec![BIND_OPCODE_SET_DYLIB_ORDINAL_ULEB];
        overflow.extend_from_slice(&[0xff; 10]);
        assert_eq!(
            bind_opcodes(&overflow).next(),
            Some(Err(LegacyError::UlebOverflow { offset: 1 }))
        );
    }

    #[test]
    fn rejects_unterminated_or_empty_symbol() {
        let unterminated = [0x40, b'f', b'o', b'o'];
        assert_eq!(
            bind_opcodes(&unterminated).next(),
            Some(Err(LegacyError::UnterminatedSymbol { offset: 0 }))
        );

        let empty = [0x40, 0];
        assert_eq!(
            bind_opcodes(&empty).next(),
            Some(Err(LegacyError::EmptySymbol { offset: 0 }))
        );
    }

    #[test]
    fn unsupported_threaded_bind_fails_closed_and_seals_iterator() {
        let bytes = [0xd0, 0x90];
        let mut iter = bind_opcodes(&bytes);
        assert_eq!(
            iter.next(),
            Some(Err(LegacyError::UnknownBindOpcode {
                offset: 0,
                opcode: 0xd0,
            }))
        );
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn decodes_all_compound_count_and_skip_operands() {
        let rebase = [0x80, 0x03, 0x20, 0x00];
        let first = rebase_opcodes(&rebase).next().unwrap().unwrap();
        assert_eq!(
            first.opcode,
            RebaseOpcode::DoRebaseUlebTimesSkipping { count: 3, skip: 32 }
        );

        let bind = [0xc0, 0x04, 0x18];
        let first = bind_opcodes(&bind).next().unwrap().unwrap();
        assert_eq!(
            first.opcode,
            BindOpcode::DoBindUlebTimesSkipping { count: 4, skip: 24 }
        );
    }
}
