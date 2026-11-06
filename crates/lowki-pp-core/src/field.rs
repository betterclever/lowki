//! BN254 scalar-field helpers.

use ark_ff::{BigInteger, PrimeField};

use crate::{Error, Fr};

/// `SNARK_SCALAR_FIELD` (the BN254 scalar field modulus `p`), big-endian.
pub const SNARK_SCALAR_FIELD: [u8; 32] = [
    0x30, 0x64, 0x4e, 0x72, 0xe1, 0x31, 0xa0, 0x29, 0xb8, 0x50, 0x45, 0xb6, 0x81, 0x81, 0x58, 0x5d,
    0x28, 0x33, 0xe8, 0x48, 0x79, 0xb9, 0x70, 0x91, 0x43, 0xe1, 0xf5, 0x93, 0xf0, 0x00, 0x00, 0x01,
];

/// Interprets 32 big-endian bytes as a uint256 and reduces it mod p.
///
/// This is how every reference treats out-of-range Poseidon inputs (maci-crypto's
/// `Fr.e`, Kohaku T3's `x % p`) and how `keccak256(..) % SNARK_SCALAR_FIELD` works.
pub fn fr_from_be_bytes_mod_order(bytes: &[u8; 32]) -> Fr {
    Fr::from_be_bytes_mod_order(bytes)
}

/// Strict conversion: errors with [`Error::NonCanonicalField`] unless the value is < p.
pub fn fr_from_be_bytes(bytes: &[u8; 32]) -> Result<Fr, Error> {
    if bytes.as_slice() >= SNARK_SCALAR_FIELD.as_slice() {
        return Err(Error::NonCanonicalField);
    }
    Ok(Fr::from_be_bytes_mod_order(bytes))
}

/// Canonical big-endian encoding of a field element.
pub fn fr_to_be_bytes(x: &Fr) -> [u8; 32] {
    let v = x.into_bigint().to_bytes_be();
    let mut out = [0u8; 32];
    out.copy_from_slice(&v);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modulus_matches_ark() {
        let p = Fr::MODULUS.to_bytes_be();
        assert_eq!(p.as_slice(), SNARK_SCALAR_FIELD.as_slice());
        assert_eq!(
            fr_from_be_bytes(&SNARK_SCALAR_FIELD),
            Err(Error::NonCanonicalField)
        );
        assert_eq!(
            fr_from_be_bytes_mod_order(&SNARK_SCALAR_FIELD),
            Fr::from(0u64)
        );
        let mut pm1 = SNARK_SCALAR_FIELD;
        pm1[31] = 0;
        assert_eq!(fr_to_be_bytes(&fr_from_be_bytes(&pm1).unwrap()), pm1);
    }
}
