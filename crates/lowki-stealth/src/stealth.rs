use k256::elliptic_curve::PrimeField;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::{NonZeroScalar, ProjectivePoint, PublicKey, Scalar};
use zeroize::{Zeroize, Zeroizing};

use crate::Error;
use crate::keys::{
    Address, SecretKey, address_from_point, keccak256, parse_compressed, parse_pubkey,
};
use crate::meta::StealthMetaAddress;

/// Output of the sender side of ERC-5564 scheme 1.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GeneratedStealthAddress {
    pub stealth_address: Address,
    /// 33-byte compressed ephemeral public key (goes into the announcement).
    pub ephemeral_pubkey: [u8; 33],
    /// First byte of keccak256(compressed shared secret).
    pub view_tag: u8,
}

/// `keccak256(compressed(priv · pub))`, zeroized on drop.
fn hashed_shared_secret(priv_key: &SecretKey, pub_key: &PublicKey) -> Zeroizing<[u8; 32]> {
    let point = (pub_key.to_projective() * *priv_key.scalar()).to_affine();
    let mut enc = point.to_encoded_point(true);
    let h = Zeroizing::new(keccak256(enc.as_bytes()));
    enc.zeroize();
    h
}

/// Interpret the hash as a scalar without reduction; rejects 0 and >= n (as the reference does).
fn hash_scalar(h: &[u8; 32]) -> Result<NonZeroScalar, Error> {
    let s: Option<Scalar> = Scalar::from_repr((*h).into()).into();
    let s = s.ok_or(Error::InvalidHashScalar)?;
    Option::<NonZeroScalar>::from(NonZeroScalar::new(s)).ok_or(Error::InvalidHashScalar)
}

fn stealth_point(spending_pub: &PublicKey, s_h: &NonZeroScalar) -> Result<ProjectivePoint, Error> {
    let p = spending_pub.to_projective() + ProjectivePoint::GENERATOR * **s_h;
    if p == ProjectivePoint::IDENTITY {
        return Err(Error::PointAtInfinity);
    }
    Ok(p)
}

/// Sender side: derive a one-time stealth address for `meta` using `ephemeral_priv`.
pub fn generate_stealth_address(
    meta: &StealthMetaAddress,
    ephemeral_priv: &SecretKey,
) -> Result<GeneratedStealthAddress, Error> {
    let viewing = parse_compressed(meta.viewing_pubkey())?;
    let spending = parse_compressed(meta.spending_pubkey())?;
    let h = hashed_shared_secret(ephemeral_priv, &viewing);
    let s_h = hash_scalar(&h)?;
    let p = stealth_point(&spending, &s_h)?;
    Ok(GeneratedStealthAddress {
        stealth_address: address_from_point(&p),
        ephemeral_pubkey: ephemeral_priv.public_key(),
        view_tag: h[0],
    })
}

/// Recipient side: returns the stealth address if this announcement is for us.
///
/// Fast-rejects (one ECDH + one keccak) when the view tag does not match.
/// `ephemeral_pub` may be 33-byte compressed or 65-byte uncompressed SEC1; any
/// malformed input yields `None`.
///
/// Note: a matching view tag only means a 1/256 false-positive filter passed. The
/// returned address is the address the sender *would* have paid if the announcement
/// is for us; callers must still compare it to the announced `stealthAddress`.
pub fn check_announcement(
    viewing_priv: &SecretKey,
    spending_pub: &[u8; 33],
    ephemeral_pub: &[u8],
    view_tag: u8,
) -> Option<Address> {
    let eph = parse_pubkey(ephemeral_pub).ok()?;
    let h = hashed_shared_secret(viewing_priv, &eph);
    if h[0] != view_tag {
        return None;
    }
    let spending = parse_compressed(spending_pub).ok()?;
    let s_h = hash_scalar(&h).ok()?;
    let p = stealth_point(&spending, &s_h).ok()?;
    Some(address_from_point(&p))
}

/// Recipient side: the private key controlling the stealth address,
/// `(spending_priv + keccak256(compressed(viewing_priv · P_eph))) mod n`.
pub fn compute_stealth_private_key(
    spending_priv: &SecretKey,
    viewing_priv: &SecretKey,
    ephemeral_pub: &[u8],
) -> Result<SecretKey, Error> {
    let eph = parse_pubkey(ephemeral_pub)?;
    let h = hashed_shared_secret(viewing_priv, &eph);
    let s_h = hash_scalar(&h)?;
    let k = *spending_priv.scalar() + *s_h;
    let k = Option::<NonZeroScalar>::from(NonZeroScalar::new(k)).ok_or(Error::PointAtInfinity)?;
    Ok(SecretKey::from_scalar(k))
}
