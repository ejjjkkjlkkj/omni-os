//! On-disk boot-state record (recovery rule 5: tiny, redundant, checksummed boot-writable state).
//!
//! Two fixed-size copies are kept (A and B). The loader always overwrites the copy that does
//! not hold the newest record, so a torn write can never destroy the last good state.

use crate::{
    BootSelectionState, BootStateError, BootStateRecord, GenerationLocator, select_newest_record,
};
use aw_generation::ObjectId;

/// Size of one on-disk boot-state record.
pub const RECORD_BYTES: usize = 128;
const RECORD_MAGIC: [u8; 8] = *b"OMNIBST\x01";
const RECORD_VERSION: u16 = 1;
const CHECKSUM_OFFSET: usize = RECORD_BYTES - 4;

/// Why an on-disk record was rejected. Every rejection is final: a record is either exactly
/// valid or unusable, never "repaired" by guessing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordDecodeError {
    BadLength,
    BadMagic,
    BadVersion,
    BadChecksum,
    NonZeroReserved,
    BadStateTag,
    BadLocator,
    Invalid(BootStateError),
}

/// IEEE CRC-32 (reflected, polynomial 0xEDB88320). It detects every single-bit error and every
/// burst up to 32 bits in a record: an integrity check against torn or corrupted writes, not an
/// authentication (signed records are a later step).
#[must_use]
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn put_locator(out: &mut [u8; RECORD_BYTES], at: usize, locator: GenerationLocator) {
    out[at..at + 8].copy_from_slice(&locator.generation().to_le_bytes());
    out[at + 8..at + 40].copy_from_slice(&locator.manifest().bytes());
}

fn get_u64(data: &[u8], at: usize) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&data[at..at + 8]);
    u64::from_le_bytes(bytes)
}

fn get_locator(data: &[u8], at: usize) -> Result<GenerationLocator, RecordDecodeError> {
    let mut manifest = [0u8; 32];
    manifest.copy_from_slice(&data[at + 8..at + 40]);
    let manifest = ObjectId::new(manifest).ok_or(RecordDecodeError::BadLocator)?;
    GenerationLocator::new(get_u64(data, at), manifest).ok_or(RecordDecodeError::BadLocator)
}

impl BootStateRecord {
    /// Encodes the record in its fixed little-endian on-disk layout:
    ///
    /// | bytes | field |
    /// |---|---|
    /// | 0..8 | magic `OMNIBST\x01` |
    /// | 8..10 | version (1) |
    /// | 10..12 | reserved, zero |
    /// | 12..20 | sequence |
    /// | 20..60 | selected generation (u64) + manifest digest (32 bytes) |
    /// | 60..100 | previous successful generation + manifest digest |
    /// | 100..108 | rollback floor |
    /// | 108 | state: 0 trial, 1 trial attempt, 2 successful |
    /// | 109 | tries remaining (0 when successful) |
    /// | 110..124 | reserved, zero |
    /// | 124..128 | CRC-32 of bytes 0..124 |
    #[must_use]
    pub fn encode(self) -> [u8; RECORD_BYTES] {
        let mut out = [0u8; RECORD_BYTES];
        out[0..8].copy_from_slice(&RECORD_MAGIC);
        out[8..10].copy_from_slice(&RECORD_VERSION.to_le_bytes());
        out[12..20].copy_from_slice(&self.sequence().to_le_bytes());
        put_locator(&mut out, 20, self.selected());
        put_locator(&mut out, 60, self.previous_successful());
        out[100..108].copy_from_slice(&self.rollback_floor().to_le_bytes());
        let (tag, tries) = match self.state() {
            BootSelectionState::Trial { tries_remaining } => (0, tries_remaining),
            BootSelectionState::TrialAttempt { tries_remaining } => (1, tries_remaining),
            BootSelectionState::Successful => (2, 0),
        };
        out[108] = tag;
        out[109] = tries;
        let crc = crc32(&out[..CHECKSUM_OFFSET]);
        out[CHECKSUM_OFFSET..].copy_from_slice(&crc.to_le_bytes());
        out
    }

    /// Decodes and fully validates one on-disk record. Any deviation - length, magic, checksum,
    /// version, reserved bytes, state tag, locators or record invariants - rejects it.
    pub fn decode(data: &[u8]) -> Result<Self, RecordDecodeError> {
        if data.len() != RECORD_BYTES {
            return Err(RecordDecodeError::BadLength);
        }
        if data[0..8] != RECORD_MAGIC {
            return Err(RecordDecodeError::BadMagic);
        }
        let mut stored = [0u8; 4];
        stored.copy_from_slice(&data[CHECKSUM_OFFSET..]);
        if u32::from_le_bytes(stored) != crc32(&data[..CHECKSUM_OFFSET]) {
            return Err(RecordDecodeError::BadChecksum);
        }
        if u16::from_le_bytes([data[8], data[9]]) != RECORD_VERSION {
            return Err(RecordDecodeError::BadVersion);
        }
        if data[10..12]
            .iter()
            .chain(&data[110..CHECKSUM_OFFSET])
            .any(|b| *b != 0)
        {
            return Err(RecordDecodeError::NonZeroReserved);
        }
        let tries = data[109];
        let state = match data[108] {
            0 => BootSelectionState::Trial {
                tries_remaining: tries,
            },
            1 => BootSelectionState::TrialAttempt {
                tries_remaining: tries,
            },
            2 if tries == 0 => BootSelectionState::Successful,
            _ => return Err(RecordDecodeError::BadStateTag),
        };
        Self::new(
            get_u64(data, 12),
            get_locator(data, 20)?,
            get_locator(data, 60)?,
            get_u64(data, 100),
            state,
        )
        .map_err(RecordDecodeError::Invalid)
    }
}

/// Decodes both on-disk copies and selects the newest valid one. A copy that fails any check is
/// ignored; two valid copies with the same sequence but different contents fail closed.
pub fn select_from_disk(
    copy_a: Option<&[u8]>,
    copy_b: Option<&[u8]>,
) -> Result<BootStateRecord, BootStateError> {
    let a = copy_a.and_then(|bytes| BootStateRecord::decode(bytes).ok());
    let b = copy_b.and_then(|bytes| BootStateRecord::decode(bytes).ok());
    select_newest_record(a, b)
}

/// Which copy (A = `false`, B = `true`) the next write must go to: never the one that holds the
/// newest valid record. With no valid copy, A is written first.
#[must_use]
pub fn next_write_slot(copy_a: Option<&[u8]>, copy_b: Option<&[u8]>) -> bool {
    let a = copy_a.and_then(|bytes| BootStateRecord::decode(bytes).ok());
    let b = copy_b.and_then(|bytes| BootStateRecord::decode(bytes).ok());
    match (a, b) {
        (Some(a), Some(b)) => a.sequence() > b.sequence(),
        (Some(_), None) => true,
        (None, _) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MAX_TRIAL_BOOT_ATTEMPTS;

    fn locator(generation: u64, seed: u8) -> GenerationLocator {
        GenerationLocator::new(generation, ObjectId::new([seed; 32]).unwrap()).unwrap()
    }

    fn records() -> [BootStateRecord; 3] {
        let make = |sequence, state| {
            BootStateRecord::new(sequence, locator(7, 0xA7), locator(6, 0x56), 3, state).unwrap()
        };
        [
            make(10, BootSelectionState::Trial { tries_remaining: 3 }),
            make(11, BootSelectionState::TrialAttempt { tries_remaining: 2 }),
            make(12, BootSelectionState::Successful),
        ]
    }

    fn reseal(mut bytes: [u8; RECORD_BYTES]) -> [u8; RECORD_BYTES] {
        let crc = crc32(&bytes[..CHECKSUM_OFFSET]);
        bytes[CHECKSUM_OFFSET..].copy_from_slice(&crc.to_le_bytes());
        bytes
    }

    #[test]
    fn crc32_matches_the_ieee_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn every_state_round_trips() {
        for record in records() {
            assert_eq!(BootStateRecord::decode(&record.encode()), Ok(record));
        }
    }

    #[test]
    fn every_single_bit_flip_is_rejected() {
        for record in records() {
            let good = record.encode();
            for bit in 0..RECORD_BYTES * 8 {
                let mut bad = good;
                bad[bit / 8] ^= 1 << (bit % 8);
                assert!(BootStateRecord::decode(&bad).is_err(), "bit {bit} accepted");
            }
        }
    }

    #[test]
    fn wrong_lengths_are_rejected() {
        let good = records()[0].encode();
        assert_eq!(
            BootStateRecord::decode(&good[..RECORD_BYTES - 1]),
            Err(RecordDecodeError::BadLength)
        );
        assert_eq!(
            BootStateRecord::decode(&[]),
            Err(RecordDecodeError::BadLength)
        );
    }

    #[test]
    fn a_valid_checksum_does_not_excuse_invalid_content() {
        let good = records()[2].encode();
        let mut reserved = good;
        reserved[115] = 1;
        assert_eq!(
            BootStateRecord::decode(&reseal(reserved)),
            Err(RecordDecodeError::NonZeroReserved)
        );
        let mut tag = good;
        tag[108] = 9;
        assert_eq!(
            BootStateRecord::decode(&reseal(tag)),
            Err(RecordDecodeError::BadStateTag)
        );
        let mut zero_generation = good;
        zero_generation[20..28].fill(0);
        assert_eq!(
            BootStateRecord::decode(&reseal(zero_generation)),
            Err(RecordDecodeError::BadLocator)
        );
        let mut too_many = records()[0].encode();
        too_many[109] = MAX_TRIAL_BOOT_ATTEMPTS + 1;
        assert_eq!(
            BootStateRecord::decode(&reseal(too_many)),
            Err(RecordDecodeError::Invalid(
                BootStateError::InvalidTrialAttempts
            ))
        );
    }

    #[test]
    fn arbitrary_bytes_never_panic_and_are_never_accepted() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..20_000 {
            let mut bytes = [0u8; RECORD_BYTES];
            for byte in &mut bytes {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *byte = state.to_le_bytes()[0];
            }
            assert!(BootStateRecord::decode(&bytes).is_err());
        }
    }

    #[test]
    fn the_newest_valid_copy_wins_and_a_corrupt_copy_is_ignored() {
        let [older, newer, _] = records();
        let (a, b) = (older.encode(), newer.encode());
        assert_eq!(select_from_disk(Some(&a), Some(&b)), Ok(newer));
        let mut torn = b;
        torn[40] ^= 0xFF;
        assert_eq!(select_from_disk(Some(&a), Some(&torn)), Ok(older));
        assert_eq!(
            select_from_disk(None, None),
            Err(BootStateError::NoUsableRecord)
        );
    }

    #[test]
    fn equal_sequences_with_different_content_fail_closed() {
        let one = records()[0];
        let other = BootStateRecord::new(
            one.sequence(),
            locator(8, 0xB8),
            locator(6, 0x56),
            3,
            BootSelectionState::Successful,
        )
        .unwrap();
        assert_eq!(
            select_from_disk(Some(&one.encode()), Some(&other.encode())),
            Err(BootStateError::ConflictingSequence)
        );
    }

    #[test]
    fn writes_never_target_the_copy_holding_the_newest_record() {
        let [older, newer, _] = records();
        let (a, b) = (older.encode(), newer.encode());
        assert!(!next_write_slot(Some(&a), Some(&b)), "B is newest: write A");
        assert!(next_write_slot(Some(&b), Some(&a)), "A is newest: write B");
        assert!(next_write_slot(Some(&a), None), "only A valid: write B");
        let mut torn = a;
        torn[0] ^= 1;
        assert!(
            !next_write_slot(Some(&torn), Some(&b)),
            "A corrupt: write A"
        );
        assert!(!next_write_slot(None, None), "nothing yet: write A");
    }
}
