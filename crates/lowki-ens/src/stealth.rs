//! ENSIP-30 (draft, ensdomains/ensips PR #88) stealth address resolution, as implemented by
//! the reference `StealthResolver` and `Scheme1Verifier` (vendor `stealth-ens-resolver`).
//!
//! * Section 1 record: key `stealth-meta-address[<scheme>][<coinType>]` (ENSIP-24 `data()`),
//!   value `flag || meta-address` (67 bytes for scheme 1; flag bit 0 = client-side derivation
//!   allowed, bits 1..7 reserved and zero).
//! * Gateway response (`StealthResolver.resolveStealthCallback`):
//!   `abi.encode(address stealthAddress, bytes verificationData)`.
//! * Scheme-1 verification data (`Scheme1Verifier`): `e || S.x || S.y` (96 bytes), `e` the
//!   ephemeral private key, `S = e·P_view`. It contains the ephemeral **private** key, so it is
//!   handled as a secret here (`Zeroizing`, no `Debug`).

use alloc::string::String;
use alloc::vec::Vec;

use k256::elliptic_curve::ff::PrimeField;
use k256::elliptic_curve::ops::Reduce;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::{AffinePoint, ProjectivePoint, PublicKey, Scalar, U256 as KU256};
use zeroize::Zeroizing;

use crate::abi::{self, Token};
use crate::{Address, Error, SecretKey, StealthMetaAddress, keccak256};

/// ERC-5564 scheme 1 (secp256k1 with view tags).
pub const SCHEME_ID: u64 = 1;
/// Scheme-1 record length: flag byte + 66-byte meta-address.
pub const SCHEME1_RECORD_LEN: usize = 67;

/// ENSIP-30 Section 1 key: `stealth-meta-address[<scheme>][<coinType>]` (decimal, no leading
/// zeros).
pub fn stealth_data_key(scheme_id: u64, coin_type: u64) -> String {
    alloc::format!("stealth-meta-address[{scheme_id}][{coin_type}]")
}

/// The older draft's text-record key `stealth-meta-address[<scheme>]`, whose value is the
/// ERC-5564 URI `st:eth:0x<spend><view>` ([`StealthMetaAddress::to_uri`]). Compatibility only.
pub fn stealth_text_key(scheme_id: u64) -> String {
    alloc::format!("stealth-meta-address[{scheme_id}]")
}

/// Parses a canonical Section 1 key into `(scheme_id, coin_type)`; non-canonical spellings
/// (`[01]`, `[0x1]`, `[+1]`, overflow) are `None`, as in `StealthResolver._parseStealthKey`.
pub fn parse_stealth_data_key(key: &str) -> Option<(u64, u64)> {
    let rest = key.strip_prefix("stealth-meta-address[")?;
    let (scheme, rest) = rest.split_once("][")?;
    let coin = rest.strip_suffix(']')?;
    let parse = |s: &str| -> Option<u64> {
        if s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let v: u64 = s.parse().ok()?;
        (alloc::format!("{v}") == s).then_some(v)
    };
    Some((parse(scheme)?, parse(coin)?))
}

/// A decoded Section 1 record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StealthRecord {
    pub client_side_allowed: bool,
    pub meta: StealthMetaAddress,
}

/// `flag || spend(33) || view(33)`.
pub fn encode_stealth_record(client_side_allowed: bool, meta: &StealthMetaAddress) -> Vec<u8> {
    let mut v = Vec::with_capacity(SCHEME1_RECORD_LEN);
    v.push(client_side_allowed as u8);
    v.extend_from_slice(&meta.to_raw());
    v
}

/// Decodes a scheme-1 record with the ENSIP-30 failure semantics: exactly 67 bytes, reserved
/// flag bits zero, both keys valid compressed secp256k1 points. Anything else is
/// [`Error::InvalidStealthRecord`] (never a partial result). An empty value means "no record"
/// and is also an error here.
pub fn decode_stealth_record(value: &[u8]) -> Result<StealthRecord, Error> {
    if value.len() != SCHEME1_RECORD_LEN || value[0] & 0xfe != 0 {
        return Err(Error::InvalidStealthRecord);
    }
    let spend: [u8; 33] = value[1..34].try_into().expect("33");
    let view: [u8; 33] = value[34..67].try_into().expect("33");
    let meta = StealthMetaAddress::new(&spend, &view).map_err(|_| Error::InvalidStealthRecord)?;
    Ok(StealthRecord {
        client_side_allowed: value[0] & 1 == 1,
        meta,
    })
}

// ---------------------------------------------------------------- StealthResolver calldata

pub const SET_CONFIG_SIG: &str = "setConfig(bytes,address,string[])";
pub const SET_CHAINS_SIG: &str = "setChains(bytes,uint256[],bool)";
pub const SET_META_ADDRESS_SIG: &str = "setMetaAddress(bytes,bytes)";
pub const CLEAR_RECORDS_SIG: &str = "clearRecords(bytes)";
pub const CAN_SET_META_ADDRESS_SIG: &str = "canSetMetaAddress(bytes,address)";
pub const CAN_SET_CONFIG_SIG: &str = "canSetConfig(bytes,address)";
pub const RESOLVE_STEALTH_CALLBACK_SIG: &str = "resolveStealthCallback(bytes,bytes)";

/// `setConfig(bytes name, address recordsResolver, string[] urls)`
pub fn encode_set_config<S: AsRef<str>>(
    dns_name: &[u8],
    records_resolver: &Address,
    urls: &[S],
) -> Vec<u8> {
    abi::encode_sig(
        SET_CONFIG_SIG,
        &[
            Token::bytes(dns_name),
            Token::address(records_resolver),
            Token::string_array(urls),
        ],
    )
}

/// `setChains(bytes name, uint256[] coinTypes, bool enabled)`
pub fn encode_set_chains(dns_name: &[u8], coin_types: &[u64], enabled: bool) -> Vec<u8> {
    abi::encode_sig(
        SET_CHAINS_SIG,
        &[
            Token::bytes(dns_name),
            Token::Array(coin_types.iter().map(|&c| Token::uint(c)).collect()),
            Token::bool(enabled),
        ],
    )
}

/// `setMetaAddress(bytes name, bytes value)` (`value` = [`encode_stealth_record`], or empty to
/// clear).
pub fn encode_set_meta_address(dns_name: &[u8], value: &[u8]) -> Vec<u8> {
    abi::encode_sig(
        SET_META_ADDRESS_SIG,
        &[Token::bytes(dns_name), Token::bytes(value)],
    )
}

/// `clearRecords(bytes name)`
pub fn encode_clear_records(dns_name: &[u8]) -> Vec<u8> {
    abi::encode_sig(CLEAR_RECORDS_SIG, &[Token::bytes(dns_name)])
}

/// `canSetMetaAddress(bytes name, address account)`
pub fn encode_can_set_meta_address(dns_name: &[u8], account: &Address) -> Vec<u8> {
    abi::encode_sig(
        CAN_SET_META_ADDRESS_SIG,
        &[Token::bytes(dns_name), Token::address(account)],
    )
}

/// `canSetConfig(bytes name, address account)`
pub fn encode_can_set_config(dns_name: &[u8], account: &Address) -> Vec<u8> {
    abi::encode_sig(
        CAN_SET_CONFIG_SIG,
        &[Token::bytes(dns_name), Token::address(account)],
    )
}

// ---------------------------------------------------------------- gateway formats

/// Gateway response: `abi.encode(address stealthAddress, bytes verificationData)`.
pub fn encode_gateway_response(stealth: &Address, verification_data: &[u8]) -> Vec<u8> {
    abi::encode(&[Token::address(stealth), Token::bytes(verification_data)])
}

/// Decodes a gateway response into `(stealthAddress, verificationData)`.
pub fn decode_gateway_response(data: &[u8]) -> Result<(Address, Zeroizing<Vec<u8>>), Error> {
    let r = abi::Reader::new(data);
    Ok((r.address(0)?, Zeroizing::new(r.bytes(1)?.to_vec())))
}

fn uncompressed_xy(p: &AffinePoint) -> [u8; 64] {
    let ep = p.to_encoded_point(false);
    let mut out = [0u8; 64];
    out.copy_from_slice(&ep.as_bytes()[1..65]);
    out
}

fn parse_compressed(k: &[u8; 33]) -> Result<ProjectivePoint, Error> {
    PublicKey::from_sec1_bytes(k)
        .map(|p| p.to_projective())
        .map_err(|_| Error::MalformedMetaAddress)
}

fn point_address(xy: &[u8]) -> Address {
    let h = keccak256(xy);
    let mut a = [0u8; 20];
    a.copy_from_slice(&h[12..]);
    a
}

/// Scheme-1 verification data `e || S.x || S.y` with `S = e·P_view` (affine coordinates).
pub fn scheme1_verification_data(
    ephemeral: &SecretKey,
    meta: &StealthMetaAddress,
) -> Result<Zeroizing<[u8; 96]>, Error> {
    let view = parse_compressed(meta.viewing_pubkey())?;
    let e_bytes = ephemeral.to_bytes();
    let e = Zeroizing::new(
        Option::<Scalar>::from(Scalar::from_repr((*e_bytes).into()))
            .ok_or(Error::InvalidPrivateKey)?,
    );
    let s = (view * *e).to_affine();
    let mut out = Zeroizing::new([0u8; 96]);
    out[..32].copy_from_slice(&e_bytes[..]);
    out[32..].copy_from_slice(&uncompressed_xy(&s));
    Ok(out)
}

/// What a gateway returns for one stealth resolution.
pub struct GatewayDerivation {
    pub stealth_address: Address,
    /// For the ERC-5564 announcement.
    pub ephemeral_pubkey: [u8; 33],
    pub view_tag: u8,
    /// `e || S.x || S.y` (secret: contains the ephemeral private key).
    pub verification_data: Zeroizing<[u8; 96]>,
}

impl core::fmt::Debug for GatewayDerivation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GatewayDerivation")
            .field(
                "stealth_address",
                &crate::hex::encode(&self.stealth_address),
            )
            .field(
                "ephemeral_pubkey",
                &crate::hex::encode(&self.ephemeral_pubkey),
            )
            .field("view_tag", &self.view_tag)
            .field("verification_data", &"<redacted>")
            .finish()
    }
}

impl GatewayDerivation {
    /// The gateway response bytes `abi.encode(stealthAddress, verificationData)`.
    pub fn gateway_response(&self) -> Vec<u8> {
        encode_gateway_response(&self.stealth_address, &self.verification_data[..])
    }
}

/// Gateway side: derive a stealth address for `meta` with the caller's random `ephemeral` key
/// (lowki-stealth `generate_stealth_address`) and the matching verification data.
pub fn gateway_derive(
    meta: &StealthMetaAddress,
    ephemeral: &SecretKey,
) -> Result<GatewayDerivation, Error> {
    let g = crate::generate_stealth_address(meta, ephemeral)?;
    let vd = scheme1_verification_data(ephemeral, meta)?;
    Ok(GatewayDerivation {
        stealth_address: g.stealth_address,
        ephemeral_pubkey: g.ephemeral_pubkey,
        view_tag: g.view_tag,
        verification_data: vd,
    })
}

/// secp256k1 group order, big-endian.
const N_BE: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe,
    0xba, 0xae, 0xdc, 0xe6, 0xaf, 0x48, 0xa0, 0x3b, 0xbf, 0xd2, 0x5e, 0x8c, 0xd0, 0x36, 0x41, 0x41,
];

/// `LibEcrecover.compressedKey`: prefix 2/3 and `0 < x < n`.
fn compressed_key_ok(k: &[u8; 33]) -> bool {
    (k[0] == 2 || k[0] == 3) && k[1..].iter().any(|&b| b != 0) && k[1..] < N_BE[..]
}

/// Rust re-implementation of `Scheme1Verifier.verify(node, coinType, metaAddress,
/// verificationData, claimed)`.
///
/// * empty `meta_address`: unverified mode, returns `claimed`;
/// * `meta_address` must be 66 bytes of two compressed keys with `0 < x < n`
///   (`MalformedMetaAddress`), `verification_data` 96 bytes with `0 < e < n`
///   (`InvalidVerificationData`);
/// * `address(S.x||S.y)` must equal `address(e·P_view)` (`InvalidSharedSecret`);
/// * `s_h = keccak256((2 + (S.y & 1)) || S.x) mod n` (**reduced**, unlike lowki-stealth and the
///   ScopeLift SDK, which reject `s_h >= n`; they differ with probability about 2⁻¹²⁸);
/// * returns `address(P_spend + s_h·G)` (`MalformedMetaAddress` if it is the identity).
pub fn scheme1_verify(
    meta_address: &[u8],
    verification_data: &[u8],
    claimed: &Address,
) -> Result<Address, Error> {
    if meta_address.is_empty() {
        return Ok(*claimed);
    }
    if meta_address.len() != 66 {
        return Err(Error::MalformedMetaAddress);
    }
    let spend: [u8; 33] = meta_address[..33].try_into().expect("33");
    let view: [u8; 33] = meta_address[33..].try_into().expect("33");
    if !compressed_key_ok(&spend) || !compressed_key_ok(&view) {
        return Err(Error::MalformedMetaAddress);
    }
    if verification_data.len() != 96 {
        return Err(Error::InvalidVerificationData);
    }
    let e_bytes: [u8; 32] = verification_data[..32].try_into().expect("32");
    let e = Option::<Scalar>::from(Scalar::from_repr(e_bytes.into()))
        .ok_or(Error::InvalidVerificationData)?;
    if bool::from(e.is_zero()) {
        return Err(Error::InvalidVerificationData);
    }
    let view_p = parse_compressed(&view)?;
    let shared = uncompressed_xy(&(view_p * e).to_affine());
    if point_address(&shared) != point_address(&verification_data[32..96]) {
        return Err(Error::InvalidSharedSecret);
    }
    let mut pre = [0u8; 33];
    pre[0] = 2 + (verification_data[95] & 1);
    pre[1..].copy_from_slice(&verification_data[32..64]);
    let sh = <Scalar as Reduce<KU256>>::reduce_bytes(&keccak256(&pre).into());
    let spend_p = parse_compressed(&spend)?;
    let p = spend_p + ProjectivePoint::GENERATOR * sh;
    if p == ProjectivePoint::IDENTITY {
        return Err(Error::MalformedMetaAddress);
    }
    Ok(point_address(&uncompressed_xy(&p.to_affine())))
}

// ---------------------------------------------------------------- gateway request parsing

/// Decodes `resolve(bytes name, bytes data)` calldata (the `OffchainLookup.callData` a
/// `StealthResolver` sends) into `(dns_name, inner_data)`.
pub fn decode_resolve_calldata(calldata: &[u8]) -> Result<(Vec<u8>, Vec<u8>), Error> {
    let r = abi::expect_selector(calldata, crate::universal_resolver::RESOLVE_SELECTOR)?;
    Ok((r.bytes(0)?.to_vec(), r.bytes(1)?.to_vec()))
}

/// An address query inside `resolve()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddrQuery {
    pub node: [u8; 32],
    /// 60 for the legacy `addr(bytes32)`.
    pub coin_type: crate::U256,
    /// `addr(bytes32)` (returns `address`) rather than `addr(bytes32,uint256)` (`bytes`).
    pub legacy: bool,
}

impl AddrQuery {
    /// The coin type as `u64` if it fits.
    pub fn coin_type_u64(&self) -> Option<u64> {
        abi::word_to_u64(&self.coin_type).ok()
    }
}

/// Parses `addr(bytes32)` / `addr(bytes32,uint256)` calldata; `None` for anything else.
pub fn parse_addr_query(inner: &[u8]) -> Option<AddrQuery> {
    let (sel, r) = abi::split_selector(inner).ok()?;
    if sel == crate::resolver::ADDR_SELECTOR {
        Some(AddrQuery {
            node: r.b32(0).ok()?,
            coin_type: abi::u256_from_u64(60),
            legacy: true,
        })
    } else if sel == crate::resolver::ADDR_COIN_SELECTOR {
        Some(AddrQuery {
            node: r.b32(0).ok()?,
            coin_type: r.u256(1).ok()?,
            legacy: false,
        })
    } else {
        None
    }
}

// ---------------------------------------------------------------- records-endpoint auth

/// The message a recipient signs with its **viewing** key to fetch its records from the
/// gateway: `"Lowki stealth records\nname: {name}\ntimestamp: {timestamp}"`.
pub fn records_auth_message(name: &str, timestamp: u64) -> String {
    alloc::format!("Lowki stealth records\nname: {name}\ntimestamp: {timestamp}")
}

/// EIP-191 `personal_sign` (viem `signMessage`).
pub fn sign_personal(key: &SecretKey, msg: &str) -> Result<[u8; 65], Error> {
    crate::sig::sign_personal(key, msg.as_bytes())
}

/// Recovers the `personal_sign` signer.
pub fn recover_personal(msg: &str, sig: &[u8]) -> Result<Address, Error> {
    crate::sig::recover_personal(msg.as_bytes(), sig)
}
