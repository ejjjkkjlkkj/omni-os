#![no_std]
#![forbid(unsafe_code)]

pub mod wire;

pub const AWFS_BLOCK_SIZE: u32 = 4096;
pub const AWFS_DIGEST_BYTES: usize = 32;
pub const AWFS_VOLUME_ID_BYTES: usize = 16;
pub const AWFS_FORMAT_VERSION: u32 = 1;
pub const AWFS_CHECKPOINT_COPIES: u64 = 4;
pub const AWFS_CANONICAL_HEADER_BYTES: u32 = 128;
pub const AWFS_MAX_PAYLOAD_BYTES: u32 = AWFS_BLOCK_SIZE - AWFS_CANONICAL_HEADER_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Digest([u8; AWFS_DIGEST_BYTES]);

impl Digest {
    #[must_use]
    pub fn new(bytes: [u8; AWFS_DIGEST_BYTES]) -> Option<Self> {
        bytes.iter().any(|byte| *byte != 0).then_some(Self(bytes))
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; AWFS_DIGEST_BYTES] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VolumeId([u8; AWFS_VOLUME_ID_BYTES]);

impl VolumeId {
    #[must_use]
    pub fn new(bytes: [u8; AWFS_VOLUME_ID_BYTES]) -> Option<Self> {
        bytes.iter().any(|byte| *byte != 0).then_some(Self(bytes))
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; AWFS_VOLUME_ID_BYTES] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VolumeMode {
    ImmutableVerifiedStore,
    EncryptedMutableState,
}

impl VolumeMode {
    #[must_use]
    pub const fn is_mutable(self) -> bool {
        matches!(self, Self::EncryptedMutableState)
    }

    #[must_use]
    pub const fn requires_content_authentication(self) -> bool {
        true
    }

    #[must_use]
    pub const fn requires_encryption(self) -> bool {
        matches!(self, Self::EncryptedMutableState)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootPointer {
    physical_block: u64,
    digest: Digest,
}

impl RootPointer {
    #[must_use]
    pub const fn new(physical_block: u64, digest: Digest) -> Option<Self> {
        if physical_block < AWFS_CHECKPOINT_COPIES {
            return None;
        }
        Some(Self {
            physical_block,
            digest,
        })
    }

    #[must_use]
    pub const fn physical_block(self) -> u64 {
        self.physical_block
    }

    #[must_use]
    pub const fn digest(self) -> Digest {
        self.digest
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckpointRecord {
    format_version: u32,
    sequence: u64,
    volume_id: VolumeId,
    mode: VolumeMode,
    root: RootPointer,
    previous_root: Option<RootPointer>,
}

impl CheckpointRecord {
    #[must_use]
    pub fn new(
        sequence: u64,
        volume_id: VolumeId,
        mode: VolumeMode,
        root: RootPointer,
        previous_root: Option<RootPointer>,
    ) -> Option<Self> {
        if sequence == 0 {
            return None;
        }
        if let Some(previous) = previous_root
            && previous == root
        {
            return None;
        }
        Some(Self {
            format_version: AWFS_FORMAT_VERSION,
            sequence,
            volume_id,
            mode,
            root,
            previous_root,
        })
    }

    #[must_use]
    pub const fn format_version(self) -> u32 {
        self.format_version
    }

    #[must_use]
    pub const fn sequence(self) -> u64 {
        self.sequence
    }

    #[must_use]
    pub const fn volume_id(self) -> VolumeId {
        self.volume_id
    }

    #[must_use]
    pub const fn mode(self) -> VolumeMode {
        self.mode
    }

    #[must_use]
    pub const fn root(self) -> RootPointer {
        self.root
    }

    #[must_use]
    pub const fn previous_root(self) -> Option<RootPointer> {
        self.previous_root
    }

    #[must_use]
    pub const fn is_supported(self) -> bool {
        self.format_version == AWFS_FORMAT_VERSION && self.sequence != 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckpointSelectionError {
    NoValidCheckpoint,
    ConflictingSequence { sequence: u64 },
    VolumeMismatch,
    ModeMismatch,
}

/// Selects the newest valid checkpoint without guessing through ambiguous state.
///
/// Equal sequence numbers are accepted only when the records are identical. If two records claim
/// the same newest sequence but point at different roots, the filesystem must enter recovery.
pub fn select_checkpoint<const COPIES: usize>(
    records: &[Option<CheckpointRecord>; COPIES],
) -> Result<CheckpointRecord, CheckpointSelectionError> {
    let mut selected: Option<CheckpointRecord> = None;

    for record in records.iter().flatten().copied() {
        if !record.is_supported() {
            continue;
        }

        if let Some(current) = selected {
            if record.volume_id() != current.volume_id() {
                return Err(CheckpointSelectionError::VolumeMismatch);
            }
            if record.mode() != current.mode() {
                return Err(CheckpointSelectionError::ModeMismatch);
            }

            if record.sequence() > current.sequence() {
                selected = Some(record);
            } else if record.sequence() == current.sequence() && record != current {
                return Err(CheckpointSelectionError::ConflictingSequence {
                    sequence: record.sequence(),
                });
            }
        } else {
            selected = Some(record);
        }
    }

    selected.ok_or(CheckpointSelectionError::NoValidCheckpoint)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetadataBlockKind {
    Root,
    ObjectIndex,
    Directory,
    Inode,
    ExtentMap,
    AllocationMap,
    SnapshotIndex,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetadataValidationError {
    ZeroTransaction,
    ZeroOwner,
    PayloadTooLarge,
    WrongPhysicalLocation { expected: u64, actual: u64 },
}

/// Semantic header shared by AWFS metadata blocks.
///
/// This is deliberately not an on-disk `repr(C)` structure. Canonical byte encoding will be a
/// separate parser/serializer so malformed disk bytes can never be interpreted by transmuting a
/// Rust structure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MetadataBlockHeader {
    kind: MetadataBlockKind,
    owner: u64,
    logical_id: u64,
    transaction: u64,
    expected_physical_block: u64,
    payload_len: u32,
}

impl MetadataBlockHeader {
    pub fn new(
        kind: MetadataBlockKind,
        owner: u64,
        logical_id: u64,
        transaction: u64,
        expected_physical_block: u64,
        payload_len: u32,
    ) -> Result<Self, MetadataValidationError> {
        if transaction == 0 {
            return Err(MetadataValidationError::ZeroTransaction);
        }
        if owner == 0 {
            return Err(MetadataValidationError::ZeroOwner);
        }
        if payload_len > AWFS_MAX_PAYLOAD_BYTES {
            return Err(MetadataValidationError::PayloadTooLarge);
        }

        Ok(Self {
            kind,
            owner,
            logical_id,
            transaction,
            expected_physical_block,
            payload_len,
        })
    }

    #[must_use]
    pub const fn kind(self) -> MetadataBlockKind {
        self.kind
    }

    #[must_use]
    pub const fn owner(self) -> u64 {
        self.owner
    }

    #[must_use]
    pub const fn logical_id(self) -> u64 {
        self.logical_id
    }

    #[must_use]
    pub const fn transaction(self) -> u64 {
        self.transaction
    }

    #[must_use]
    pub const fn expected_physical_block(self) -> u64 {
        self.expected_physical_block
    }

    #[must_use]
    pub const fn payload_len(self) -> u32 {
        self.payload_len
    }

    pub const fn validate_at(
        self,
        actual_physical_block: u64,
    ) -> Result<(), MetadataValidationError> {
        if actual_physical_block != self.expected_physical_block {
            return Err(MetadataValidationError::WrongPhysicalLocation {
                expected: self.expected_physical_block,
                actual: actual_physical_block,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(seed: u8) -> Digest {
        Digest::new([seed; AWFS_DIGEST_BYTES]).unwrap()
    }

    fn volume(seed: u8) -> VolumeId {
        VolumeId::new([seed; AWFS_VOLUME_ID_BYTES]).unwrap()
    }

    fn root(block: u64, seed: u8) -> RootPointer {
        RootPointer::new(block, digest(seed)).unwrap()
    }

    fn checkpoint(sequence: u64, block: u64, seed: u8) -> CheckpointRecord {
        CheckpointRecord::new(
            sequence,
            volume(1),
            VolumeMode::EncryptedMutableState,
            root(block, seed),
            None,
        )
        .unwrap()
    }

    #[test]
    fn sentinels_and_reserved_checkpoint_blocks_are_rejected() {
        assert_eq!(Digest::new([0; AWFS_DIGEST_BYTES]), None);
        assert_eq!(VolumeId::new([0; AWFS_VOLUME_ID_BYTES]), None);
        assert_eq!(RootPointer::new(0, digest(1)), None);
        assert_eq!(RootPointer::new(3, digest(1)), None);
        assert!(RootPointer::new(4, digest(1)).is_some());
        assert_eq!(
            CheckpointRecord::new(
                0,
                volume(1),
                VolumeMode::EncryptedMutableState,
                root(4, 1),
                None,
            ),
            None
        );
    }

    #[test]
    fn newest_checkpoint_wins_when_identity_and_mode_match() {
        let records = [
            Some(checkpoint(7, 10, 1)),
            Some(checkpoint(9, 12, 2)),
            Some(checkpoint(8, 11, 3)),
            None,
        ];
        let selected = select_checkpoint(&records).unwrap();
        assert_eq!(selected.sequence(), 9);
        assert_eq!(selected.root().physical_block(), 12);
    }

    #[test]
    fn identical_redundant_checkpoint_is_accepted() {
        let record = checkpoint(9, 12, 2);
        let records = [Some(record), Some(record), None, None];
        assert_eq!(select_checkpoint(&records), Ok(record));
    }

    #[test]
    fn conflicting_equal_sequence_fails_closed() {
        let records = [
            Some(checkpoint(9, 12, 2)),
            Some(checkpoint(9, 13, 3)),
            None,
            None,
        ];
        assert_eq!(
            select_checkpoint(&records),
            Err(CheckpointSelectionError::ConflictingSequence { sequence: 9 })
        );
    }

    #[test]
    fn mixed_volume_or_mode_never_gets_silently_merged() {
        let first = checkpoint(7, 10, 1);
        let other_volume = CheckpointRecord::new(
            8,
            volume(2),
            VolumeMode::EncryptedMutableState,
            root(11, 2),
            None,
        )
        .unwrap();
        assert_eq!(
            select_checkpoint(&[Some(first), Some(other_volume)]),
            Err(CheckpointSelectionError::VolumeMismatch)
        );

        let other_mode = CheckpointRecord::new(
            8,
            volume(1),
            VolumeMode::ImmutableVerifiedStore,
            root(11, 2),
            None,
        )
        .unwrap();
        assert_eq!(
            select_checkpoint(&[Some(first), Some(other_mode)]),
            Err(CheckpointSelectionError::ModeMismatch)
        );
    }

    #[test]
    fn metadata_header_is_bounded_and_location_checked() {
        assert_eq!(
            MetadataBlockHeader::new(MetadataBlockKind::Root, 1, 1, 0, 20, 64),
            Err(MetadataValidationError::ZeroTransaction)
        );
        assert_eq!(
            MetadataBlockHeader::new(MetadataBlockKind::Root, 0, 1, 1, 20, 64),
            Err(MetadataValidationError::ZeroOwner)
        );
        assert_eq!(
            MetadataBlockHeader::new(
                MetadataBlockKind::Root,
                1,
                1,
                1,
                20,
                AWFS_MAX_PAYLOAD_BYTES + 1,
            ),
            Err(MetadataValidationError::PayloadTooLarge)
        );

        let header = MetadataBlockHeader::new(
            MetadataBlockKind::Directory,
            7,
            42,
            9,
            123,
            AWFS_MAX_PAYLOAD_BYTES,
        )
        .unwrap();
        assert_eq!(header.validate_at(123), Ok(()));
        assert_eq!(
            header.validate_at(124),
            Err(MetadataValidationError::WrongPhysicalLocation {
                expected: 123,
                actual: 124,
            })
        );
    }

    #[test]
    fn volume_modes_encode_different_trust_policies() {
        assert!(!VolumeMode::ImmutableVerifiedStore.is_mutable());
        assert!(VolumeMode::ImmutableVerifiedStore.requires_content_authentication());
        assert!(!VolumeMode::ImmutableVerifiedStore.requires_encryption());

        assert!(VolumeMode::EncryptedMutableState.is_mutable());
        assert!(VolumeMode::EncryptedMutableState.requires_content_authentication());
        assert!(VolumeMode::EncryptedMutableState.requires_encryption());
    }
}
