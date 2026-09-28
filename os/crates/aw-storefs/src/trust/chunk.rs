use super::AuthenticatedManifest;
use crate::ObjectDescriptor;
use crate::merkle::{MerkleHasher, MerkleProof, MerkleProofError, VerifiedChunk, verify_chunk};
use aw_fs_core::Digest;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthenticatedChunkError {
    UnknownObject,
    Verification(MerkleProofError),
}

/// Proof that a chunk belongs to an object referenced by an authenticated, anti-rollback manifest
/// and that the chunk's Merkle path reaches the object root from that manifest.
///
/// This proof still does not grant executable authority. Execution policy remains a separate gate.
pub struct AuthenticatedChunk<'a> {
    generation: u64,
    verified: VerifiedChunk<'a>,
}

impl AuthenticatedChunk<'_> {
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn descriptor(&self) -> ObjectDescriptor {
        self.verified.descriptor()
    }

    #[must_use]
    pub const fn leaf_index(&self) -> u64 {
        self.verified.leaf_index()
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.verified.bytes()
    }
}

pub fn verify_authenticated_chunk<
    'a,
    const OBJECTS: usize,
    const MAX_DEPTH: usize,
    H: MerkleHasher,
>(
    manifest: &AuthenticatedManifest<OBJECTS>,
    object_digest: Digest,
    bytes: &'a [u8],
    proof: &MerkleProof<MAX_DEPTH>,
    hasher: &H,
) -> Result<AuthenticatedChunk<'a>, AuthenticatedChunkError> {
    let descriptor = manifest
        .find_by_digest(object_digest)
        .ok_or(AuthenticatedChunkError::UnknownObject)?;
    let verified = verify_chunk(descriptor, bytes, proof, hasher)
        .map_err(AuthenticatedChunkError::Verification)?;
    Ok(AuthenticatedChunk {
        generation: manifest.generation(),
        verified,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trust::{ManifestAuthenticator, authenticate_manifest};
    use crate::wire::{MANIFEST_HEADER_BYTES, OBJECT_RECORD_BYTES, encode_manifest};
    use crate::{ObjectClass, ObjectExtent, StoreManifest};

    fn digest(seed: u8) -> Digest {
        Digest::new([seed.max(1); 32]).unwrap()
    }

    struct Allow;

    impl ManifestAuthenticator for Allow {
        fn authenticate(&self, _canonical_manifest: &[u8]) -> bool {
            true
        }
    }

    struct SingleLeafHasher;

    impl MerkleHasher for SingleLeafHasher {
        fn hash_leaf(&self, _leaf_index: u64, _valid_bytes: u32, bytes: &[u8]) -> Digest {
            digest(bytes.first().copied().unwrap_or(1))
        }

        fn hash_parent(&self, level: u32, left: Digest, right: Digest) -> Digest {
            let seed = left.bytes()[0] ^ right.bytes()[0] ^ (level as u8).wrapping_add(1);
            digest(seed)
        }
    }

    fn authenticated_manifest() -> AuthenticatedManifest<1> {
        let mut manifest = StoreManifest::<1>::new(42, 64).unwrap();
        manifest
            .push(ObjectDescriptor::new(
                digest(7),
                ObjectClass::SystemComponent,
                ObjectExtent::new(8, 1, 4).unwrap(),
            ))
            .unwrap();
        let mut bytes = [0_u8; MANIFEST_HEADER_BYTES + OBJECT_RECORD_BYTES];
        let used = encode_manifest(&manifest, &mut bytes).unwrap();
        authenticate_manifest::<1, _>(&bytes[..used], 42, &Allow).unwrap()
    }

    #[test]
    fn chunk_requires_authenticated_manifest_membership_and_merkle_root() {
        let manifest = authenticated_manifest();
        let proof = MerkleProof::<0>::new(0, 1, [], 0).unwrap();
        let bytes = [7_u8; 4];
        let chunk =
            verify_authenticated_chunk(&manifest, digest(7), &bytes, &proof, &SingleLeafHasher)
                .unwrap();
        assert_eq!(chunk.generation(), 42);
        assert_eq!(chunk.leaf_index(), 0);
        assert_eq!(chunk.bytes(), &bytes);
    }

    #[test]
    fn unknown_or_corrupt_chunk_fails_closed() {
        let manifest = authenticated_manifest();
        let proof = MerkleProof::<0>::new(0, 1, [], 0).unwrap();
        assert!(matches!(
            verify_authenticated_chunk(&manifest, digest(8), &[7_u8; 4], &proof, &SingleLeafHasher),
            Err(AuthenticatedChunkError::UnknownObject)
        ));
        assert!(matches!(
            verify_authenticated_chunk(&manifest, digest(7), &[8_u8; 4], &proof, &SingleLeafHasher),
            Err(AuthenticatedChunkError::Verification(
                MerkleProofError::RootMismatch
            ))
        ));
    }
}
