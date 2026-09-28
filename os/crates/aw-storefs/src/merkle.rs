use super::{ObjectDescriptor, ReadWindow};
use aw_fs_core::Digest;

pub trait MerkleHasher {
    /// Hashes one logical object chunk with explicit position and logical length domain separation.
    fn hash_leaf(&self, leaf_index: u64, valid_bytes: u32, bytes: &[u8]) -> Digest;

    /// Hashes an internal node with explicit level domain separation.
    fn hash_parent(&self, level: u32, left: Digest, right: Digest) -> Digest;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MerkleProofError {
    ZeroLeafCount,
    LeafOutOfRange,
    DepthMismatch { expected: usize, actual: usize },
    MissingSibling { level: usize },
    UnexpectedSibling { level: usize },
    TrailingSibling { level: usize },
    ObjectShapeMismatch,
    ChunkLengthMismatch { expected: u32, actual: usize },
    RootMismatch,
}

pub struct MerkleProof<const MAX_DEPTH: usize> {
    leaf_index: u64,
    leaf_count: u64,
    siblings: [Option<Digest>; MAX_DEPTH],
    depth: usize,
}

impl<const MAX_DEPTH: usize> MerkleProof<MAX_DEPTH> {
    pub fn new(
        leaf_index: u64,
        leaf_count: u64,
        siblings: [Option<Digest>; MAX_DEPTH],
        depth: usize,
    ) -> Result<Self, MerkleProofError> {
        if leaf_count == 0 {
            return Err(MerkleProofError::ZeroLeafCount);
        }
        if leaf_index >= leaf_count {
            return Err(MerkleProofError::LeafOutOfRange);
        }
        let expected_depth = required_depth(leaf_count);
        if depth != expected_depth || depth > MAX_DEPTH {
            return Err(MerkleProofError::DepthMismatch {
                expected: expected_depth,
                actual: depth,
            });
        }

        let mut index = leaf_index;
        let mut nodes = leaf_count;
        for (level, sibling) in siblings.iter().enumerate().take(depth) {
            let requires_sibling = index % 2 == 1 || index + 1 < nodes;
            if requires_sibling && sibling.is_none() {
                return Err(MerkleProofError::MissingSibling { level });
            }
            if !requires_sibling && sibling.is_some() {
                return Err(MerkleProofError::UnexpectedSibling { level });
            }
            index /= 2;
            nodes = nodes.div_ceil(2);
        }
        if let Some((level, _)) = siblings
            .iter()
            .enumerate()
            .skip(depth)
            .find(|(_, sibling)| sibling.is_some())
        {
            return Err(MerkleProofError::TrailingSibling { level });
        }

        Ok(Self {
            leaf_index,
            leaf_count,
            siblings,
            depth,
        })
    }

    #[must_use]
    pub const fn leaf_index(&self) -> u64 {
        self.leaf_index
    }

    #[must_use]
    pub const fn leaf_count(&self) -> u64 {
        self.leaf_count
    }

    #[must_use]
    pub const fn depth(&self) -> usize {
        self.depth
    }
}

pub struct VerifiedChunk<'a> {
    descriptor: ObjectDescriptor,
    leaf_index: u64,
    bytes: &'a [u8],
}

impl VerifiedChunk<'_> {
    #[must_use]
    pub const fn descriptor(&self) -> ObjectDescriptor {
        self.descriptor
    }

    #[must_use]
    pub const fn leaf_index(&self) -> u64 {
        self.leaf_index
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.bytes
    }
}

pub fn verify_chunk<'a, const MAX_DEPTH: usize, H: MerkleHasher>(
    descriptor: ObjectDescriptor,
    bytes: &'a [u8],
    proof: &MerkleProof<MAX_DEPTH>,
    hasher: &H,
) -> Result<VerifiedChunk<'a>, MerkleProofError> {
    if proof.leaf_count != descriptor.extent().block_count() {
        return Err(MerkleProofError::ObjectShapeMismatch);
    }
    let window = descriptor
        .extent()
        .read_window(proof.leaf_index)
        .ok_or(MerkleProofError::ObjectShapeMismatch)?;
    validate_chunk_length(window, bytes)?;

    let mut current = hasher.hash_leaf(proof.leaf_index, window.valid_bytes(), bytes);
    let mut index = proof.leaf_index;
    let mut nodes = proof.leaf_count;
    for (level, sibling) in proof.siblings.iter().copied().enumerate().take(proof.depth) {
        current = if index % 2 == 1 {
            hasher.hash_parent(level as u32, sibling.expect("validated sibling"), current)
        } else if index + 1 < nodes {
            hasher.hash_parent(level as u32, current, sibling.expect("validated sibling"))
        } else {
            hasher.hash_parent(level as u32, current, current)
        };
        index /= 2;
        nodes = nodes.div_ceil(2);
    }

    if current != descriptor.digest() {
        return Err(MerkleProofError::RootMismatch);
    }
    Ok(VerifiedChunk {
        descriptor,
        leaf_index: proof.leaf_index,
        bytes,
    })
}

fn validate_chunk_length(window: ReadWindow, bytes: &[u8]) -> Result<(), MerkleProofError> {
    if bytes.len() != window.valid_bytes() as usize {
        return Err(MerkleProofError::ChunkLengthMismatch {
            expected: window.valid_bytes(),
            actual: bytes.len(),
        });
    }
    Ok(())
}

#[must_use]
pub fn required_depth(mut leaf_count: u64) -> usize {
    let mut depth = 0;
    while leaf_count > 1 {
        leaf_count = leaf_count.div_ceil(2);
        depth += 1;
    }
    depth
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ObjectClass, ObjectExtent};

    fn digest(seed: u8) -> Digest {
        Digest::new([seed.max(1); 32]).unwrap()
    }

    struct TestHasher;

    impl MerkleHasher for TestHasher {
        fn hash_leaf(&self, leaf_index: u64, valid_bytes: u32, bytes: &[u8]) -> Digest {
            let seed = bytes.first().copied().unwrap_or(1)
                ^ leaf_index as u8
                ^ (valid_bytes as u8).wrapping_mul(3);
            digest(seed)
        }

        fn hash_parent(&self, level: u32, left: Digest, right: Digest) -> Digest {
            let seed = left.bytes()[0] ^ right.bytes()[0] ^ (level as u8).wrapping_add(17);
            digest(seed)
        }
    }

    #[test]
    fn required_depth_handles_even_and_odd_trees() {
        assert_eq!(required_depth(1), 0);
        assert_eq!(required_depth(2), 1);
        assert_eq!(required_depth(3), 2);
        assert_eq!(required_depth(4), 2);
        assert_eq!(required_depth(5), 3);
    }

    #[test]
    fn two_leaf_object_verifies_each_chunk_against_same_root() {
        let hasher = TestHasher;
        let first = [4_u8; 4096];
        let second = [9_u8; 904];
        let left = hasher.hash_leaf(0, 4096, &first);
        let right = hasher.hash_leaf(1, 904, &second);
        let root = hasher.hash_parent(0, left, right);
        let descriptor = ObjectDescriptor::new(
            root,
            ObjectClass::SystemComponent,
            ObjectExtent::new(8, 2, 5000).unwrap(),
        );

        let left_proof = MerkleProof::<2>::new(0, 2, [Some(right), None], 1).unwrap();
        let right_proof = MerkleProof::<2>::new(1, 2, [Some(left), None], 1).unwrap();
        assert_eq!(
            verify_chunk(descriptor, &first, &left_proof, &hasher)
                .unwrap()
                .leaf_index(),
            0
        );
        assert_eq!(
            verify_chunk(descriptor, &second, &right_proof, &hasher)
                .unwrap()
                .leaf_index(),
            1
        );
    }

    #[test]
    fn odd_tree_requires_canonical_duplicate_last_shape() {
        let a = digest(1);
        let b = digest(2);
        assert!(matches!(
            MerkleProof::<2>::new(2, 3, [Some(a), Some(b)], 2),
            Err(MerkleProofError::UnexpectedSibling { level: 0 })
        ));
        assert!(MerkleProof::<2>::new(2, 3, [None, Some(b)], 2).is_ok());
    }

    #[test]
    fn verifier_rejects_padding_and_wrong_root() {
        let hasher = TestHasher;
        let bytes = [7_u8; 3];
        let leaf = hasher.hash_leaf(0, 3, &bytes);
        let descriptor = ObjectDescriptor::new(
            leaf,
            ObjectClass::Resource,
            ObjectExtent::new(8, 1, 3).unwrap(),
        );
        let proof = MerkleProof::<1>::new(0, 1, [None], 0).unwrap();

        assert!(verify_chunk(descriptor, &bytes, &proof, &hasher).is_ok());
        assert_eq!(
            verify_chunk(descriptor, &[7_u8; 4], &proof, &hasher).err(),
            Some(MerkleProofError::ChunkLengthMismatch {
                expected: 3,
                actual: 4
            })
        );

        let wrong_descriptor =
            ObjectDescriptor::new(digest(99), ObjectClass::Resource, descriptor.extent());
        assert_eq!(
            verify_chunk(wrong_descriptor, &bytes, &proof, &hasher).err(),
            Some(MerkleProofError::RootMismatch)
        );
    }
}
