use super::AuthenticatedManifest;
use super::chunk::{AuthenticatedChunk, AuthenticatedChunkError, verify_authenticated_chunk};
use crate::AWSTOREFS_BLOCK_BYTES;
use crate::merkle::{MerkleHasher, MerkleProof};
use aw_fs_core::Digest;

/// Minimal read-only block source for AWStoreFS.
///
/// Implementations must fill the complete caller-provided block buffer or return an error. No
/// write, flush, discard, resize, or mutation capability is part of this interface.
pub trait BlockReader {
    type Error;

    fn read_block(&self, physical_block: u64, output: &mut [u8]) -> Result<(), Self::Error>;
}

#[derive(Debug, Eq, PartialEq)]
pub enum ReadAuthenticatedChunkError<E> {
    BufferSize { expected: usize, actual: usize },
    UnknownObject,
    InvalidChunkIndex,
    Device(E),
    Authentication(AuthenticatedChunkError),
}

/// Reads exactly the physical block selected by an authenticated manifest object and verifies the
/// requested logical chunk before exposing it.
///
/// The caller owns `block_buffer`; returned bytes borrow from that buffer. The API therefore does
/// not allocate and cannot silently cache unauthenticated content. The final object block exposes
/// only its logical bytes, never trailing disk padding.
pub fn read_authenticated_chunk<'a, const OBJECTS: usize, const MAX_DEPTH: usize, D, H>(
    manifest: &AuthenticatedManifest<OBJECTS>,
    object_digest: Digest,
    proof: &MerkleProof<MAX_DEPTH>,
    device: &D,
    block_buffer: &'a mut [u8],
    hasher: &H,
) -> Result<AuthenticatedChunk<'a>, ReadAuthenticatedChunkError<D::Error>>
where
    D: BlockReader,
    H: MerkleHasher,
{
    let expected_block_bytes = AWSTOREFS_BLOCK_BYTES as usize;
    if block_buffer.len() != expected_block_bytes {
        return Err(ReadAuthenticatedChunkError::BufferSize {
            expected: expected_block_bytes,
            actual: block_buffer.len(),
        });
    }

    let descriptor = manifest
        .find_by_digest(object_digest)
        .ok_or(ReadAuthenticatedChunkError::UnknownObject)?;
    let window = descriptor
        .extent()
        .read_window(proof.leaf_index())
        .ok_or(ReadAuthenticatedChunkError::InvalidChunkIndex)?;

    block_buffer.fill(0);
    device
        .read_block(window.physical_block(), block_buffer)
        .map_err(ReadAuthenticatedChunkError::Device)?;
    let logical = &block_buffer[..window.valid_bytes() as usize];
    verify_authenticated_chunk(manifest, object_digest, logical, proof, hasher)
        .map_err(ReadAuthenticatedChunkError::Authentication)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ObjectDescriptor;
    use crate::merkle::MerkleProofError;
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

    struct FakeDevice {
        expected_block: u64,
        bytes: [u8; 4096],
        fail: bool,
    }

    impl BlockReader for FakeDevice {
        type Error = &'static str;

        fn read_block(&self, physical_block: u64, output: &mut [u8]) -> Result<(), Self::Error> {
            if self.fail {
                return Err("device failure");
            }
            if physical_block != self.expected_block || output.len() != self.bytes.len() {
                return Err("unexpected read");
            }
            output.copy_from_slice(&self.bytes);
            Ok(())
        }
    }

    fn trusted_single_block_manifest(logical_bytes: u64) -> AuthenticatedManifest<1> {
        let mut manifest = StoreManifest::<1>::new(42, 64).unwrap();
        manifest
            .push(ObjectDescriptor::new(
                digest(7),
                ObjectClass::SystemComponent,
                ObjectExtent::new(8, 1, logical_bytes).unwrap(),
            ))
            .unwrap();
        let mut bytes = [0_u8; MANIFEST_HEADER_BYTES + OBJECT_RECORD_BYTES];
        let used = encode_manifest(&manifest, &mut bytes).unwrap();
        authenticate_manifest::<1, _>(&bytes[..used], 42, &Allow).unwrap()
    }

    #[test]
    fn reader_uses_authenticated_extent_and_exposes_only_logical_bytes() {
        let manifest = trusted_single_block_manifest(4);
        let proof = MerkleProof::<0>::new(0, 1, [], 0).unwrap();
        let mut disk_block = [99_u8; 4096];
        disk_block[..4].fill(7);
        let device = FakeDevice {
            expected_block: 8,
            bytes: disk_block,
            fail: false,
        };
        let mut buffer = [0_u8; 4096];
        let chunk = read_authenticated_chunk(
            &manifest,
            digest(7),
            &proof,
            &device,
            &mut buffer,
            &SingleLeafHasher,
        )
        .unwrap();
        assert_eq!(chunk.bytes(), &[7_u8; 4]);
        assert_eq!(chunk.generation(), 42);
    }

    #[test]
    fn reader_rejects_wrong_buffer_unknown_object_and_device_failure() {
        let manifest = trusted_single_block_manifest(4);
        let proof = MerkleProof::<0>::new(0, 1, [], 0).unwrap();
        let device = FakeDevice {
            expected_block: 8,
            bytes: [7_u8; 4096],
            fail: false,
        };
        let mut short = [0_u8; 4095];
        assert_eq!(
            read_authenticated_chunk(
                &manifest,
                digest(7),
                &proof,
                &device,
                &mut short,
                &SingleLeafHasher
            )
            .err(),
            Some(ReadAuthenticatedChunkError::BufferSize {
                expected: 4096,
                actual: 4095
            })
        );

        let mut buffer = [0_u8; 4096];
        assert!(matches!(
            read_authenticated_chunk(
                &manifest,
                digest(8),
                &proof,
                &device,
                &mut buffer,
                &SingleLeafHasher
            ),
            Err(ReadAuthenticatedChunkError::UnknownObject)
        ));

        let failing = FakeDevice {
            expected_block: 8,
            bytes: [7_u8; 4096],
            fail: true,
        };
        assert_eq!(
            read_authenticated_chunk(
                &manifest,
                digest(7),
                &proof,
                &failing,
                &mut buffer,
                &SingleLeafHasher
            )
            .err(),
            Some(ReadAuthenticatedChunkError::Device("device failure"))
        );
    }

    #[test]
    fn reader_never_returns_corrupt_disk_bytes() {
        let manifest = trusted_single_block_manifest(4);
        let proof = MerkleProof::<0>::new(0, 1, [], 0).unwrap();
        let device = FakeDevice {
            expected_block: 8,
            bytes: [8_u8; 4096],
            fail: false,
        };
        let mut buffer = [0_u8; 4096];
        assert!(matches!(
            read_authenticated_chunk(
                &manifest,
                digest(7),
                &proof,
                &device,
                &mut buffer,
                &SingleLeafHasher
            ),
            Err(ReadAuthenticatedChunkError::Authentication(
                AuthenticatedChunkError::Verification(MerkleProofError::RootMismatch)
            ))
        ));
    }
}
