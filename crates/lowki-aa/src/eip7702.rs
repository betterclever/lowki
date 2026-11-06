//! EIP-7702 set-code authorizations.

use alloy_primitives::{Address, B256, U256, keccak256};
use alloy_rlp::{Encodable, Header};
use serde::{Deserialize, Serialize};

use crate::Error;
use crate::hex_quantity;
use crate::signer::EphemeralSigner;

/// `MAGIC` prefix of the authorization signing payload.
pub const AUTHORIZATION_MAGIC: u8 = 0x05;

/// An unsigned authorization `[chain_id, address, nonce]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Authorization {
    /// 0 = valid on any chain.
    pub chain_id: U256,
    /// Delegate (implementation) address.
    pub address: Address,
    /// The authority EOA's transaction count at inclusion time.
    pub nonce: u64,
}

impl Authorization {
    /// `rlp([chain_id, address, nonce])`.
    pub fn rlp(&self) -> Vec<u8> {
        let payload_length = self.chain_id.length() + self.address.length() + self.nonce.length();
        let mut out = Vec::with_capacity(payload_length + 4);
        Header {
            list: true,
            payload_length,
        }
        .encode(&mut out);
        self.chain_id.encode(&mut out);
        self.address.encode(&mut out);
        self.nonce.encode(&mut out);
        out
    }

    /// `keccak256(0x05 || rlp([chain_id, address, nonce]))` (viem `hashAuthorization`).
    pub fn signing_hash(&self) -> B256 {
        let mut buf = vec![AUTHORIZATION_MAGIC];
        buf.extend_from_slice(&self.rlp());
        keccak256(&buf)
    }

    /// Sign with the authority key (viem `signAuthorization`).
    pub fn sign(&self, signer: &EphemeralSigner) -> Result<SignedAuthorization, Error> {
        let sig = signer.sign_hash(&self.signing_hash())?;
        Ok(SignedAuthorization {
            inner: *self,
            y_parity: sig.y_parity,
            r: sig.r,
            s: sig.s,
        })
    }
}

/// Free-function form of [`Authorization::signing_hash`].
pub fn authorization_signing_hash(chain_id: U256, address: Address, nonce: u64) -> B256 {
    Authorization {
        chain_id,
        address,
        nonce,
    }
    .signing_hash()
}

/// A signed authorization tuple `[chain_id, address, nonce, y_parity, r, s]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignedAuthorization {
    pub inner: Authorization,
    pub y_parity: u8,
    pub r: B256,
    pub s: B256,
}

impl SignedAuthorization {
    /// Recover the authority (signer) address.
    pub fn recover_authority(&self) -> Option<Address> {
        crate::signer::RecoverableSignature {
            r: self.r,
            s: self.s,
            y_parity: self.y_parity,
        }
        .recover(&self.inner.signing_hash())
    }

    /// The `eip7702Auth` object bundlers accept alongside a userOp (Kohaku `serializeAuth`).
    pub fn to_serialized(&self) -> SerializedAuth {
        SerializedAuth {
            address: self.inner.address.to_checksum(None),
            chain_id: hex_quantity(self.inner.chain_id),
            nonce: hex_quantity(self.inner.nonce),
            r: self.r.to_string(),
            s: self.s.to_string(),
            y_parity: hex_quantity(self.y_parity),
        }
    }
}

/// `eip7702Auth` JSON shape (`SerializedAuth` in Kohaku): hex quantities, checksummed address.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedAuth {
    pub address: String,
    pub chain_id: String,
    pub nonce: String,
    pub r: String,
    pub s: String,
    pub y_parity: String,
}
