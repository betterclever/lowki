//! The worker's message protocol: one `Request` in, one `Response` out.
//!
//! Wire format (JS ⇄ worker): plain objects tagged by `type`, camelCase fields.
//! Secret-bearing byte fields (`prfOutput`, shares, wrapped blobs) are `Uint8Array` so
//! the caller can zero them after use; public values (addresses, hashes, signatures,
//! quantities) are hex/decimal strings. Quantities accept decimal or `0x` hex strings.
//!
//! `Debug` on every type here redacts secret fields (`SecretBytes`, mnemonics).

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::error::WorkerError;

/// Bytes that are secret (PRF outputs, shares, wrapped blobs, mnemonics' entropy).
/// Zeroized on drop; `Debug` prints only the length; (de)serialized as a byte buffer
/// (`Uint8Array` across wasm-bindgen).
#[derive(Clone, Default, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    pub fn new(v: Vec<u8>) -> Self {
        Self(v)
    }
    pub fn from_slice(s: &[u8]) -> Self {
        Self(s.to_vec())
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Copies into a fixed array (zeroized on drop) or fails with a length error.
    pub fn to_array<const N: usize>(&self, what: &str) -> Result<Zeroizing<[u8; N]>, WorkerError> {
        if self.0.len() != N {
            return Err(WorkerError::invalid(format!("{what} must be {N} bytes, got {}", self.0.len())));
        }
        let mut out = Zeroizing::new([0u8; N]);
        out.copy_from_slice(&self.0);
        Ok(out)
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretBytes(len={})", self.0.len())
    }
}

impl Serialize for SecretBytes {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for SecretBytes {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let buf = serde_bytes::ByteBuf::deserialize(d)?;
        Ok(SecretBytes(buf.into_vec()))
    }
}

/// A secret string (mnemonic). Zeroized on drop, redacted in `Debug`.
#[derive(Clone, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(..)")
    }
}

/// Merkle inclusion proof as Kohaku/zk-kit produce it (compressed `index`, siblings
/// padded to 32 by `proofFromTree`). Public data.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MerkleProofIn {
    pub root: String,
    pub index: String,
    pub siblings: Vec<String>,
}

/// `Withdrawal { processooor, data }` (the struct the pool hashes into `context`).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WithdrawalIn {
    pub processooor: String,
    pub data: String,
}

/// Either a precomputed `context` or the withdrawal + pool scope to compute it from.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextIn {
    pub context: Option<String>,
    pub withdrawal: Option<WithdrawalIn>,
    pub scope: Option<String>,
}

/// Note coordinates: which Kohaku `SignatureSecretManager` note (all public indices).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteRef {
    #[serde(default)]
    pub account_index: u32,
    pub chain_id: String,
    pub entrypoint: String,
    pub deposit_index: u64,
    /// 0 = the deposit note; k = the note after k withdrawals.
    #[serde(default)]
    pub secret_index: u64,
}

/// An EIP-1559 (type 2) or EIP-7702 (type 4) transaction to sign.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxIn {
    /// "eip1559" or "eip7702".
    #[serde(rename = "type")]
    pub kind: String,
    pub chain_id: String,
    pub nonce: String,
    pub max_priority_fee_per_gas: String,
    pub max_fee_per_gas: String,
    pub gas: String,
    /// Required for eip7702; absent = contract creation (eip1559 only).
    pub to: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default)]
    pub access_list: Vec<AccessListItemIn>,
    /// Signed authorizations (eip7702 only).
    #[serde(default)]
    pub authorization_list: Vec<SignedAuthorizationIn>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessListItemIn {
    pub address: String,
    pub storage_keys: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedAuthorizationIn {
    pub chain_id: String,
    pub address: String,
    pub nonce: String,
    pub y_parity: u8,
    pub r: String,
    pub s: String,
}

/// The delegation grant the app asks for (ARCHITECTURE.md "Delegation").
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantIn {
    pub signer_id: String,
    pub policy_ids: Vec<String>,
    /// Unix seconds.
    pub expiry: u64,
    /// 32-byte hex nonce; the worker generates one when absent.
    pub nonce: Option<String>,
}

/// Gas limits for the paymaster userOp (decimal/hex strings).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GasIn {
    pub call_gas_limit: String,
    pub verification_gas_limit: String,
    pub pre_verification_gas: String,
    pub paymaster_verification_gas_limit: String,
    pub paymaster_post_op_gas_limit: String,
}

/// Solidity-shaped proof (`{pA, pB, pC, pubSignals}`, decimal strings, pB limbs swapped).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SolidityProofIn {
    #[serde(rename = "pA")]
    pub p_a: [String; 2],
    #[serde(rename = "pB")]
    pub p_b: [[String; 2]; 2],
    #[serde(rename = "pC")]
    pub p_c: [String; 2],
    #[serde(rename = "pubSignals")]
    pub pub_signals: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[allow(clippy::large_enum_variant)] // one request at a time; boxing would only add noise
pub enum Request {
    // ---- session ----------------------------------------------------------------------
    /// New random wallet. Returns the PRF-wrapped blob and the 2-of-3 shares; unlocks.
    CreateWallet { prf_output: SecretBytes },
    /// Import a 24-word BIP-39 phrase (e.g. Privy export / tests). Same output as create.
    ImportMnemonic { mnemonic: SecretString, prf_output: SecretBytes },
    /// Daily unlock: PRF output + wrapped blob.
    Unlock { prf_output: SecretBytes, wrapped_blob: SecretBytes },
    /// New device without the passkey: server share S + recovery share R.
    Recover { server_share: SecretBytes, recovery_share: SecretBytes },
    /// Unlock with the passkey-wrapped P share plus S or R.
    RecoverWithPasskeyShare { prf_output: SecretBytes, wrapped_passkey_share: SecretBytes, other_share: SecretBytes },
    /// After `recover`: wrap the unlocked seed for a new passkey. With `passkeyShareX`
    /// (from `walletCreated`) it also re-derives and wraps the same P share.
    Rewrap { prf_output: SecretBytes, passkey_share_x: Option<u8>, server_share: Option<SecretBytes>, recovery_share: Option<SecretBytes> },
    Lock,
    Status,

    // ---- accounts -----------------------------------------------------------------------
    AppAccount { origin: String },
    StealthMetaAddress,

    // ---- signing (app account of `origin`) ----------------------------------------------
    /// EIP-191 personal_sign over raw bytes (hex) or UTF-8 text.
    SignMessage { origin: String, message: Option<String>, message_hex: Option<String> },
    /// EIP-712 typed data (`{types, primaryType, domain, message}`, as viem/ethers send it).
    SignTypedData { origin: String, typed_data: serde_json::Value },
    SignTransaction { origin: String, tx: TxIn },
    /// EIP-7702 authorization signed by the app account (for its own type-4 delegation).
    SignAuthorization { origin: String, chain_id: String, address: String, nonce: String },

    // ---- privacy pools ------------------------------------------------------------------
    /// Precommitment/nullifier hash/commitment for a note (all public hashes).
    PpNote { note: NoteRef, value: Option<String>, label: Option<String> },
    /// Shield (deposit) transaction for the deposit note `note` (secret_index must be 0).
    PpShield { note: NoteRef, value: String },
    /// Load circuit artifacts (bytes the iframe fetched and pinned). Tries WebGPU if asked.
    PpLoadCircuits {
        withdraw_wasm: Option<serde_bytes::ByteBuf>,
        withdraw_zkey: Option<serde_bytes::ByteBuf>,
        commitment_wasm: Option<serde_bytes::ByteBuf>,
        commitment_zkey: Option<serde_bytes::ByteBuf>,
        #[serde(default)]
        use_gpu: bool,
    },
    /// Withdraw proof for the note at `note` (existing note = secret_index; change note =
    /// secret_index + 1). Secrets are derived inside the worker.
    PpProveWithdraw {
        note: NoteRef,
        label: String,
        existing_value: String,
        withdrawn_value: String,
        context: ContextIn,
        state_proof: MerkleProofIn,
        asp_proof: MerkleProofIn,
    },
    /// Commitment (ragequit) proof for a note.
    PpProveCommitment { note: NoteRef, value: String, label: String },
    /// Signed ERC-4337 v0.8 userOp for a paymaster-sponsored withdrawal (Kohaku flow).
    /// The sender is the note's ephemeral signer `(depositIndex, secretIndex)`.
    PpBuildPaymasterWithdraw {
        note: NoteRef,
        withdrawal: WithdrawalIn,
        proof: SolidityProofIn,
        pool: String,
        paymaster: String,
        adapter: String,
        gas: GasIn,
        max_fee_per_gas: String,
        max_priority_fee_per_gas: String,
        nonce: String,
        authorization_nonce: Option<u64>,
    },

    // ---- Kohaku host ---------------------------------------------------------------------
    /// Kohaku `Keystore.deriveAt`, scoped to `m/28784'/1'/…` + allowed paths.
    KohakuDeriveAt { path: String },
    KohakuAllowPath { path: String },
    /// Kohaku `EnvelopeSigner.signEnvelope` for the PrivacyPoolNote envelope.
    PpSignNoteEnvelope { account_index: u32, chain_id: String, entrypoint: String, deposit_index: u64, secret_index: u64 },

    // ---- delegation ----------------------------------------------------------------------
    SealAppKey { origin: String, enclave_public_key: String, grant: GrantIn },

    // ---- ENS name + stealth payments (src/ens.rs) -------------------------------------------
    /// The name-owner identity key `m/137'/0'/{index}'` (address only).
    EnsIdentity { #[serde(default)] index: u32 },
    /// EIP-712 `Register` for `LowkiRegistrar`, signed by the identity key (owner = its address).
    EnsSignRegistration {
        chain_id: String,
        registrar: String,
        label: String,
        #[serde(default)]
        resolver: Option<String>,
        duration: u64,
        nonce: String,
        deadline: u64,
        #[serde(default)]
        index: u32,
    },
    /// The sponsored owner batch: EIP-7702 authorisation + EIP-712 `Execute` over the setup calls.
    EnsSignOwnerSetup {
        chain_id: String,
        name: String,
        owner_account: String,
        auth_nonce: String,
        exec_nonce: String,
        deadline: u64,
        factory: String,
        permissioned_resolver_impl: String,
        stealth_resolver: String,
        registry: String,
        gateway_url: String,
        records_resolver: String,
        #[serde(default)]
        index: u32,
    },
    /// `personal_sign` by the stealth viewing key over `records_auth_message(name, timestamp)`.
    EnsRecordsAuth { name: String, timestamp: u64 },
    /// Which gateway records belong to this wallet (viewing key; view tag, then the address).
    StealthScan { records: Vec<StealthRecordIn> },
    /// Signs a transaction from a found stealth address. The key is derived and used here only.
    StealthSignTransaction { ephemeral_pubkey: String, stealth_address: String, tx: TxIn },
    /// Encrypts the user's app list (UTF-8, normally JSON) under the seed's app-list key.
    AppListSeal { data: String },
    AppListOpen { blob: String },
}

/// One gateway record (`GET /v1/ens/records`), as the server returns it.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StealthRecordIn {
    pub stealth_address: String,
    pub ephemeral_pubkey: String,
    pub view_tag: u8,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StealthFound {
    pub stealth_address: String,
    pub ephemeral_pubkey: String,
}

impl Request {
    /// Name for logs (no field values).
    pub fn kind(&self) -> &'static str {
        match self {
            Request::CreateWallet { .. } => "createWallet",
            Request::ImportMnemonic { .. } => "importMnemonic",
            Request::Unlock { .. } => "unlock",
            Request::Recover { .. } => "recover",
            Request::RecoverWithPasskeyShare { .. } => "recoverWithPasskeyShare",
            Request::Rewrap { .. } => "rewrap",
            Request::Lock => "lock",
            Request::Status => "status",
            Request::AppAccount { .. } => "appAccount",
            Request::StealthMetaAddress => "stealthMetaAddress",
            Request::SignMessage { .. } => "signMessage",
            Request::SignTypedData { .. } => "signTypedData",
            Request::SignTransaction { .. } => "signTransaction",
            Request::SignAuthorization { .. } => "signAuthorization",
            Request::PpNote { .. } => "ppNote",
            Request::PpShield { .. } => "ppShield",
            Request::PpLoadCircuits { .. } => "ppLoadCircuits",
            Request::PpProveWithdraw { .. } => "ppProveWithdraw",
            Request::PpProveCommitment { .. } => "ppProveCommitment",
            Request::PpBuildPaymasterWithdraw { .. } => "ppBuildPaymasterWithdraw",
            Request::KohakuDeriveAt { .. } => "kohakuDeriveAt",
            Request::KohakuAllowPath { .. } => "kohakuAllowPath",
            Request::PpSignNoteEnvelope { .. } => "ppSignNoteEnvelope",
            Request::SealAppKey { .. } => "sealAppKey",
            Request::EnsIdentity { .. } => "ensIdentity",
            Request::EnsSignRegistration { .. } => "ensSignRegistration",
            Request::EnsSignOwnerSetup { .. } => "ensSignOwnerSetup",
            Request::EnsRecordsAuth { .. } => "ensRecordsAuth",
            Request::StealthScan { .. } => "stealthScan",
            Request::StealthSignTransaction { .. } => "stealthSignTransaction",
            Request::AppListSeal { .. } => "appListSeal",
            Request::AppListOpen { .. } => "appListOpen",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareXs {
    pub passkey: u8,
    pub server: u8,
    pub recovery: u8,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TxOut {
    pub to: String,
    pub data: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Response {
    WalletCreated {
        /// `0x01 || nonce12 || AES-256-GCM(kek, entropy)` (lowki-keys format, 61 bytes).
        wrapped_blob: SecretBytes,
        /// `0x02 || nonce12 || AES-256-GCM(kek, P)` (62 bytes); see README.
        wrapped_passkey_share: SecretBytes,
        /// S: store on lowki-server, released only for a valid Google/email session.
        server_share: SecretBytes,
        /// R: shown to the user as the recovery code / backed up.
        recovery_share: SecretBytes,
        share_xs: ShareXs,
        expires_at_ms: f64,
    },
    Unlocked { expires_at_ms: f64 },
    Rewrapped { wrapped_blob: SecretBytes, wrapped_passkey_share: Option<SecretBytes> },
    Locked,
    Status { unlocked: bool, expires_at_ms: Option<f64>, circuits: Vec<String>, gpu: bool },
    AppAccount { origin: String, address: String, pairwise_user_id: String, app_index: u32 },
    StealthMetaAddress { uri: String, spending_public_key: String, viewing_public_key: String },
    Signature { address: String, signature: String, hash: String },
    SignedTransaction { address: String, raw_transaction: String, hash: String },
    SignedAuthorization { address: String, authorization: SignedAuthorizationIn },
    PpNote { precommitment: String, nullifier_hash: String, commitment: Option<String> },
    PpShield { precommitment: String, tx: TxOut },
    CircuitsLoaded { circuits: Vec<String>, witness_backend: String, gpu: bool, gpu_error: Option<String> },
    Proof {
        circuit: String,
        proof: serde_json::Value,
        public_signals: Vec<String>,
        solidity: serde_json::Value,
        backend: String,
        ms: f64,
    },
    PaymasterUserOp { sender: String, user_op_hash: String, user_operation: serde_json::Value },
    KohakuKey { key: SecretHex },
    Ok,
    NoteSignature { signature: SecretHex },
    SealedAppKey {
        suite: String,
        address: String,
        enc: String,
        ciphertext: String,
        grant: serde_json::Value,
        grant_hash: String,
        grant_signature: String,
    },
    EnsIdentity { index: u32, path: String, address: String },
    EnsRegistration { owner: String, signature: String },
    EnsOwnerSetup {
        owner: String,
        execute_signature: String,
        authorization: SignedAuthorizationIn,
        /// The ENSIP-30 record `0x01 || spendPub || viewPub` the batch writes.
        record: String,
        meta_address: String,
    },
    RecordsAuth { name: String, timestamp: u64, signature: String, viewing_address: String },
    StealthScan { scanned: u32, found: Vec<StealthFound> },
    SealedAppList { blob: String },
    AppList { data: String },
    Error { code: crate::error::ErrorCode, message: String },
}

impl From<WorkerError> for Response {
    fn from(e: WorkerError) -> Self {
        Response::Error { code: e.code, message: e.message }
    }
}

/// A secret that Kohaku's interfaces require as a `0x` hex string (a scoped key or a
/// note signature). Zeroized on drop, redacted in `Debug`.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretHex(String);

impl SecretHex {
    pub fn from_bytes(b: &[u8]) -> Self {
        let mut s = String::with_capacity(2 + 2 * b.len());
        s.push_str("0x");
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for &x in b {
            s.push(HEX[(x >> 4) as usize] as char);
            s.push(HEX[(x & 15) as usize] as char);
        }
        Self(s)
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretHex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretHex(..)")
    }
}

impl Serialize for SecretHex {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}
