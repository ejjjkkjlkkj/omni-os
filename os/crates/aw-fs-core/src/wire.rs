use super::{AWFS_FORMAT_VERSION, CheckpointRecord, Digest, RootPointer, VolumeId, VolumeMode};

pub const CHECKPOINT_PAYLOAD_BYTES: usize = 128;
const CHECKPOINT_MAGIC: [u8; 8] = *b"AWFSCP01";
const MODE_IMMUTABLE_VERIFIED_STORE: u8 = 1;
const MODE_ENCRYPTED_MUTABLE_STATE: u8 = 2;
const PREVIOUS_ROOT_ABSENT: u8 = 0;
const PREVIOUS_ROOT_PRESENT: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckpointDecodeError {
    InvalidLength,
    InvalidMagic,
    UnsupportedVersion(u32),
    InvalidMode(u8),
    InvalidPreviousRootFlag(u8),
    NonCanonicalReservedBytes,
    InvalidVolumeId,
    InvalidRoot,
    InvalidPreviousRoot,
    InvalidRecord,
}

/// Canonically serializes the semantic portion of an AWFS checkpoint.
///
/// This payload is deliberately **not authenticated by this function**. It must be wrapped by a
/// cryptographic integrity/authentication layer before it is eligible for checkpoint selection.
/// Keeping canonical encoding separate from authentication lets the on-disk format support
/// algorithm agility without teaching the parser to trust unauthenticated bytes.
#[must_use]
pub fn encode_checkpoint_payload(record: CheckpointRecord) -> [u8; CHECKPOINT_PAYLOAD_BYTES] {
    let mut output = [0_u8; CHECKPOINT_PAYLOAD_BYTES];
    output[0..8].copy_from_slice(&CHECKPOINT_MAGIC);
    output[8..12].copy_from_slice(&record.format_version().to_le_bytes());
    output[12] = match record.mode() {
        VolumeMode::ImmutableVerifiedStore => MODE_IMMUTABLE_VERIFIED_STORE,
        VolumeMode::EncryptedMutableState => MODE_ENCRYPTED_MUTABLE_STATE,
    };
    output[16..24].copy_from_slice(&record.sequence().to_le_bytes());
    output[24..40].copy_from_slice(&record.volume_id().bytes());
    output[40..48].copy_from_slice(&record.root().physical_block().to_le_bytes());
    output[48..80].copy_from_slice(&record.root().digest().bytes());

    if let Some(previous) = record.previous_root() {
        output[13] = PREVIOUS_ROOT_PRESENT;
        output[80..88].copy_from_slice(&previous.physical_block().to_le_bytes());
        output[88..120].copy_from_slice(&previous.digest().bytes());
    }

    output
}

/// Parses only the canonical semantic checkpoint payload.
///
/// Success means the bytes are structurally canonical and satisfy AWFS semantic invariants. It
/// does **not** prove authenticity, freshness, or that referenced blocks hash to the encoded
/// digests. Callers must authenticate the envelope and verify the root graph before exposing a
/// returned record to `select_checkpoint`.
pub fn decode_checkpoint_payload(input: &[u8]) -> Result<CheckpointRecord, CheckpointDecodeError> {
    if input.len() != CHECKPOINT_PAYLOAD_BYTES {
        return Err(CheckpointDecodeError::InvalidLength);
    }
    if input[0..8] != CHECKPOINT_MAGIC {
        return Err(CheckpointDecodeError::InvalidMagic);
    }

    let version = read_u32(input, 8);
    if version != AWFS_FORMAT_VERSION {
        return Err(CheckpointDecodeError::UnsupportedVersion(version));
    }

    let mode = match input[12] {
        MODE_IMMUTABLE_VERIFIED_STORE => VolumeMode::ImmutableVerifiedStore,
        MODE_ENCRYPTED_MUTABLE_STATE => VolumeMode::EncryptedMutableState,
        other => return Err(CheckpointDecodeError::InvalidMode(other)),
    };
    let previous_flag = input[13];
    if !matches!(previous_flag, PREVIOUS_ROOT_ABSENT | PREVIOUS_ROOT_PRESENT) {
        return Err(CheckpointDecodeError::InvalidPreviousRootFlag(
            previous_flag,
        ));
    }
    if input[14..16].iter().any(|byte| *byte != 0) || input[120..128].iter().any(|byte| *byte != 0)
    {
        return Err(CheckpointDecodeError::NonCanonicalReservedBytes);
    }

    let sequence = read_u64(input, 16);
    let mut volume_bytes = [0_u8; 16];
    volume_bytes.copy_from_slice(&input[24..40]);
    let volume_id = VolumeId::new(volume_bytes).ok_or(CheckpointDecodeError::InvalidVolumeId)?;

    let root = decode_root(input, 40, 48).ok_or(CheckpointDecodeError::InvalidRoot)?;

    let previous_root = if previous_flag == PREVIOUS_ROOT_PRESENT {
        Some(decode_root(input, 80, 88).ok_or(CheckpointDecodeError::InvalidPreviousRoot)?)
    } else {
        if input[80..120].iter().any(|byte| *byte != 0) {
            return Err(CheckpointDecodeError::NonCanonicalReservedBytes);
        }
        None
    };

    CheckpointRecord::new(sequence, volume_id, mode, root, previous_root)
        .ok_or(CheckpointDecodeError::InvalidRecord)
}

fn decode_root(input: &[u8], block_offset: usize, digest_offset: usize) -> Option<RootPointer> {
    let physical_block = read_u64(input, block_offset);
    let mut digest_bytes = [0_u8; 32];
    digest_bytes.copy_from_slice(&input[digest_offset..digest_offset + 32]);
    RootPointer::new(physical_block, Digest::new(digest_bytes)?)
}

fn read_u32(input: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
    ])
}

fn read_u64(input: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
        input[offset + 4],
        input[offset + 5],
        input[offset + 6],
        input[offset + 7],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(seed: u8) -> Digest {
        Digest::new([seed; 32]).unwrap()
    }

    fn record(previous: bool) -> CheckpointRecord {
        CheckpointRecord::new(
            9,
            VolumeId::new([7; 16]).unwrap(),
            VolumeMode::EncryptedMutableState,
            RootPointer::new(42, digest(1)).unwrap(),
            previous.then(|| RootPointer::new(41, digest(2)).unwrap()),
        )
        .unwrap()
    }

    #[test]
    fn canonical_payload_round_trips() {
        for original in [record(false), record(true)] {
            let encoded = encode_checkpoint_payload(original);
            assert_eq!(decode_checkpoint_payload(&encoded), Ok(original));
            assert_eq!(encode_checkpoint_payload(original), encoded);
        }
    }

    #[test]
    fn parser_requires_exact_length_magic_and_version() {
        let encoded = encode_checkpoint_payload(record(false));
        assert_eq!(
            decode_checkpoint_payload(&encoded[..127]),
            Err(CheckpointDecodeError::InvalidLength)
        );

        let mut bad_magic = encoded;
        bad_magic[0] ^= 0x80;
        assert_eq!(
            decode_checkpoint_payload(&bad_magic),
            Err(CheckpointDecodeError::InvalidMagic)
        );

        let mut bad_version = encoded;
        bad_version[8..12].copy_from_slice(&2_u32.to_le_bytes());
        assert_eq!(
            decode_checkpoint_payload(&bad_version),
            Err(CheckpointDecodeError::UnsupportedVersion(2))
        );
    }

    #[test]
    fn parser_rejects_noncanonical_reserved_and_hidden_previous_bytes() {
        let encoded = encode_checkpoint_payload(record(false));

        let mut reserved = encoded;
        reserved[14] = 1;
        assert_eq!(
            decode_checkpoint_payload(&reserved),
            Err(CheckpointDecodeError::NonCanonicalReservedBytes)
        );

        let mut hidden_previous = encoded;
        hidden_previous[80] = 1;
        assert_eq!(
            decode_checkpoint_payload(&hidden_previous),
            Err(CheckpointDecodeError::NonCanonicalReservedBytes)
        );
    }

    #[test]
    fn parser_rejects_invalid_modes_flags_and_root_material() {
        let encoded = encode_checkpoint_payload(record(false));

        let mut bad_mode = encoded;
        bad_mode[12] = 99;
        assert_eq!(
            decode_checkpoint_payload(&bad_mode),
            Err(CheckpointDecodeError::InvalidMode(99))
        );

        let mut bad_flag = encoded;
        bad_flag[13] = 2;
        assert_eq!(
            decode_checkpoint_payload(&bad_flag),
            Err(CheckpointDecodeError::InvalidPreviousRootFlag(2))
        );

        let mut zero_digest = encoded;
        zero_digest[48..80].fill(0);
        assert_eq!(
            decode_checkpoint_payload(&zero_digest),
            Err(CheckpointDecodeError::InvalidRoot)
        );

        let mut reserved_root_block = encoded;
        reserved_root_block[40..48].copy_from_slice(&3_u64.to_le_bytes());
        assert_eq!(
            decode_checkpoint_payload(&reserved_root_block),
            Err(CheckpointDecodeError::InvalidRoot)
        );
    }

    #[test]
    fn parser_rejects_previous_root_equal_to_current_root() {
        let current = record(false);
        let mut encoded = encode_checkpoint_payload(current);
        encoded[13] = PREVIOUS_ROOT_PRESENT;
        encoded[80..88].copy_from_slice(&current.root().physical_block().to_le_bytes());
        encoded[88..120].copy_from_slice(&current.root().digest().bytes());
        assert_eq!(
            decode_checkpoint_payload(&encoded),
            Err(CheckpointDecodeError::InvalidRecord)
        );
    }
}
