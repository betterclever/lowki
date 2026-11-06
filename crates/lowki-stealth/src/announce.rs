//! ERC-5564 Announcer calldata / `Announcement` log decoding, announcement metadata
//! layout, and ERC-6538 registry calldata.

use alloc::vec::Vec;

use crate::Error;
use crate::abi::{self, Token, address_from_word, word_address, word_u64};
use crate::keys::{Address, keccak256};
use crate::meta::StealthMetaAddress;

/// `announce(uint256,address,bytes,bytes)`
pub const ANNOUNCE_SELECTOR_SIGNATURE: &str = "announce(uint256,address,bytes,bytes)";
/// `Announcement(uint256,address,address,bytes,bytes)`; schemeId, stealthAddress, caller are indexed.
pub const ANNOUNCEMENT_EVENT_SIGNATURE: &str = "Announcement(uint256,address,address,bytes,bytes)";
const REGISTER_KEYS_SIGNATURE: &str = "registerKeys(uint256,bytes)";
const STEALTH_META_ADDRESS_OF_SIGNATURE: &str = "stealthMetaAddressOf(address,uint256)";

/// EIP-5564 native-ETH function identifier (`0xeeeeeeee`).
pub const ETH_TOKEN_IDENTIFIER: [u8; 4] = [0xee; 4];
/// EIP-5564 native-ETH pseudo-address `0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE`.
pub const ETH_TOKEN_ADDRESS: Address = [0xee; 20];
/// `transfer(address,uint256)`
pub const ERC20_TRANSFER_SELECTOR: [u8; 4] = [0xa9, 0x05, 0x9c, 0xbb];
/// `transferFrom(address,address,uint256)`
pub const ERC721_TRANSFER_FROM_SELECTOR: [u8; 4] = [0x23, 0xb8, 0x72, 0xdd];

/// Parsed 57-byte EIP-5564 metadata: `viewTag(1) || selector(4) || contract(20) || amount(32)`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MetadataFields {
    pub view_tag: u8,
    pub function_identifier: [u8; 4],
    pub contract_address: Address,
    /// Big-endian uint256 (amount, or token id for ERC-721).
    pub amount: [u8; 32],
}

/// Build the 57-byte metadata layout suggested by EIP-5564.
pub fn build_metadata(
    view_tag: u8,
    function_identifier: [u8; 4],
    contract: &Address,
    amount: &[u8; 32],
) -> [u8; 57] {
    let mut m = [0u8; 57];
    m[0] = view_tag;
    m[1..5].copy_from_slice(&function_identifier);
    m[5..25].copy_from_slice(contract);
    m[25..].copy_from_slice(amount);
    m
}

/// Native ETH transfer metadata (`0xeeeeeeee`, `0xEeee…EEeE`, amount in wei).
pub fn metadata_eth(view_tag: u8, amount_wei: &[u8; 32]) -> [u8; 57] {
    build_metadata(
        view_tag,
        ETH_TOKEN_IDENTIFIER,
        &ETH_TOKEN_ADDRESS,
        amount_wei,
    )
}

/// ERC-20 `transfer` metadata.
pub fn metadata_erc20(view_tag: u8, token: &Address, amount: &[u8; 32]) -> [u8; 57] {
    build_metadata(view_tag, ERC20_TRANSFER_SELECTOR, token, amount)
}

/// ERC-721 `transferFrom` metadata.
pub fn metadata_erc721(view_tag: u8, token: &Address, token_id: &[u8; 32]) -> [u8; 57] {
    build_metadata(view_tag, ERC721_TRANSFER_FROM_SELECTOR, token, token_id)
}

/// Parse the 57-byte layout (exact length required).
pub fn parse_metadata(metadata: &[u8]) -> Result<MetadataFields, Error> {
    if metadata.len() != 57 {
        return Err(Error::InvalidMetadata);
    }
    let mut f = MetadataFields {
        view_tag: metadata[0],
        function_identifier: [0; 4],
        contract_address: [0; 20],
        amount: [0; 32],
    };
    f.function_identifier.copy_from_slice(&metadata[1..5]);
    f.contract_address.copy_from_slice(&metadata[5..25]);
    f.amount.copy_from_slice(&metadata[25..]);
    Ok(f)
}

/// The view tag is the first metadata byte (`None` for empty metadata).
pub fn view_tag_from_metadata(metadata: &[u8]) -> Option<u8> {
    metadata.first().copied()
}

/// Calldata for `ERC5564Announcer.announce(schemeId, stealthAddress, ephemeralPubKey, metadata)`.
pub fn encode_announce_calldata(
    scheme_id: u64,
    stealth_address: &Address,
    ephemeral_pubkey: &[u8],
    metadata: &[u8],
) -> Vec<u8> {
    abi::encode_call(
        abi::selector(ANNOUNCE_SELECTOR_SIGNATURE),
        &[
            Token::Static(word_u64(scheme_id)),
            Token::Static(word_address(stealth_address)),
            Token::Bytes(ephemeral_pubkey),
            Token::Bytes(metadata),
        ],
    )
}

/// `topics[0]` of the `Announcement` event.
pub fn announcement_topic0() -> [u8; 32] {
    keccak256(ANNOUNCEMENT_EVENT_SIGNATURE.as_bytes())
}

/// Decoded `Announcement` event.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Announcement {
    /// uint256 schemeId (big-endian).
    pub scheme_id: [u8; 32],
    pub stealth_address: Address,
    pub caller: Address,
    pub ephemeral_pubkey: Vec<u8>,
    pub metadata: Vec<u8>,
}

impl Announcement {
    /// True if `schemeId == 1` (the scheme this crate implements).
    pub fn is_scheme_1(&self) -> bool {
        self.scheme_id == word_u64(crate::SCHEME_ID)
    }

    /// View tag (first metadata byte), if present.
    pub fn view_tag(&self) -> Option<u8> {
        view_tag_from_metadata(&self.metadata)
    }
}

/// Decode an `Announcement` log from its topics and data.
///
/// Requires exactly 4 topics with `topics[0] == keccak256(ANNOUNCEMENT_EVENT_SIGNATURE)` and
/// clean (zero-padded) address topics.
pub fn decode_announcement_log(topics: &[[u8; 32]], data: &[u8]) -> Result<Announcement, Error> {
    if topics.len() != 4 || topics[0] != announcement_topic0() {
        return Err(Error::InvalidLog);
    }
    Ok(Announcement {
        scheme_id: topics[1],
        stealth_address: address_from_word(&topics[2]).map_err(|_| Error::InvalidLog)?,
        caller: address_from_word(&topics[3]).map_err(|_| Error::InvalidLog)?,
        ephemeral_pubkey: abi::decode_bytes(data, 0)?,
        metadata: abi::decode_bytes(data, 1)?,
    })
}

/// Calldata for `ERC6538Registry.registerKeys(schemeId, stealthMetaAddress)` (66-byte raw form).
pub fn encode_register_keys_calldata(scheme_id: u64, meta: &StealthMetaAddress) -> Vec<u8> {
    let raw = meta.to_raw();
    abi::encode_call(
        abi::selector(REGISTER_KEYS_SIGNATURE),
        &[Token::Static(word_u64(scheme_id)), Token::Bytes(&raw)],
    )
}

/// Calldata for `ERC6538Registry.stealthMetaAddressOf(registrant, schemeId)`.
pub fn encode_stealth_meta_address_of_calldata(registrant: &Address, scheme_id: u64) -> Vec<u8> {
    abi::encode_call(
        abi::selector(STEALTH_META_ADDRESS_OF_SIGNATURE),
        &[
            Token::Static(word_address(registrant)),
            Token::Static(word_u64(scheme_id)),
        ],
    )
}

/// Decode `stealthMetaAddressOf` return data (`bytes`). Returns `Ok(None)` when the
/// registrant has not registered (empty bytes).
pub fn decode_stealth_meta_address_of_return(
    data: &[u8],
) -> Result<Option<StealthMetaAddress>, Error> {
    let raw = abi::decode_bytes(data, 0)?;
    if raw.is_empty() {
        return Ok(None);
    }
    StealthMetaAddress::from_raw(&raw).map(Some)
}
