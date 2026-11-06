//! circomlib-compatible Poseidon over BN254 (x^5 S-box, circom round constants /
//! MDS, capacity element 0), via `light-poseidon`'s circom parameters.
//!
//! Verified against maci-crypto 2.5.0 `poseidon` (arity 1, 2, 3) and Kohaku's
//! `@kohaku-eth/poseidon` T3 (arity 2). Inputs are [`Fr`], i.e. already reduced;
//! the references reduce uint256 inputs >= p mod p, which corresponds to
//! [`crate::fr_from_be_bytes_mod_order`].

use std::cell::RefCell;

use light_poseidon::{Poseidon, PoseidonHasher};

use crate::{Error, Fr};

thread_local! {
    static HASHERS: [RefCell<Poseidon<Fr>>; 3] = [
        RefCell::new(Poseidon::<Fr>::new_circom(1).expect("circom params t=2")),
        RefCell::new(Poseidon::<Fr>::new_circom(2).expect("circom params t=3")),
        RefCell::new(Poseidon::<Fr>::new_circom(3).expect("circom params t=4")),
    ];
}

/// Poseidon of 1, 2 or 3 field elements (`maci-crypto` `poseidon(inputs)`).
pub fn poseidon(inputs: &[Fr]) -> Result<Fr, Error> {
    let n = inputs.len();
    if !(1..=3).contains(&n) {
        return Err(Error::InvalidArity(n));
    }
    HASHERS.with(|h| {
        h[n - 1]
            .borrow_mut()
            .hash(inputs)
            .map_err(|_| Error::InvalidArity(n))
    })
}

/// `poseidon([a])` (T2), e.g. the nullifier hash.
pub fn poseidon1(a: Fr) -> Fr {
    poseidon(&[a]).expect("arity 1")
}

/// `poseidon([a, b])` (T3), e.g. Merkle nodes and the precommitment.
pub fn poseidon2(a: Fr, b: Fr) -> Fr {
    poseidon(&[a, b]).expect("arity 2")
}

/// `poseidon([a, b, c])` (T4), e.g. the note commitment.
pub fn poseidon3(a: Fr, b: Fr, c: Fr) -> Fr {
    poseidon(&[a, b, c]).expect("arity 3")
}
