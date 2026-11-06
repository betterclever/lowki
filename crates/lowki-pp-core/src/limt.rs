//! Lean incremental Merkle tree (zk-kit LeanIMT semantics) over Poseidon T3, a
//! port of Kohaku's `packages/poseidon-lean-imt/src/limt.ts`, plus
//! `proofFromTree` from `packages/privacy-pools/src/utils/proof.util.ts`.
//!
//! LeanIMT: a node without a right sibling is promoted unchanged (no zero padding),
//! so the depth is `ceil(log2(size))` and a one-leaf tree's root is the leaf.

use std::collections::HashMap;

use crate::poseidon::poseidon2;
use crate::{Error, Fr};

/// Depth the Privacy Pools withdraw circuit expects; `proof_from_tree` pads to it.
pub const PADDED_PROOF_DEPTH: usize = 32;

/// A LeanIMT inclusion proof.
///
/// `index` is the *compressed* path index: bit `i` is set when `siblings[i]` is on the
/// left. Levels where the node had no sibling are skipped (zk-kit semantics).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MerkleProof {
    pub root: Fr,
    pub leaf: Fr,
    pub index: u64,
    pub siblings: Vec<Fr>,
}

/// LeanIMT over Poseidon T3.
#[derive(Clone, Debug, Default)]
pub struct LeanImt {
    /// `levels[0]` are the leaves, the last level holds `[root]`.
    levels: Vec<Vec<Fr>>,
    /// First occurrence of each leaf.
    positions: HashMap<Fr, usize>,
}

/// `ceil(log2(x))` for `x >= 1` (0 for x <= 1).
fn ceil_log2(x: usize) -> usize {
    if x <= 1 {
        0
    } else {
        (usize::BITS - (x - 1).leading_zeros()) as usize
    }
}

fn set(level: &mut Vec<Fr>, index: usize, value: Fr) {
    if index < level.len() {
        level[index] = value;
    } else {
        debug_assert_eq!(index, level.len());
        level.push(value);
    }
}

impl LeanImt {
    /// An empty tree.
    pub fn new() -> Self {
        Self {
            levels: vec![Vec::new()],
            positions: HashMap::new(),
        }
    }

    /// Builds a tree from `leaves` (like `new PoseidonT3LeanIMT(leaves)`).
    pub fn from_leaves(leaves: &[Fr]) -> Self {
        let mut t = Self::new();
        if !leaves.is_empty() {
            t.insert_many(leaves).expect("non-empty");
        }
        t
    }

    /// The root, or `None` for an empty tree.
    pub fn root(&self) -> Option<Fr> {
        self.levels[self.depth()].first().copied()
    }

    /// Number of levels above the leaves.
    pub fn depth(&self) -> usize {
        self.levels.len() - 1
    }

    /// Number of leaves.
    pub fn size(&self) -> usize {
        self.levels[0].len()
    }

    /// The leaves, in insertion order.
    pub fn leaves(&self) -> &[Fr] {
        &self.levels[0]
    }

    /// Index of the first occurrence of `leaf`.
    pub fn index_of(&self, leaf: &Fr) -> Option<usize> {
        self.positions.get(leaf).copied()
    }

    /// Whether `leaf` is in the tree.
    pub fn has(&self, leaf: &Fr) -> bool {
        self.positions.contains_key(leaf)
    }

    /// Appends one leaf.
    pub fn insert(&mut self, leaf: Fr) {
        let mut index = self.size();
        self.positions.entry(leaf).or_insert(index);
        if self.depth() < ceil_log2(index + 1) {
            self.levels.push(Vec::new());
        }
        let depth = self.depth();
        let mut node = leaf;
        for level in 0..depth {
            set(&mut self.levels[level], index, node);
            if index & 1 == 1 {
                node = poseidon2(self.levels[level][index - 1], node);
            }
            index >>= 1;
        }
        self.levels[depth] = vec![node];
    }

    /// Appends many leaves, recomputing only the touched parents.
    pub fn insert_many(&mut self, leaves: &[Fr]) -> Result<(), Error> {
        if leaves.is_empty() {
            return Err(Error::NoLeaves);
        }
        for leaf in leaves {
            let i = self.levels[0].len();
            self.positions.entry(*leaf).or_insert(i);
            self.levels[0].push(*leaf);
        }
        while self.depth() < ceil_log2(self.size()) {
            self.levels.push(Vec::new());
        }
        let mut start = (self.size() - leaves.len()) >> 1;
        for level in 0..self.depth() {
            let prev_len = self.levels[level].len();
            let end = prev_len.div_ceil(2);
            for i in start..end {
                let right = 2 * i + 1;
                let parent = if right < prev_len {
                    poseidon2(self.levels[level][2 * i], self.levels[level][right])
                } else {
                    self.levels[level][2 * i]
                };
                set(&mut self.levels[level + 1], i, parent);
            }
            start >>= 1;
        }
        Ok(())
    }

    /// Proof for the leaf at `index` (unpadded; see [`proof_from_tree`]).
    pub fn generate_proof(&self, index: usize) -> Result<MerkleProof, Error> {
        if index >= self.size() {
            return Err(Error::IndexOutOfRange(index));
        }
        let leaf = self.levels[0][index];
        let mut siblings = Vec::with_capacity(self.depth());
        let mut path_index = 0u64;
        let mut pos = index;
        for level in 0..self.depth() {
            let is_right = pos & 1 == 1;
            let sibling = if is_right { pos - 1 } else { pos + 1 };
            if sibling < self.levels[level].len() {
                if is_right {
                    path_index |= 1u64 << siblings.len();
                }
                siblings.push(self.levels[level][sibling]);
            }
            pos >>= 1;
        }
        Ok(MerkleProof {
            root: self.root().expect("non-empty"),
            leaf,
            index: path_index,
            siblings,
        })
    }
}

/// Verifies a LeanIMT proof (Kohaku `PoseidonT3LeanIMT.verifyProof`). Zero padding
/// added by [`proof_from_tree`] is *not* understood here (it changes the hash chain),
/// exactly as in the reference; verify the unpadded proof.
pub fn verify_proof(proof: &MerkleProof) -> bool {
    let mut node = proof.leaf;
    for (i, s) in proof.siblings.iter().enumerate() {
        let is_right = i < 64 && (proof.index >> i) & 1 == 1;
        node = if is_right {
            poseidon2(*s, node)
        } else {
            poseidon2(node, *s)
        };
    }
    node == proof.root
}

/// Kohaku `proofFromTree`: proof for the first occurrence of `leaf`, with siblings
/// right-padded with zeros to [`PADDED_PROOF_DEPTH`] (the circuit's fixed depth).
pub fn proof_from_tree(tree: &LeanImt, leaf: &Fr) -> Result<MerkleProof, Error> {
    let index = tree.index_of(leaf).ok_or(Error::LeafNotFound)?;
    let mut proof = tree.generate_proof(index)?;
    if proof.siblings.len() < PADDED_PROOF_DEPTH {
        proof.siblings.resize(PADDED_PROOF_DEPTH, Fr::from(0u64));
    }
    Ok(proof)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_trees() {
        assert_eq!(ceil_log2(1), 0);
        assert_eq!(ceil_log2(2), 1);
        assert_eq!(ceil_log2(3), 2);
        assert_eq!(ceil_log2(1024), 10);
        assert_eq!(ceil_log2(1025), 11);
        let mut t = LeanImt::new();
        assert_eq!(t.root(), None);
        assert_eq!(t.insert_many(&[]), Err(Error::NoLeaves));
        t.insert(Fr::from(5u64));
        assert_eq!(t.root(), Some(Fr::from(5u64)));
        assert_eq!(t.depth(), 0);
        let p = t.generate_proof(0).unwrap();
        assert!(p.siblings.is_empty() && verify_proof(&p));
        assert_eq!(t.generate_proof(1), Err(Error::IndexOutOfRange(1)));
        assert_eq!(
            proof_from_tree(&t, &Fr::from(6u64)),
            Err(Error::LeafNotFound)
        );
    }
}
