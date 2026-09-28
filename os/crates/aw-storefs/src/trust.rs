pub mod chunk;
pub mod reader;

use super::{
    DigestVerifier, ObjectDescriptor, ObjectVerificationError, StoreManifest, VerifiedObject,
    verify_resident_object,
};
use crate::wire::{ManifestDecodeError, decode_manifest};
use aw_fs_core::Digest;

/// Signature/provenance boundary for exact canonical manifest bytes.
///
/// Implementations are expected to bind the bytes to a trusted signing policy outside AWStoreFS.
/// This trait intentionally does not prescribe a signature algorithm or key-storage mechanism.
pub trait ManifestAuthenticator {
    fn authenticate(&self, canonical_manifest: &[u8]) -> bool;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManifestAuthenticationError {
    Rejected,
    Decode(ManifestDecodeError),
    RollbackRejected { declared: u64, minimum: u64 },
}

/// Proof that exact canonical manifest bytes passed an external authenticity check and the local
/// generation rollback floor.
pub struct AuthenticatedManifest<const OBJECTS: usize> {
    manifest: StoreManifest<OBJECTS>,
}

impl<const OBJECTS: usize> AuthenticatedManifest<OBJECTS> {
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.manifest.generation()
    }

    #[must_use]
    pub const fn volume_blocks(&self) -> u64 {
        self.manifest.volume_blocks()
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.manifest.len()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.manifest.is_empty()
    }

    #[must_use]
    pub fn object(&self, index: usize) -> Option<ObjectDescriptor> {
        self.manifest.object(index)
    }

    #[must_use]
    pub fn find_by_digest(&self, digest: Digest) -> Option<ObjectDescriptor> {
        self.manifest.find_by_digest(digest)
    }
}

/// Authenticates opaque canonical bytes before parsing them into a trusted semantic manifest.
///
/// Parser safety is still required because callers may choose to parse separately for diagnostics,
/// but this trust-producing path never upgrades unauthenticated parsed state into trusted state.
pub fn authenticate_manifest<const OBJECTS: usize, A: ManifestAuthenticator>(
    canonical_manifest: &[u8],
    minimum_generation: u64,
    authenticator: &A,
) -> Result<AuthenticatedManifest<OBJECTS>, ManifestAuthenticationError> {
    if !authenticator.authenticate(canonical_manifest) {
        return Err(ManifestAuthenticationError::Rejected);
    }
    let manifest =
        decode_manifest(canonical_manifest).map_err(ManifestAuthenticationError::Decode)?;
    if manifest.generation() < minimum_generation {
        return Err(ManifestAuthenticationError::RollbackRejected {
            declared: manifest.generation(),
            minimum: minimum_generation,
        });
    }
    Ok(AuthenticatedManifest { manifest })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthenticatedObjectError {
    UnknownObject,
    Verification(ObjectVerificationError),
}

/// Proof that bytes both belong to an authenticated manifest and match that manifest's object
/// digest through a caller-supplied cryptographic verifier.
///
/// This still does not grant execution authority. Boot composition, platform policy and runtime
/// sandbox/capability policy remain separate gates.
pub struct AuthenticatedObject<'a> {
    generation: u64,
    verified: VerifiedObject<'a>,
}

impl AuthenticatedObject<'_> {
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn descriptor(&self) -> ObjectDescriptor {
        self.verified.descriptor()
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.verified.bytes()
    }
}

pub fn verify_authenticated_object<'a, const OBJECTS: usize, V: DigestVerifier>(
    manifest: &AuthenticatedManifest<OBJECTS>,
    digest: Digest,
    bytes: &'a [u8],
    verifier: &V,
) -> Result<AuthenticatedObject<'a>, AuthenticatedObjectError> {
    let descriptor = manifest
        .find_by_digest(digest)
        .ok_or(AuthenticatedObjectError::UnknownObject)?;
    let verified = verify_resident_object(descriptor, bytes, verifier)
        .map_err(AuthenticatedObjectError::Verification)?;
    Ok(AuthenticatedObject {
        generation: manifest.generation(),
        verified,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{MANIFEST_HEADER_BYTES, OBJECT_RECORD_BYTES, encode_manifest};
    use crate::{ObjectClass, ObjectExtent, StoreManifest};

    fn digest(seed: u8) -> Digest {
        Digest::new([seed; 32]).unwrap()
    }

    fn canonical_manifest() -> ([u8; MANIFEST_HEADER_BYTES + OBJECT_RECORD_BYTES], usize) {
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
        (bytes, used)
    }

    struct Allow;
    impl ManifestAuthenticator for Allow {
        fn authenticate(&self, _canonical_manifest: &[u8]) -> bool {
            true
        }
    }

    struct Reject;
    impl ManifestAuthenticator for Reject {
        fn authenticate(&self, _canonical_manifest: &[u8]) -> bool {
            false
        }
    }

    struct ExactDigest;
    impl DigestVerifier for ExactDigest {
        fn verify(&self, expected: Digest, bytes: &[u8]) -> bool {
            expected == digest(bytes.first().copied().unwrap_or(0))
        }
    }

    #[test]
    fn rejected_signature_never_produces_trusted_manifest() {
        let (bytes, used) = canonical_manifest();
        assert!(matches!(
            authenticate_manifest::<1, _>(&bytes[..used], 1, &Reject),
            Err(ManifestAuthenticationError::Rejected)
        ));
    }

    #[test]
    fn authenticated_manifest_enforces_rollback_floor() {
        let (bytes, used) = canonical_manifest();
        assert!(matches!(
            authenticate_manifest::<1, _>(&bytes[..used], 43, &Allow),
            Err(ManifestAuthenticationError::RollbackRejected {
                declared: 42,
                minimum: 43
            })
        ));
        let trusted = authenticate_manifest::<1, _>(&bytes[..used], 42, &Allow).unwrap();
        assert_eq!(trusted.generation(), 42);
        assert_eq!(trusted.len(), 1);
    }

    #[test]
    fn object_must_be_referenced_and_digest_verified() {
        let (bytes, used) = canonical_manifest();
        let trusted = authenticate_manifest::<1, _>(&bytes[..used], 42, &Allow).unwrap();
        let object_bytes = [7_u8; 4];
        let object =
            verify_authenticated_object(&trusted, digest(7), &object_bytes, &ExactDigest).unwrap();
        assert_eq!(object.generation(), 42);
        assert_eq!(object.bytes(), &object_bytes);

        assert!(matches!(
            verify_authenticated_object(&trusted, digest(8), &object_bytes, &ExactDigest),
            Err(AuthenticatedObjectError::UnknownObject)
        ));
        assert!(matches!(
            verify_authenticated_object(&trusted, digest(7), &[8_u8; 4], &ExactDigest),
            Err(AuthenticatedObjectError::Verification(
                ObjectVerificationError::DigestMismatch
            ))
        ));
    }
}
