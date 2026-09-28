#![no_std]
#![forbid(unsafe_code)]

pub mod merkle;
pub mod trust;
pub mod wire;

use aw_fs_core::{AWFS_BLOCK_SIZE, AWFS_CHECKPOINT_COPIES, Digest};

pub const AWSTOREFS_BLOCK_BYTES: u64 = AWFS_BLOCK_SIZE as u64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectClass {
    Kernel,
    SystemComponent,
    RecoveryComponent,
    ApplicationPackage,
    Resource,
}

impl ObjectClass {
    #[must_use]
    pub const fn contains_executable_code(self) -> bool {
        matches!(
            self,
            Self::Kernel
                | Self::SystemComponent
                | Self::RecoveryComponent
                | Self::ApplicationPackage
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtentError {
    ReservedBlock,
    ZeroBlocks,
    ZeroLogicalBytes,
    BlockRangeOverflow,
    CapacityOverflow,
    NonCanonicalBlockCount,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectExtent {
    first_block: u64,
    block_count: u64,
    logical_bytes: u64,
}

impl ObjectExtent {
    pub fn new(
        first_block: u64,
        block_count: u64,
        logical_bytes: u64,
    ) -> Result<Self, ExtentError> {
        if first_block < AWFS_CHECKPOINT_COPIES {
            return Err(ExtentError::ReservedBlock);
        }
        if block_count == 0 {
            return Err(ExtentError::ZeroBlocks);
        }
        if logical_bytes == 0 {
            return Err(ExtentError::ZeroLogicalBytes);
        }
        first_block
            .checked_add(block_count)
            .ok_or(ExtentError::BlockRangeOverflow)?;
        let capacity = block_count
            .checked_mul(AWSTOREFS_BLOCK_BYTES)
            .ok_or(ExtentError::CapacityOverflow)?;
        if logical_bytes > capacity {
            return Err(ExtentError::NonCanonicalBlockCount);
        }
        let required_blocks = logical_bytes
            .checked_add(AWSTOREFS_BLOCK_BYTES - 1)
            .ok_or(ExtentError::CapacityOverflow)?
            / AWSTOREFS_BLOCK_BYTES;
        if required_blocks != block_count {
            return Err(ExtentError::NonCanonicalBlockCount);
        }

        Ok(Self {
            first_block,
            block_count,
            logical_bytes,
        })
    }

    #[must_use]
    pub const fn first_block(self) -> u64 {
        self.first_block
    }

    #[must_use]
    pub const fn block_count(self) -> u64 {
        self.block_count
    }

    #[must_use]
    pub const fn logical_bytes(self) -> u64 {
        self.logical_bytes
    }

    #[must_use]
    pub fn end_block_exclusive(self) -> u64 {
        self.first_block + self.block_count
    }

    #[must_use]
    pub fn overlaps(self, other: Self) -> bool {
        self.first_block < other.end_block_exclusive()
            && other.first_block < self.end_block_exclusive()
    }

    #[must_use]
    pub fn read_window(self, object_block_index: u64) -> Option<ReadWindow> {
        if object_block_index >= self.block_count {
            return None;
        }
        let physical_block = self.first_block.checked_add(object_block_index)?;
        let consumed = object_block_index.checked_mul(AWSTOREFS_BLOCK_BYTES)?;
        let remaining = self.logical_bytes.checked_sub(consumed)?;
        let valid_bytes = remaining.min(AWSTOREFS_BLOCK_BYTES) as u32;
        Some(ReadWindow {
            physical_block,
            valid_bytes,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadWindow {
    physical_block: u64,
    valid_bytes: u32,
}

impl ReadWindow {
    #[must_use]
    pub const fn physical_block(self) -> u64 {
        self.physical_block
    }

    #[must_use]
    pub const fn valid_bytes(self) -> u32 {
        self.valid_bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectDescriptor {
    digest: Digest,
    class: ObjectClass,
    extent: ObjectExtent,
}

impl ObjectDescriptor {
    #[must_use]
    pub const fn new(digest: Digest, class: ObjectClass, extent: ObjectExtent) -> Self {
        Self {
            digest,
            class,
            extent,
        }
    }

    #[must_use]
    pub const fn digest(self) -> Digest {
        self.digest
    }

    #[must_use]
    pub const fn class(self) -> ObjectClass {
        self.class
    }

    #[must_use]
    pub const fn extent(self) -> ObjectExtent {
        self.extent
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManifestPlanError {
    ZeroGeneration,
    VolumeTooSmall,
    Capacity,
    DuplicateDigest,
    OverlappingExtent,
    ObjectOutsideVolume,
}

/// Semantic, read-only AWStoreFS manifest plan.
///
/// This type validates layout invariants only. It does not prove that a manifest was signed, that
/// its generation is fresh, or that object bytes match their digests. Those are separate trust
/// boundaries and must be satisfied before any executable object is authorized for execution.
pub struct StoreManifest<const OBJECTS: usize> {
    generation: u64,
    volume_blocks: u64,
    objects: [Option<ObjectDescriptor>; OBJECTS],
    len: usize,
}

impl<const OBJECTS: usize> StoreManifest<OBJECTS> {
    pub fn new(generation: u64, volume_blocks: u64) -> Result<Self, ManifestPlanError> {
        if generation == 0 {
            return Err(ManifestPlanError::ZeroGeneration);
        }
        if volume_blocks <= AWFS_CHECKPOINT_COPIES {
            return Err(ManifestPlanError::VolumeTooSmall);
        }
        Ok(Self {
            generation,
            volume_blocks,
            objects: [None; OBJECTS],
            len: 0,
        })
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn volume_blocks(&self) -> u64 {
        self.volume_blocks
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[must_use]
    pub fn object(&self, index: usize) -> Option<ObjectDescriptor> {
        if index >= self.len {
            return None;
        }
        self.objects[index]
    }

    pub fn push(&mut self, object: ObjectDescriptor) -> Result<(), ManifestPlanError> {
        if object.extent().end_block_exclusive() > self.volume_blocks {
            return Err(ManifestPlanError::ObjectOutsideVolume);
        }
        for existing in self.objects[..self.len].iter().flatten().copied() {
            if existing.digest() == object.digest() {
                return Err(ManifestPlanError::DuplicateDigest);
            }
            if existing.extent().overlaps(object.extent()) {
                return Err(ManifestPlanError::OverlappingExtent);
            }
        }
        if self.len >= OBJECTS {
            return Err(ManifestPlanError::Capacity);
        }
        self.objects[self.len] = Some(object);
        self.len += 1;
        Ok(())
    }

    #[must_use]
    pub fn find_by_digest(&self, digest: Digest) -> Option<ObjectDescriptor> {
        self.objects[..self.len]
            .iter()
            .flatten()
            .copied()
            .find(|object| object.digest() == digest)
    }
}

/// Trust boundary implemented by a cryptographic layer outside AWStoreFS.
///
/// Implementations must compare `bytes` against `expected` using the digest algorithm fixed by an
/// authenticated manifest policy. A permissive implementation is not a security boundary.
pub trait DigestVerifier {
    fn verify(&self, expected: Digest, bytes: &[u8]) -> bool;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectVerificationError {
    ObjectTooLargeForAddressSpace,
    LengthMismatch { expected: u64, actual: usize },
    DigestMismatch,
}

/// Proof that resident object bytes matched the descriptor digest through a caller-provided
/// cryptographic verifier.
///
/// This proof is intentionally insufficient to authorize execution. Signature provenance,
/// generation anti-rollback and execution policy remain separate mandatory gates.
pub struct VerifiedObject<'a> {
    descriptor: ObjectDescriptor,
    bytes: &'a [u8],
}

impl VerifiedObject<'_> {
    #[must_use]
    pub const fn descriptor(&self) -> ObjectDescriptor {
        self.descriptor
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.bytes
    }
}

pub fn verify_resident_object<'a, V: DigestVerifier>(
    descriptor: ObjectDescriptor,
    bytes: &'a [u8],
    verifier: &V,
) -> Result<VerifiedObject<'a>, ObjectVerificationError> {
    let expected = usize::try_from(descriptor.extent().logical_bytes())
        .map_err(|_| ObjectVerificationError::ObjectTooLargeForAddressSpace)?;
    if bytes.len() != expected {
        return Err(ObjectVerificationError::LengthMismatch {
            expected: descriptor.extent().logical_bytes(),
            actual: bytes.len(),
        });
    }
    if !verifier.verify(descriptor.digest(), bytes) {
        return Err(ObjectVerificationError::DigestMismatch);
    }
    Ok(VerifiedObject { descriptor, bytes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(seed: u8) -> Digest {
        Digest::new([seed; 32]).unwrap()
    }

    fn object(
        seed: u8,
        class: ObjectClass,
        first_block: u64,
        logical_bytes: u64,
    ) -> ObjectDescriptor {
        let blocks = logical_bytes.div_ceil(AWSTOREFS_BLOCK_BYTES);
        ObjectDescriptor::new(
            digest(seed),
            class,
            ObjectExtent::new(first_block, blocks, logical_bytes).unwrap(),
        )
    }

    #[test]
    fn extent_rejects_reserved_zero_overflow_and_slack_blocks() {
        assert_eq!(ObjectExtent::new(0, 1, 1), Err(ExtentError::ReservedBlock));
        assert_eq!(
            ObjectExtent::new(AWFS_CHECKPOINT_COPIES, 0, 1),
            Err(ExtentError::ZeroBlocks)
        );
        assert_eq!(
            ObjectExtent::new(AWFS_CHECKPOINT_COPIES, 1, 0),
            Err(ExtentError::ZeroLogicalBytes)
        );
        assert_eq!(
            ObjectExtent::new(u64::MAX, 2, 1),
            Err(ExtentError::BlockRangeOverflow)
        );
        assert_eq!(
            ObjectExtent::new(AWFS_CHECKPOINT_COPIES, 2, 1),
            Err(ExtentError::NonCanonicalBlockCount)
        );
    }

    #[test]
    fn read_windows_never_expose_bytes_past_logical_eof() {
        let extent = ObjectExtent::new(AWFS_CHECKPOINT_COPIES, 2, 5000).unwrap();
        assert_eq!(
            extent.read_window(0),
            Some(ReadWindow {
                physical_block: AWFS_CHECKPOINT_COPIES,
                valid_bytes: 4096,
            })
        );
        assert_eq!(
            extent.read_window(1),
            Some(ReadWindow {
                physical_block: AWFS_CHECKPOINT_COPIES + 1,
                valid_bytes: 904,
            })
        );
        assert_eq!(extent.read_window(2), None);
    }

    #[test]
    fn manifest_rejects_duplicate_digest_overlap_and_out_of_bounds() {
        let mut manifest = StoreManifest::<4>::new(42, 100).unwrap();
        let first = object(1, ObjectClass::Kernel, 10, 4096);
        manifest.push(first).unwrap();

        let duplicate = object(1, ObjectClass::Resource, 20, 4096);
        assert_eq!(
            manifest.push(duplicate),
            Err(ManifestPlanError::DuplicateDigest)
        );

        let overlapping = object(2, ObjectClass::Resource, 10, 4096);
        assert_eq!(
            manifest.push(overlapping),
            Err(ManifestPlanError::OverlappingExtent)
        );

        let outside = object(3, ObjectClass::Resource, 99, 8192);
        assert_eq!(
            manifest.push(outside),
            Err(ManifestPlanError::ObjectOutsideVolume)
        );
    }

    #[test]
    fn manifest_lookup_is_content_addressed() {
        let mut manifest = StoreManifest::<2>::new(7, 64).unwrap();
        let kernel = object(1, ObjectClass::Kernel, 8, 4096);
        let resource = object(2, ObjectClass::Resource, 9, 20);
        manifest.push(kernel).unwrap();
        manifest.push(resource).unwrap();
        assert_eq!(manifest.find_by_digest(digest(2)), Some(resource));
        assert_eq!(manifest.find_by_digest(digest(9)), None);
    }

    struct ExactVerifier;

    impl DigestVerifier for ExactVerifier {
        fn verify(&self, expected: Digest, bytes: &[u8]) -> bool {
            expected == digest(bytes.first().copied().unwrap_or(0))
        }
    }

    #[test]
    fn resident_verification_requires_exact_length_and_digest_evidence() {
        let descriptor = object(7, ObjectClass::SystemComponent, 12, 4);
        let good = [7_u8; 4];
        let verified = verify_resident_object(descriptor, &good, &ExactVerifier).unwrap();
        assert_eq!(verified.descriptor(), descriptor);
        assert_eq!(verified.bytes(), &good);

        assert_eq!(
            verify_resident_object(descriptor, &[7_u8; 3], &ExactVerifier).err(),
            Some(ObjectVerificationError::LengthMismatch {
                expected: 4,
                actual: 3,
            })
        );
        assert_eq!(
            verify_resident_object(descriptor, &[8_u8; 4], &ExactVerifier).err(),
            Some(ObjectVerificationError::DigestMismatch)
        );
    }

    #[test]
    fn digest_verification_does_not_grant_execution_authority() {
        assert!(ObjectClass::Kernel.contains_executable_code());
        assert!(ObjectClass::ApplicationPackage.contains_executable_code());
        assert!(!ObjectClass::Resource.contains_executable_code());
        // No execution-permit API exists here by design. Authenticated manifest provenance and
        // anti-rollback must be added as independent proof layers before execution is possible.
    }
}
