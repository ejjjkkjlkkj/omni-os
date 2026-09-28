use super::{
    DigestVerifier, ManifestPlanError, ObjectClass, ObjectDescriptor, ObjectExtent, StoreManifest,
};
use aw_fs_core::Digest;

pub const MANIFEST_FORMAT_VERSION: u32 = 1;
pub const MANIFEST_HEADER_BYTES: usize = 64;
pub const OBJECT_RECORD_BYTES: usize = 64;
const MANIFEST_MAGIC: [u8; 8] = *b"AWSMF001";

const CLASS_KERNEL: u8 = 1;
const CLASS_SYSTEM_COMPONENT: u8 = 2;
const CLASS_RECOVERY_COMPONENT: u8 = 3;
const CLASS_APPLICATION_PACKAGE: u8 = 4;
const CLASS_RESOURCE: u8 = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManifestDecodeError {
    HeaderTooShort,
    InvalidMagic,
    UnsupportedVersion(u32),
    InvalidRecordSize(u32),
    NonCanonicalReservedBytes,
    ObjectCountOverflow,
    InvalidLength,
    InvalidClass(u8),
    InvalidDigest,
    InvalidExtent,
    InvalidPlan(ManifestPlanError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManifestEncodeError {
    OutputTooSmall,
    ObjectCountOverflow,
}

#[must_use]
pub fn encoded_manifest_len(object_count: usize) -> Option<usize> {
    object_count
        .checked_mul(OBJECT_RECORD_BYTES)?
        .checked_add(MANIFEST_HEADER_BYTES)
}

/// Canonically encodes a structural AWStoreFS manifest into caller-provided storage.
///
/// The resulting bytes are not signed or authenticated here. They become trustworthy only after a
/// separate signature/provenance layer authenticates the exact canonical bytes and generation
/// policy.
pub fn encode_manifest<const OBJECTS: usize>(
    manifest: &StoreManifest<OBJECTS>,
    output: &mut [u8],
) -> Result<usize, ManifestEncodeError> {
    let object_count =
        u32::try_from(manifest.len()).map_err(|_| ManifestEncodeError::ObjectCountOverflow)?;
    let required =
        encoded_manifest_len(manifest.len()).ok_or(ManifestEncodeError::ObjectCountOverflow)?;
    if output.len() < required {
        return Err(ManifestEncodeError::OutputTooSmall);
    }
    output[..required].fill(0);
    output[0..8].copy_from_slice(&MANIFEST_MAGIC);
    output[8..12].copy_from_slice(&MANIFEST_FORMAT_VERSION.to_le_bytes());
    output[12..16].copy_from_slice(&(OBJECT_RECORD_BYTES as u32).to_le_bytes());
    output[16..24].copy_from_slice(&manifest.generation().to_le_bytes());
    output[24..32].copy_from_slice(&manifest.volume_blocks().to_le_bytes());
    output[32..36].copy_from_slice(&object_count.to_le_bytes());

    for index in 0..manifest.len() {
        let object = manifest.object(index).expect("manifest length invariant");
        let base = MANIFEST_HEADER_BYTES + index * OBJECT_RECORD_BYTES;
        output[base] = encode_class(object.class());
        output[base + 8..base + 16].copy_from_slice(&object.extent().first_block().to_le_bytes());
        output[base + 16..base + 24].copy_from_slice(&object.extent().block_count().to_le_bytes());
        output[base + 24..base + 32]
            .copy_from_slice(&object.extent().logical_bytes().to_le_bytes());
        output[base + 32..base + 64].copy_from_slice(&object.digest().bytes());
    }
    Ok(required)
}

/// Parses a canonical structural manifest into a fixed-capacity semantic plan.
///
/// This parser deliberately does not authenticate the manifest. A caller must verify a signature
/// over the exact canonical bytes and enforce generation/rollback policy before using any object
/// as executable content.
pub fn decode_manifest<const OBJECTS: usize>(
    input: &[u8],
) -> Result<StoreManifest<OBJECTS>, ManifestDecodeError> {
    if input.len() < MANIFEST_HEADER_BYTES {
        return Err(ManifestDecodeError::HeaderTooShort);
    }
    if input[0..8] != MANIFEST_MAGIC {
        return Err(ManifestDecodeError::InvalidMagic);
    }
    let version = read_u32(input, 8);
    if version != MANIFEST_FORMAT_VERSION {
        return Err(ManifestDecodeError::UnsupportedVersion(version));
    }
    let record_size = read_u32(input, 12);
    if record_size != OBJECT_RECORD_BYTES as u32 {
        return Err(ManifestDecodeError::InvalidRecordSize(record_size));
    }
    if input[36..64].iter().any(|byte| *byte != 0) {
        return Err(ManifestDecodeError::NonCanonicalReservedBytes);
    }

    let generation = read_u64(input, 16);
    let volume_blocks = read_u64(input, 24);
    let object_count = usize::try_from(read_u32(input, 32))
        .map_err(|_| ManifestDecodeError::ObjectCountOverflow)?;
    if object_count > OBJECTS {
        return Err(ManifestDecodeError::ObjectCountOverflow);
    }
    let expected_len =
        encoded_manifest_len(object_count).ok_or(ManifestDecodeError::ObjectCountOverflow)?;
    if input.len() != expected_len {
        return Err(ManifestDecodeError::InvalidLength);
    }

    let mut manifest =
        StoreManifest::new(generation, volume_blocks).map_err(ManifestDecodeError::InvalidPlan)?;
    for index in 0..object_count {
        let base = MANIFEST_HEADER_BYTES + index * OBJECT_RECORD_BYTES;
        let class = decode_class(input[base])?;
        if input[base + 1..base + 8].iter().any(|byte| *byte != 0) {
            return Err(ManifestDecodeError::NonCanonicalReservedBytes);
        }
        let first_block = read_u64(input, base + 8);
        let block_count = read_u64(input, base + 16);
        let logical_bytes = read_u64(input, base + 24);
        let extent = ObjectExtent::new(first_block, block_count, logical_bytes)
            .map_err(|_| ManifestDecodeError::InvalidExtent)?;
        let mut digest_bytes = [0_u8; 32];
        digest_bytes.copy_from_slice(&input[base + 32..base + 64]);
        let digest = Digest::new(digest_bytes).ok_or(ManifestDecodeError::InvalidDigest)?;
        manifest
            .push(ObjectDescriptor::new(digest, class, extent))
            .map_err(ManifestDecodeError::InvalidPlan)?;
    }
    Ok(manifest)
}

/// Verifies exact canonical manifest bytes using a caller-supplied cryptographic digest boundary.
///
/// This proves only digest equality, not signature provenance or anti-rollback.
pub fn verify_manifest_digest<V: DigestVerifier>(
    expected: Digest,
    canonical_manifest: &[u8],
    verifier: &V,
) -> bool {
    verifier.verify(expected, canonical_manifest)
}

const fn encode_class(class: ObjectClass) -> u8 {
    match class {
        ObjectClass::Kernel => CLASS_KERNEL,
        ObjectClass::SystemComponent => CLASS_SYSTEM_COMPONENT,
        ObjectClass::RecoveryComponent => CLASS_RECOVERY_COMPONENT,
        ObjectClass::ApplicationPackage => CLASS_APPLICATION_PACKAGE,
        ObjectClass::Resource => CLASS_RESOURCE,
    }
}

fn decode_class(value: u8) -> Result<ObjectClass, ManifestDecodeError> {
    match value {
        CLASS_KERNEL => Ok(ObjectClass::Kernel),
        CLASS_SYSTEM_COMPONENT => Ok(ObjectClass::SystemComponent),
        CLASS_RECOVERY_COMPONENT => Ok(ObjectClass::RecoveryComponent),
        CLASS_APPLICATION_PACKAGE => Ok(ObjectClass::ApplicationPackage),
        CLASS_RESOURCE => Ok(ObjectClass::Resource),
        other => Err(ManifestDecodeError::InvalidClass(other)),
    }
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
    use crate::AWSTOREFS_BLOCK_BYTES;

    fn digest(seed: u8) -> Digest {
        Digest::new([seed; 32]).unwrap()
    }

    fn sample_manifest() -> StoreManifest<4> {
        let mut manifest = StoreManifest::new(42, 128).unwrap();
        manifest
            .push(ObjectDescriptor::new(
                digest(1),
                ObjectClass::Kernel,
                ObjectExtent::new(8, 1, AWSTOREFS_BLOCK_BYTES).unwrap(),
            ))
            .unwrap();
        manifest
            .push(ObjectDescriptor::new(
                digest(2),
                ObjectClass::Resource,
                ObjectExtent::new(9, 1, 7).unwrap(),
            ))
            .unwrap();
        manifest
    }

    #[test]
    fn canonical_manifest_round_trips() {
        let manifest = sample_manifest();
        let mut bytes = [0_u8; MANIFEST_HEADER_BYTES + 4 * OBJECT_RECORD_BYTES];
        let used = encode_manifest(&manifest, &mut bytes).unwrap();
        assert_eq!(used, MANIFEST_HEADER_BYTES + 2 * OBJECT_RECORD_BYTES);
        let parsed = decode_manifest::<4>(&bytes[..used]).unwrap();
        assert_eq!(parsed.generation(), 42);
        assert_eq!(parsed.volume_blocks(), 128);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed.object(0), manifest.object(0));
        assert_eq!(parsed.object(1), manifest.object(1));

        let mut encoded_again = [0_u8; MANIFEST_HEADER_BYTES + 4 * OBJECT_RECORD_BYTES];
        let second_used = encode_manifest(&parsed, &mut encoded_again).unwrap();
        assert_eq!(&encoded_again[..second_used], &bytes[..used]);
    }

    #[test]
    fn parser_rejects_unknown_class_and_reserved_bytes() {
        let manifest = sample_manifest();
        let mut bytes = [0_u8; MANIFEST_HEADER_BYTES + 4 * OBJECT_RECORD_BYTES];
        let used = encode_manifest(&manifest, &mut bytes).unwrap();

        let mut unknown_class = bytes;
        unknown_class[MANIFEST_HEADER_BYTES] = 99;
        assert_eq!(
            decode_manifest::<4>(&unknown_class[..used]).err(),
            Some(ManifestDecodeError::InvalidClass(99))
        );

        let mut reserved = bytes;
        reserved[36] = 1;
        assert_eq!(
            decode_manifest::<4>(&reserved[..used]).err(),
            Some(ManifestDecodeError::NonCanonicalReservedBytes)
        );
    }

    #[test]
    fn parser_reuses_semantic_overlap_and_bounds_validation() {
        let manifest = sample_manifest();
        let mut bytes = [0_u8; MANIFEST_HEADER_BYTES + 4 * OBJECT_RECORD_BYTES];
        let used = encode_manifest(&manifest, &mut bytes).unwrap();

        let second = MANIFEST_HEADER_BYTES + OBJECT_RECORD_BYTES;
        bytes[second + 8..second + 16].copy_from_slice(&8_u64.to_le_bytes());
        assert_eq!(
            decode_manifest::<4>(&bytes[..used]).err(),
            Some(ManifestDecodeError::InvalidPlan(
                ManifestPlanError::OverlappingExtent
            ))
        );
    }

    #[test]
    fn parser_is_fixed_capacity_and_exact_length() {
        let manifest = sample_manifest();
        let mut bytes = [0_u8; MANIFEST_HEADER_BYTES + 4 * OBJECT_RECORD_BYTES];
        let used = encode_manifest(&manifest, &mut bytes).unwrap();
        assert_eq!(
            decode_manifest::<1>(&bytes[..used]).err(),
            Some(ManifestDecodeError::ObjectCountOverflow)
        );
        assert_eq!(
            decode_manifest::<4>(&bytes[..used - 1]).err(),
            Some(ManifestDecodeError::InvalidLength)
        );
    }
}
