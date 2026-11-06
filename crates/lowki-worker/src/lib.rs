//! # lowki-worker
//!
//! The single wasm module that holds Lowki keys. It runs in a dedicated Web Worker inside
//! the Lowki iframe and answers typed messages ([`protocol::Request`] →
//! [`protocol::Response`]). The seed exists only while a session is unlocked, and it is
//! zeroized on lock (explicit or automatic after `auto_lock_ms`).
//!
//! [`Core`] is the target-independent state machine (tested natively); `wasm` is the
//! wasm-bindgen front (`LowkiWorker`), which supplies the clock, the RNG (WebCrypto via
//! getrandom) and a timer that locks the session even when no message arrives.

pub mod delegation;
pub mod error;
pub mod pp;
pub mod protocol;
pub mod session;
pub mod sign;
pub mod util;

#[cfg(target_arch = "wasm32")]
pub mod wasm_api;

use std::rc::Rc;

use alloy_primitives::{B256, U256};
use hmac::{Hmac, Mac};
use lowki_groth16::{Circuit, CommitmentInputs, WithdrawInputs};
use rand_core::{CryptoRng, RngCore};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::error::{ErrorCode, Result, WorkerError};
use crate::pp::Prover;
use crate::protocol::{Request, Response, SecretBytes, SecretHex, TxOut};
use crate::session::Session;

/// Default auto-lock: 15 minutes after unlock.
pub const DEFAULT_AUTO_LOCK_MS: f64 = 15.0 * 60.0 * 1000.0;

/// Domain separation for the pairwise user ID (distinct from "lowki/app/v1", which picks
/// the per-app BIP-32 index).
pub const PAIRWISE_KEY_INFO: &[u8] = b"lowki/pairwise-id/key/v1";
pub const PAIRWISE_MSG_TAG: &[u8] = b"lowki/pairwise-id/v1";

pub trait CsRng: RngCore + CryptoRng {}
impl<T: RngCore + CryptoRng> CsRng for T {}

/// Owned dynamic CSPRNG (WebCrypto via getrandom in the browser; injectable in tests).
pub struct BoxRng(pub Box<dyn CsRng>);
impl RngCore for BoxRng {
    fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }
    fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }
    fn fill_bytes(&mut self, d: &mut [u8]) {
        self.0.fill_bytes(d)
    }
    fn try_fill_bytes(&mut self, d: &mut [u8]) -> std::result::Result<(), rand_core::Error> {
        self.0.try_fill_bytes(d)
    }
}
impl CryptoRng for BoxRng {}

/// Work that must run asynchronously (WebGPU, async wasm compile), prepared synchronously
/// so no `RefCell` borrow is held across an `.await`.
pub enum Action {
    Done(Response),
    ProveWithdraw(Rc<Prover>, Box<WithdrawInputs>),
    ProveCommitment(Rc<Prover>, Box<CommitmentInputs>),
    LoadCircuits(Box<LoadCircuits>),
}

pub struct LoadCircuits {
    pub withdraw: Option<(Vec<u8>, Vec<u8>)>,
    pub commitment: Option<(Vec<u8>, Vec<u8>)>,
    pub use_gpu: bool,
}

/// Result of [`Action::LoadCircuits`], installed with [`Core::install_provers`].
pub struct LoadedProvers {
    pub withdraw: Option<Prover>,
    pub commitment: Option<Prover>,
    pub witness_backend: String,
    pub gpu: bool,
    pub gpu_error: Option<String>,
}

pub struct Core {
    session: Option<Session>,
    auto_lock_ms: f64,
    withdraw: Option<Rc<Prover>>,
    commitment: Option<Rc<Prover>>,
    rng: BoxRng,
}

/// Pairwise user ID for `origin`: `HMAC-SHA256(K, "lowki/pairwise-id/v1" || 0x00 || origin)`
/// with `K = HKDF-SHA256(ikm = seed, salt = "", info = "lowki/pairwise-id/key/v1")`.
pub fn pairwise_user_id(seed: &[u8; 64], origin: &str) -> [u8; 32] {
    let hk = hkdf::Hkdf::<Sha256>::new(None, seed);
    let mut k = Zeroizing::new([0u8; 32]);
    hk.expand(PAIRWISE_KEY_INFO, k.as_mut_slice()).expect("32 bytes is a valid HKDF length");
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(k.as_slice()).expect("any key length");
    mac.update(PAIRWISE_MSG_TAG);
    mac.update(&[0]);
    mac.update(origin.as_bytes());
    mac.finalize().into_bytes().into()
}

impl Core {
    pub fn new(auto_lock_ms: f64, rng: Box<dyn CsRng>) -> Self {
        Self { session: None, auto_lock_ms, withdraw: None, commitment: None, rng: BoxRng(rng) }
    }

    pub fn is_unlocked(&self) -> bool {
        self.session.is_some()
    }

    pub fn expires_at_ms(&self) -> Option<f64> {
        self.session.as_ref().map(|s| s.expires_at_ms)
    }

    /// Drops (and thereby zeroizes) the session.
    pub fn lock(&mut self) {
        self.session = None;
    }

    /// Locks if the session has expired. Returns true if it locked.
    pub fn lock_if_expired(&mut self, now_ms: f64) -> bool {
        if self.session.as_ref().is_some_and(|s| now_ms >= s.expires_at_ms) {
            self.lock();
            return true;
        }
        false
    }

    fn session(&self) -> Result<&Session> {
        self.session.as_ref().ok_or_else(WorkerError::locked)
    }

    fn start(&mut self, entropy: &[u8; 32], now_ms: f64) -> Result<f64> {
        let expires = now_ms + self.auto_lock_ms;
        self.session = Some(Session::from_entropy(entropy, expires)?);
        Ok(expires)
    }

    fn require_locked(&self) -> Result<()> {
        if self.session.is_some() {
            return Err(WorkerError::new(ErrorCode::AlreadyUnlocked, "lock the current session first"));
        }
        Ok(())
    }

    pub fn install_provers(&mut self, p: LoadedProvers) -> Response {
        if let Some(w) = p.withdraw {
            self.withdraw = Some(Rc::new(w));
        }
        if let Some(c) = p.commitment {
            self.commitment = Some(Rc::new(c));
        }
        Response::CircuitsLoaded {
            circuits: self.circuit_names(),
            witness_backend: p.witness_backend,
            gpu: p.gpu,
            gpu_error: p.gpu_error,
        }
    }

    fn circuit_names(&self) -> Vec<String> {
        let mut v = vec![];
        if self.withdraw.is_some() {
            v.push("withdraw".into());
        }
        if self.commitment.is_some() {
            v.push("commitment".into());
        }
        v
    }

    /// Handles a request; async work is returned as an [`Action`].
    pub fn prepare(&mut self, req: Request, now_ms: f64) -> Action {
        self.lock_if_expired(now_ms);
        match self.prepare_inner(req, now_ms) {
            Ok(a) => a,
            Err(e) => Action::Done(e.into()),
        }
    }

    fn prepare_inner(&mut self, req: Request, now_ms: f64) -> Result<Action> {
        let done = |r: Response| Ok(Action::Done(r));
        match req {
            // ---- session -------------------------------------------------------------
            Request::CreateWallet { prf_output } => {
                self.require_locked()?;
                let mut entropy = Zeroizing::new([0u8; 32]);
                self.rng.fill_bytes(entropy.as_mut_slice());
                self.create_from_entropy(&prf_output, &entropy, now_ms).map(Action::Done)
            }
            Request::ImportMnemonic { mnemonic, prf_output } => {
                self.require_locked()?;
                let entropy = lowki_keys::mnemonic_to_entropy(mnemonic.as_str())
                    .map_err(|_| WorkerError::invalid("mnemonic must be a valid 24-word English BIP-39 phrase"))?;
                self.create_from_entropy(&prf_output, &entropy, now_ms).map(Action::Done)
            }
            Request::Unlock { prf_output, wrapped_blob } => {
                self.require_locked()?;
                let entropy = session::unlock_entropy(&prf_output, &wrapped_blob)?;
                let expires_at_ms = self.start(&entropy, now_ms)?;
                done(Response::Unlocked { expires_at_ms })
            }
            Request::Recover { server_share, recovery_share } => {
                self.require_locked()?;
                let entropy = session::combine_entropy(server_share.as_slice(), recovery_share.as_slice())?;
                let expires_at_ms = self.start(&entropy, now_ms)?;
                done(Response::Unlocked { expires_at_ms })
            }
            Request::RecoverWithPasskeyShare { prf_output, wrapped_passkey_share, other_share } => {
                self.require_locked()?;
                let p = session::unwrap_passkey_share(&prf_output, &wrapped_passkey_share)?;
                let entropy = session::combine_entropy(p.as_slice(), other_share.as_slice())?;
                let expires_at_ms = self.start(&entropy, now_ms)?;
                done(Response::Unlocked { expires_at_ms })
            }
            Request::Rewrap { prf_output, passkey_share_x, server_share, recovery_share } => {
                let entropy = Zeroizing::new(*self.session()?.entropy());
                let wrapped_blob = session::rewrap(&prf_output, &entropy, &mut self.rng)?;
                let wrapped_passkey_share = match (passkey_share_x, server_share, recovery_share) {
                    (Some(x), Some(s), Some(r)) => {
                        // Check S and R belong to this seed before re-deriving P from them.
                        let check = session::combine_entropy(s.as_slice(), r.as_slice())?;
                        if check.as_slice() != entropy.as_slice() {
                            return Err(WorkerError::new(ErrorCode::InvalidShares, "shares do not match the unlocked seed"));
                        }
                        let p = session::share_at(s.as_slice(), r.as_slice(), x)?;
                        Some(session::wrap_passkey_share(&prf_output, p.as_slice(), &mut self.rng)?)
                    }
                    (None, None, None) => None,
                    _ => return Err(WorkerError::invalid("give passkeyShareX, serverShare and recoveryShare together")),
                };
                done(Response::Rewrapped { wrapped_blob, wrapped_passkey_share })
            }
            Request::Lock => {
                self.lock();
                done(Response::Locked)
            }
            Request::Status => done(Response::Status {
                unlocked: self.is_unlocked(),
                expires_at_ms: self.expires_at_ms(),
                circuits: self.circuit_names(),
                gpu: self.withdraw.as_ref().is_some_and(|p| p.is_gpu()),
            }),

            // ---- accounts --------------------------------------------------------------
            Request::AppAccount { origin } => {
                let origin = util::origin(&origin)?.to_string();
                let s = self.session()?;
                let address = lowki_keys::to_checksum_address(&lowki_keys::app_address(s.seed(), &origin));
                let id = pairwise_user_id(s.seed(), &origin);
                done(Response::AppAccount {
                    app_index: lowki_keys::app_index(&origin),
                    origin,
                    address,
                    pairwise_user_id: util::hex0x(&id),
                })
            }
            Request::StealthMetaAddress => {
                let keys = lowki_stealth::derive_stealth_keys(self.session()?.seed())?;
                let meta = keys.meta_address();
                done(Response::StealthMetaAddress {
                    uri: meta.to_uri(),
                    spending_public_key: util::hex0x(meta.spending_pubkey()),
                    viewing_public_key: util::hex0x(meta.viewing_pubkey()),
                })
            }

            // ---- signing ---------------------------------------------------------------
            Request::SignMessage { origin, message, message_hex } => {
                let msg: Vec<u8> = match (message, message_hex) {
                    (Some(m), None) => m.into_bytes(),
                    (None, Some(h)) => util::bytes(&h, "messageHex")?.to_vec(),
                    _ => return Err(WorkerError::invalid("give exactly one of message or messageHex")),
                };
                let hash = sign::eip191_hash(&msg);
                self.sign_digest_response(&origin, hash).map(Action::Done)
            }
            Request::SignTypedData { origin, typed_data } => {
                let hash = sign::eip712_hash(&typed_data)?;
                self.sign_digest_response(&origin, hash).map(Action::Done)
            }
            Request::SignTransaction { origin, tx } => {
                let (key, address) = self.app_key(&origin)?;
                let signed = sign::sign_transaction(&key, &tx)?;
                done(Response::SignedTransaction {
                    address,
                    raw_transaction: util::hex0x(&signed.raw),
                    hash: format!("{}", signed.hash),
                })
            }
            Request::SignAuthorization { origin, chain_id, address, nonce } => {
                let (key, from) = self.app_key(&origin)?;
                let nonce = util::u64q(&nonce, "nonce")?;
                if nonce > (1u64 << 53) - 1 {
                    return Err(WorkerError::invalid("nonce above 2^53-1"));
                }
                let authorization =
                    sign::sign_authorization(&key, util::u256(&chain_id, "chainId")?, util::address(&address, "address")?, nonce)?;
                done(Response::SignedAuthorization { address: from, authorization })
            }

            // ---- privacy pools ---------------------------------------------------------
            Request::PpNote { note, value, label } => {
                let s = self.session()?;
                let c = pp::NoteCoords::parse(&note)?;
                let secret = pp::note_secret(s.seed(), &c, c.secret_index)?;
                let commitment = match (value, label) {
                    (Some(v), Some(l)) => Some(util::fr_to_dec(&lowki_pp_core::commitment(
                        util::fr(&v, "value")?,
                        util::fr(&l, "label")?,
                        secret.precommitment,
                    ))),
                    (None, None) => None,
                    _ => return Err(WorkerError::invalid("give value and label together")),
                };
                done(Response::PpNote {
                    precommitment: util::fr_to_dec(&secret.precommitment),
                    nullifier_hash: util::fr_to_dec(&secret.nullifier_hash),
                    commitment,
                })
            }
            Request::PpShield { note, value } => {
                let s = self.session()?;
                let c = pp::NoteCoords::parse(&note)?;
                if c.secret_index != 0 {
                    return Err(WorkerError::invalid("a shield creates the deposit note: secretIndex must be 0"));
                }
                let secret = pp::note_secret(s.seed(), &c, 0)?;
                let pre = U256::from_be_bytes(lowki_pp_core::fr_to_be_bytes(&secret.precommitment));
                let tx = lowki_aa::shield::prepare_native_shield(
                    pre,
                    util::u256(&value, "value")?,
                    alloy_primitives::Address::from(c.entrypoint),
                );
                done(Response::PpShield {
                    precommitment: pre.to_string(),
                    tx: TxOut { to: tx.to.to_checksum(None), data: util::hex0x(&tx.data), value: tx.value.to_string() },
                })
            }
            Request::PpLoadCircuits { withdraw_wasm, withdraw_zkey, commitment_wasm, commitment_zkey, use_gpu } => {
                let pair = |w: Option<serde_bytes::ByteBuf>, z: Option<serde_bytes::ByteBuf>, name: &str| match (w, z) {
                    (Some(w), Some(z)) => Ok(Some((w.into_vec(), z.into_vec()))),
                    (None, None) => Ok(None),
                    _ => Err(WorkerError::invalid(format!("{name}: give wasm and zkey together"))),
                };
                let withdraw = pair(withdraw_wasm, withdraw_zkey, "withdraw")?;
                let commitment = pair(commitment_wasm, commitment_zkey, "commitment")?;
                if withdraw.is_none() && commitment.is_none() {
                    return Err(WorkerError::invalid("no circuit artifacts given"));
                }
                Ok(Action::LoadCircuits(Box::new(LoadCircuits { withdraw, commitment, use_gpu })))
            }
            Request::PpProveWithdraw { note, label, existing_value, withdrawn_value, context, state_proof, asp_proof } => {
                let prover = self.withdraw.clone().ok_or_else(|| {
                    WorkerError::new(ErrorCode::CircuitsNotLoaded, "withdraw circuit not loaded (ppLoadCircuits)")
                })?;
                let inputs = pp::withdraw_inputs(
                    self.session()?.seed(),
                    &pp::WithdrawRequest {
                        note: &note,
                        label: &label,
                        existing_value: &existing_value,
                        withdrawn_value: &withdrawn_value,
                        context: &context,
                        state_proof: &state_proof,
                        asp_proof: &asp_proof,
                    },
                )?;
                Ok(Action::ProveWithdraw(prover, Box::new(inputs)))
            }
            Request::PpProveCommitment { note, value, label } => {
                let prover = self.commitment.clone().ok_or_else(|| {
                    WorkerError::new(ErrorCode::CircuitsNotLoaded, "commitment circuit not loaded (ppLoadCircuits)")
                })?;
                let inputs = pp::commitment_inputs(self.session()?.seed(), &note, &value, &label)?;
                Ok(Action::ProveCommitment(prover, Box::new(inputs)))
            }
            Request::PpBuildPaymasterWithdraw {
                note,
                withdrawal,
                proof,
                pool,
                paymaster,
                adapter,
                gas,
                max_fee_per_gas,
                max_priority_fee_per_gas,
                nonce,
                authorization_nonce,
            } => {
                let out = pp::build_paymaster_withdraw(
                    self.session()?.seed(),
                    &pp::PaymasterParams {
                        note: &note,
                        withdrawal: &withdrawal,
                        proof: &proof,
                        pool: &pool,
                        paymaster: &paymaster,
                        adapter: &adapter,
                        gas: &gas,
                        max_fee_per_gas: &max_fee_per_gas,
                        max_priority_fee_per_gas: &max_priority_fee_per_gas,
                        nonce: &nonce,
                        authorization_nonce,
                    },
                )?;
                done(Response::PaymasterUserOp {
                    sender: out.sender.to_checksum(None),
                    user_op_hash: out.user_op_hash,
                    user_operation: out.user_operation,
                })
            }

            // ---- Kohaku host -----------------------------------------------------------
            Request::KohakuDeriveAt { path } => {
                let key = self.session()?.kohaku().derive_at(&path)?;
                done(Response::KohakuKey { key: SecretHex::from_bytes(key.as_slice()) })
            }
            Request::KohakuAllowPath { path } => {
                lowki_keys::parse_derivation_path(&path)?;
                let s = self.session.as_mut().ok_or_else(WorkerError::locked)?;
                s.kohaku_mut().allow_path(&path);
                done(Response::Ok)
            }
            Request::PpSignNoteEnvelope { account_index, chain_id, entrypoint, deposit_index, secret_index } => {
                let c = pp::NoteCoords {
                    account_index,
                    chain_id: util::u64q(&chain_id, "chainId")?,
                    entrypoint: util::address(&entrypoint, "entrypoint")?.0.0,
                    deposit_index,
                    secret_index,
                };
                pp::account_path(account_index)?;
                let sig = pp::sign_note_envelope(self.session()?.seed(), &c)?;
                done(Response::NoteSignature { signature: SecretHex::from_bytes(sig.as_slice()) })
            }

            // ---- delegation ------------------------------------------------------------
            Request::SealAppKey { origin, enclave_public_key, grant } => {
                let origin = util::origin(&origin)?.to_string();
                let seed = Zeroizing::new(*self.session()?.seed());
                let s = delegation::seal_app_key(&seed, &origin, &enclave_public_key, &grant, &mut self.rng)?;
                done(Response::SealedAppKey {
                    suite: delegation::SUITE.into(),
                    address: s.address,
                    enc: util::hex0x(&s.enc),
                    ciphertext: util::hex0x(&s.ciphertext),
                    grant: s.grant,
                    grant_hash: util::hex0x(&s.grant_hash),
                    grant_signature: util::hex0x(&s.grant_signature),
                })
            }
        }
    }

    fn create_from_entropy(&mut self, prf: &SecretBytes, entropy: &[u8; 32], now_ms: f64) -> Result<Response> {
        let w = session::new_wallet(prf, entropy, &mut self.rng)?;
        let expires_at_ms = self.start(entropy, now_ms)?;
        Ok(Response::WalletCreated {
            wrapped_blob: w.wrapped_blob.clone(),
            wrapped_passkey_share: w.wrapped_passkey_share.clone(),
            server_share: w.server_share.clone(),
            recovery_share: w.recovery_share.clone(),
            share_xs: w.share_xs,
            expires_at_ms,
        })
    }

    fn app_key(&self, origin: &str) -> Result<(Zeroizing<[u8; 32]>, String)> {
        let origin = util::origin(origin)?;
        let key = lowki_keys::app_private_key(self.session()?.seed(), origin);
        let address = lowki_keys::to_checksum_address(&lowki_keys::address_from_private_key(&key)?);
        Ok((key, address))
    }

    fn sign_digest_response(&self, origin: &str, hash: B256) -> Result<Response> {
        let (key, address) = self.app_key(origin)?;
        let sig = sign::sign_hash(&key, &hash)?;
        Ok(Response::Signature { address, signature: util::hex0x(&sig.to_bytes65()), hash: format!("{hash}") })
    }
}

/// Runs an [`Action`]'s async part. `now_ms` is a clock for timings.
pub async fn run_proof(action: Action, now_ms: &dyn Fn() -> f64) -> Response {
    let t0 = now_ms();
    let (circuit, r) = match action {
        Action::Done(r) => return r,
        Action::LoadCircuits(_) => return WorkerError::new(ErrorCode::Internal, "use load_circuits").into(),
        Action::ProveWithdraw(p, inputs) => ("withdraw", pp::prove_withdraw(&p, &inputs).await),
        Action::ProveCommitment(p, inputs) => ("commitment", pp::prove_commitment(&p, &inputs).await),
    };
    match r {
        Ok((out, backend)) => Response::Proof {
            circuit: circuit.into(),
            proof: serde_json::to_value(&out.proof).unwrap_or_default(),
            public_signals: out.public_signals.clone(),
            solidity: serde_json::to_value(&out.solidity).unwrap_or_default(),
            backend: backend.into(),
            ms: now_ms() - t0,
        },
        Err(e) => e.into(),
    }
}

/// Loads circuit provers (and uploads them to the GPU when asked and available).
pub async fn load_circuits(l: LoadCircuits) -> Result<LoadedProvers> {
    let mut gpu_error = None;
    #[cfg(feature = "webgpu")]
    let gpu = if l.use_gpu {
        match lowki_groth16::open_gpu().await {
            Ok(g) => Some(g),
            Err(e) => {
                gpu_error = Some(format!("{e}"));
                None
            }
        }
    } else {
        None
    };
    #[cfg(not(feature = "webgpu"))]
    if l.use_gpu {
        gpu_error = Some("built without the webgpu feature".to_string());
    }
    let mut backend = String::new();
    let mut load = async |circuit: Circuit, w: &[u8], z: &[u8]| -> Result<Prover> {
        let cpu = pp::load_prover(circuit, w, z).await?;
        backend = cpu.witness_backend().to_string();
        #[cfg(feature = "webgpu")]
        if let Some(g) = &gpu {
            return match lowki_groth16::GpuProver::new(g.clone(), cpu) {
                Ok(gp) => Ok(Prover::Gpu(gp)),
                Err((cpu, e)) => {
                    gpu_error = Some(format!("{e}"));
                    Ok(Prover::Cpu(cpu))
                }
            };
        }
        Ok(Prover::Cpu(cpu))
    };
    let withdraw = match &l.withdraw {
        Some((w, z)) => Some(load(Circuit::Withdraw, w, z).await?),
        None => None,
    };
    let commitment = match &l.commitment {
        Some((w, z)) => Some(load(Circuit::Commitment, w, z).await?),
        None => None,
    };
    let gpu_on = withdraw.as_ref().or(commitment.as_ref()).is_some_and(Prover::is_gpu);
    Ok(LoadedProvers { withdraw, commitment, witness_backend: backend, gpu: gpu_on, gpu_error })
}

/// Native convenience: handle one request end to end (blocking on async parts).
#[cfg(not(target_arch = "wasm32"))]
pub fn handle_blocking(core: &mut Core, req: Request, now_ms: f64) -> Response {
    let clock = || {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
    };
    match core.prepare(req, now_ms) {
        Action::LoadCircuits(l) => match pollster::block_on(load_circuits(*l)) {
            Ok(p) => core.install_provers(p),
            Err(e) => e.into(),
        },
        a => pollster::block_on(run_proof(a, &clock)),
    }
}
