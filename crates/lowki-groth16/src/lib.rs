//! Groth16 (bn128) prover for the Privacy Pools v1 `commitment` and `withdraw` circuits,
//! producing proofs that snarkjs 0.7.5 `groth16.verify` and the on-chain Solidity verifier
//! accept. Runs natively and as `wasm32-unknown-unknown`.
//!
//! * Witness: the circuit's own circom witness-generator `.wasm` ([`witness::WitnessCalculator`]),
//!   interpreted with `wasmi` natively, and run on the host's `WebAssembly` engine on wasm32.
//! * Proving key: snarkjs `.zkey`, parsed from bytes ([`CircuitProver::load`]).
//! * Proving: `ark-groth16` 0.5 with snarkjs' R1CS-to-QAP map ([`CircomReduction`]).
//!
//! ```ignore
//! let prover = CircuitProver::load(Circuit::Withdraw, &wasm_bytes, &zkey_bytes)?;
//! let out = prove_withdraw(&prover, &WithdrawInputs::from_json(json)?)?;
//! assert!(verify(prover.verifying_key(), &out.proof, &out.public_signals)?);
//! // out.solidity = { pA, pB, pC, pubSignals } for Entrypoint.relay / Pool.ragequit
//! ```

#[cfg(feature = "webgpu")]
pub mod gpu;
mod qap;
mod stages;
pub mod witness;
mod zkey;

use core::fmt;
use core::str::FromStr;

use ark_bn254::{Fq, Fq2, G1Affine, G2Affine};
use ark_ec::AffineRepr;
use ark_ff::{BigInteger, PrimeField, UniformRand, Zero};
use ark_groth16::{Groth16, PreparedVerifyingKey, ProvingKey, prepare_verifying_key};
use ark_poly::{EvaluationDomain, GeneralEvaluationDomain};
use ark_relations::r1cs::ConstraintMatrices;
use ark_std::rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroize;

pub use ark_bn254::{Bn254, Fr};
pub use ark_groth16::{Proof, VerifyingKey};
pub use qap::CircomReduction;
#[doc(hidden)]
pub use stages::StageTimes;
pub use witness::WitnessCalculator;
#[cfg(feature = "webgpu")]
pub use gpu::{GpuProver, open_gpu, prove_commitment_gpu, prove_withdraw_gpu};

// ------------------------------------------------------------------------------------
// Errors
// ------------------------------------------------------------------------------------

/// Error type. Messages never contain secret input values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The witness-generator wasm could not be compiled / instantiated.
    Wasm(String),
    /// The circuit rejected the inputs (e.g. a failed `assert`) or the runtime trapped.
    Witness(String),
    /// Inputs are malformed (missing signal, wrong array length, bad number).
    Input(String),
    /// Malformed `.zkey`.
    Zkey(String),
    /// Malformed verification key / proof JSON.
    Format(String),
    /// Artifacts don't belong to the requested circuit, or don't match each other.
    Circuit(String),
    /// Proof generation failed (including the post-proof self-check).
    Proof(String),
    /// The WebGPU path failed (no adapter, device lost, GPU error). Fall back to the CPU.
    #[cfg(feature = "webgpu")]
    Gpu(lowki_gpu::GpuError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Wasm(m) => write!(f, "witness wasm: {m}"),
            Error::Witness(m) => write!(f, "witness generation failed: {m}"),
            Error::Input(m) => write!(f, "invalid input: {m}"),
            Error::Zkey(m) => write!(f, "invalid zkey: {m}"),
            Error::Format(m) => write!(f, "invalid format: {m}"),
            Error::Circuit(m) => write!(f, "circuit mismatch: {m}"),
            Error::Proof(m) => write!(f, "proving failed: {m}"),
            #[cfg(feature = "webgpu")]
            Error::Gpu(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

// ------------------------------------------------------------------------------------
// Field-element parsing helpers
// ------------------------------------------------------------------------------------

/// Parse a decimal or `0x`-hex string into `F`, rejecting values `>= modulus`.
fn parse_strict<F: PrimeField>(s: &str) -> Result<F, Error> {
    let s = s.trim();
    let bad = || Error::Format(format!("not a canonical field element: {:.20}", s));
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        if hex.is_empty() || hex.len() > 64 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(bad());
        }
        let padded = format!("{hex:0>64}");
        let mut be = [0u8; 32];
        for (i, b) in be.iter_mut().enumerate() {
            *b = u8::from_str_radix(&padded[2 * i..2 * i + 2], 16).map_err(|_| bad())?;
        }
        let f = F::from_be_bytes_mod_order(&be);
        // reject non-canonical (>= modulus) encodings
        let mut back = f.into_bigint().to_bytes_be();
        let ok = back[..] == be[..];
        back.zeroize();
        be.zeroize();
        return if ok { Ok(f) } else { Err(bad()) };
    }
    if s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit()) {
        return Err(bad());
    }
    let f = F::from_str(s).map_err(|_| bad())?;
    let trimmed = s.trim_start_matches('0');
    let canonical = f.to_string();
    if (trimmed.is_empty() && f.is_zero()) || canonical == trimmed {
        Ok(f)
    } else {
        Err(bad())
    }
}

/// Parse a snarkjs-style input value: decimal/hex string or JSON integer. Like snarkjs
/// (`normalize`), values are taken modulo the field, so `p` and `0` are the same signal.
fn input_fr(v: &Value, name: &str) -> Result<Fr, Error> {
    let bad = || Error::Input(format!("{name}: expected a decimal/hex string or integer"));
    match v {
        Value::String(s) => {
            let s = s.trim();
            if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                if hex.is_empty() || hex.len() > 64 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
                    return Err(bad());
                }
                let padded = format!("{hex:0>64}");
                let mut be = [0u8; 32];
                for (i, b) in be.iter_mut().enumerate() {
                    *b = u8::from_str_radix(&padded[2 * i..2 * i + 2], 16).map_err(|_| bad())?;
                }
                let f = Fr::from_be_bytes_mod_order(&be);
                be.zeroize();
                Ok(f)
            } else if !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()) {
                Fr::from_str(s).map_err(|_| bad())
            } else {
                Err(bad())
            }
        }
        Value::Number(n) => n.as_u64().map(Fr::from).ok_or_else(bad),
        _ => Err(bad()),
    }
}

fn get<'a>(obj: &'a Value, name: &str) -> Result<&'a Value, Error> {
    obj.get(name)
        .ok_or_else(|| Error::Input(format!("missing signal {name}")))
}

fn get_fr(obj: &Value, name: &str) -> Result<Fr, Error> {
    input_fr(get(obj, name)?, name)
}

fn get_fr_vec(obj: &Value, name: &str) -> Result<Vec<Fr>, Error> {
    get(obj, name)?
        .as_array()
        .ok_or_else(|| Error::Input(format!("{name}: expected an array")))?
        .iter()
        .map(|v| input_fr(v, name))
        .collect()
}

// ------------------------------------------------------------------------------------
// Inputs
// ------------------------------------------------------------------------------------

/// Inputs of the `commitment` circuit (used for ragequit). Shape matches
/// `CommitmentInputSignals` in `@fatsolutions/privacy-pools-core-circuits`.
/// Zeroized on drop; `Debug` redacts everything.
#[derive(Clone)]
pub struct CommitmentInputs {
    pub value: Fr,
    pub label: Fr,
    pub nullifier: Fr,
    pub secret: Fr,
}

impl Drop for CommitmentInputs {
    fn drop(&mut self) {
        self.value.zeroize();
        self.label.zeroize();
        self.nullifier.zeroize();
        self.secret.zeroize();
    }
}

impl fmt::Debug for CommitmentInputs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CommitmentInputs { .. }")
    }
}

impl CommitmentInputs {
    /// Parse snarkjs-style JSON (`{"value": "123", ...}`, decimal or `0x` strings).
    pub fn from_json(json: &str) -> Result<Self, Error> {
        let v: Value = serde_json::from_str(json).map_err(|_| Error::Input("invalid JSON".into()))?;
        Ok(Self {
            value: get_fr(&v, "value")?,
            label: get_fr(&v, "label")?,
            nullifier: get_fr(&v, "nullifier")?,
            secret: get_fr(&v, "secret")?,
        })
    }

    /// `(signal, values)` pairs as fed to the witness generator.
    pub fn as_signals(&self) -> [(&'static str, &[Fr]); 4] {
        use core::slice::from_ref as one;
        [
            ("value", one(&self.value)),
            ("label", one(&self.label)),
            ("nullifier", one(&self.nullifier)),
            ("secret", one(&self.secret)),
        ]
    }
}

/// Inputs of the `withdraw` circuit. Shape matches `WithdrawInputSignals` in
/// `@fatsolutions/privacy-pools-core-circuits` (siblings padded to 32, depths = 32 as in
/// Kohaku's `withdrawThunk`). Zeroized on drop; `Debug` redacts everything.
#[derive(Clone)]
pub struct WithdrawInputs {
    pub withdrawn_value: Fr,
    pub state_root: Fr,
    pub state_tree_depth: Fr,
    pub asp_root: Fr,
    pub asp_tree_depth: Fr,
    pub context: Fr,
    pub label: Fr,
    pub existing_value: Fr,
    pub existing_nullifier: Fr,
    pub existing_secret: Fr,
    pub new_nullifier: Fr,
    pub new_secret: Fr,
    pub state_siblings: Vec<Fr>,
    pub state_index: Fr,
    pub asp_siblings: Vec<Fr>,
    pub asp_index: Fr,
}

impl Drop for WithdrawInputs {
    fn drop(&mut self) {
        for f in [
            &mut self.withdrawn_value,
            &mut self.state_root,
            &mut self.state_tree_depth,
            &mut self.asp_root,
            &mut self.asp_tree_depth,
            &mut self.context,
            &mut self.label,
            &mut self.existing_value,
            &mut self.existing_nullifier,
            &mut self.existing_secret,
            &mut self.new_nullifier,
            &mut self.new_secret,
            &mut self.state_index,
            &mut self.asp_index,
        ] {
            f.zeroize();
        }
        self.state_siblings.zeroize();
        self.asp_siblings.zeroize();
    }
}

impl fmt::Debug for WithdrawInputs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WithdrawInputs { .. }")
    }
}

impl WithdrawInputs {
    /// Parse snarkjs-style JSON using the circuit's signal names
    /// (`withdrawnValue`, `stateRoot`, `stateSiblings`, `ASPRoot`, ...).
    pub fn from_json(json: &str) -> Result<Self, Error> {
        let v: Value = serde_json::from_str(json).map_err(|_| Error::Input("invalid JSON".into()))?;
        Ok(Self {
            withdrawn_value: get_fr(&v, "withdrawnValue")?,
            state_root: get_fr(&v, "stateRoot")?,
            state_tree_depth: get_fr(&v, "stateTreeDepth")?,
            asp_root: get_fr(&v, "ASPRoot")?,
            asp_tree_depth: get_fr(&v, "ASPTreeDepth")?,
            context: get_fr(&v, "context")?,
            label: get_fr(&v, "label")?,
            existing_value: get_fr(&v, "existingValue")?,
            existing_nullifier: get_fr(&v, "existingNullifier")?,
            existing_secret: get_fr(&v, "existingSecret")?,
            new_nullifier: get_fr(&v, "newNullifier")?,
            new_secret: get_fr(&v, "newSecret")?,
            state_siblings: get_fr_vec(&v, "stateSiblings")?,
            state_index: get_fr(&v, "stateIndex")?,
            asp_siblings: get_fr_vec(&v, "ASPSiblings")?,
            asp_index: get_fr(&v, "ASPIndex")?,
        })
    }

    /// `(signal, values)` pairs as fed to the witness generator.
    pub fn as_signals(&self) -> [(&'static str, &[Fr]); 16] {
        use core::slice::from_ref as one;
        [
            ("withdrawnValue", one(&self.withdrawn_value)),
            ("stateRoot", one(&self.state_root)),
            ("stateTreeDepth", one(&self.state_tree_depth)),
            ("ASPRoot", one(&self.asp_root)),
            ("ASPTreeDepth", one(&self.asp_tree_depth)),
            ("context", one(&self.context)),
            ("label", one(&self.label)),
            ("existingValue", one(&self.existing_value)),
            ("existingNullifier", one(&self.existing_nullifier)),
            ("existingSecret", one(&self.existing_secret)),
            ("newNullifier", one(&self.new_nullifier)),
            ("newSecret", one(&self.new_secret)),
            ("stateSiblings", &self.state_siblings),
            ("stateIndex", one(&self.state_index)),
            ("ASPSiblings", &self.asp_siblings),
            ("ASPIndex", one(&self.asp_index)),
        ]
    }
}

// ------------------------------------------------------------------------------------
// Output formats
// ------------------------------------------------------------------------------------

/// A Groth16 proof in snarkjs JSON form (`groth16.fullProve(...).proof`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnarkjsProof {
    pub pi_a: [String; 3],
    pub pi_b: [[String; 2]; 3],
    pub pi_c: [String; 3],
    pub protocol: String,
    pub curve: String,
}

/// The on-chain proof struct (`WithdrawProof` / ragequit `ProofLib`):
/// `{ uint256[2] pA; uint256[2][2] pB; uint256[2] pC; uint256[N] pubSignals }`,
/// decimal strings. `pB` has each G2 coordinate's Fq2 limbs swapped (`[c1, c0]`), as the
/// EIP-197 precompile and snarkjs' Solidity verifier expect.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolidityProof {
    #[serde(rename = "pA")]
    pub p_a: [String; 2],
    #[serde(rename = "pB")]
    pub p_b: [[String; 2]; 2],
    #[serde(rename = "pC")]
    pub p_c: [String; 2],
    #[serde(rename = "pubSignals")]
    pub pub_signals: Vec<String>,
}

/// Result of a proof: snarkjs-shaped proof + public signals, and the Solidity calldata form.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofOutput {
    pub proof: SnarkjsProof,
    #[serde(rename = "publicSignals")]
    pub public_signals: Vec<String>,
    pub solidity: SolidityProof,
}

fn g1_json(p: &G1Affine) -> [String; 3] {
    match p.xy() {
        Some((x, y)) => [x.to_string(), y.to_string(), "1".into()],
        None => ["0".into(), "1".into(), "0".into()],
    }
}

fn g2_json(p: &G2Affine) -> [[String; 2]; 3] {
    match p.xy() {
        Some((x, y)) => [
            [x.c0.to_string(), x.c1.to_string()],
            [y.c0.to_string(), y.c1.to_string()],
            ["1".into(), "0".into()],
        ],
        None => [
            ["0".into(), "0".into()],
            ["1".into(), "0".into()],
            ["0".into(), "0".into()],
        ],
    }
}

fn g1_from_json(p: &[String]) -> Result<G1Affine, Error> {
    let [x, y, z] = p else {
        return Err(Error::Format("G1 point must have 3 coordinates".into()));
    };
    let z: Fq = parse_strict(z)?;
    if z.is_zero() {
        return Ok(G1Affine::identity());
    }
    if z != Fq::from(1u64) {
        return Err(Error::Format("G1 point must be affine (z = 1)".into()));
    }
    let pt = G1Affine::new_unchecked(parse_strict(x)?, parse_strict(y)?);
    if !pt.is_on_curve() {
        return Err(Error::Format("G1 point not on curve".into()));
    }
    Ok(pt)
}

fn g2_from_json(p: &[[String; 2]]) -> Result<G2Affine, Error> {
    let [x, y, z] = p else {
        return Err(Error::Format("G2 point must have 3 coordinates".into()));
    };
    let fq2 = |c: &[String; 2]| -> Result<Fq2, Error> { Ok(Fq2::new(parse_strict(&c[0])?, parse_strict(&c[1])?)) };
    let z = fq2(z)?;
    if z.is_zero() {
        return Ok(G2Affine::identity());
    }
    if z != Fq2::from(1u64) {
        return Err(Error::Format("G2 point must be affine (z = 1)".into()));
    }
    let pt = G2Affine::new_unchecked(fq2(x)?, fq2(y)?);
    if !pt.is_on_curve() || !pt.is_in_correct_subgroup_assuming_on_curve() {
        return Err(Error::Format("G2 point not in the subgroup".into()));
    }
    Ok(pt)
}

impl SnarkjsProof {
    pub fn from_ark(p: &Proof<Bn254>) -> Self {
        Self {
            pi_a: g1_json(&p.a),
            pi_b: g2_json(&p.b),
            pi_c: g1_json(&p.c),
            protocol: "groth16".into(),
            curve: "bn128".into(),
        }
    }

    pub fn to_ark(&self) -> Result<Proof<Bn254>, Error> {
        Ok(Proof {
            a: g1_from_json(&self.pi_a)?,
            b: g2_from_json(&self.pi_b)?,
            c: g1_from_json(&self.pi_c)?,
        })
    }

    /// Solidity calldata form; same mapping as Kohaku `toWithdrawProof` /
    /// `encodeRagequitPayload` (packages/privacy-pools/src/utils/encoding.utils.ts) and
    /// snarkjs `groth16.exportSolidityCallData`.
    pub fn to_solidity(&self, public_signals: &[String]) -> Result<SolidityProof, Error> {
        let fq = |s: &String| parse_strict::<Fq>(s).map(|f| f.to_string());
        let fr = |s: &String| parse_strict::<Fr>(s).map(|f| f.to_string());
        Ok(SolidityProof {
            p_a: [fq(&self.pi_a[0])?, fq(&self.pi_a[1])?],
            p_b: [
                [fq(&self.pi_b[0][1])?, fq(&self.pi_b[0][0])?],
                [fq(&self.pi_b[1][1])?, fq(&self.pi_b[1][0])?],
            ],
            p_c: [fq(&self.pi_c[0])?, fq(&self.pi_c[1])?],
            pub_signals: public_signals.iter().map(fr).collect::<Result<_, _>>()?,
        })
    }
}

/// Parse a snarkjs verification key JSON (`*.vkey`, `snarkjs zkey export verificationkey`).
pub fn parse_vkey_json(json: &str) -> Result<VerifyingKey<Bn254>, Error> {
    #[derive(Deserialize)]
    struct VkJson {
        protocol: String,
        curve: String,
        #[serde(rename = "nPublic")]
        n_public: usize,
        vk_alpha_1: [String; 3],
        vk_beta_2: [[String; 2]; 3],
        vk_gamma_2: [[String; 2]; 3],
        vk_delta_2: [[String; 2]; 3],
        #[serde(rename = "IC")]
        ic: Vec<[String; 3]>,
    }
    let v: VkJson = serde_json::from_str(json).map_err(|e| Error::Format(format!("vkey: {e}")))?;
    if v.protocol != "groth16" || v.curve != "bn128" || v.ic.len() != v.n_public + 1 {
        return Err(Error::Format("vkey is not a bn128 Groth16 key".into()));
    }
    Ok(VerifyingKey {
        alpha_g1: g1_from_json(&v.vk_alpha_1)?,
        beta_g2: g2_from_json(&v.vk_beta_2)?,
        gamma_g2: g2_from_json(&v.vk_gamma_2)?,
        delta_g2: g2_from_json(&v.vk_delta_2)?,
        gamma_abc_g1: v.ic.iter().map(|p| g1_from_json(p)).collect::<Result<_, _>>()?,
    })
}

/// Verify a snarkjs-shaped proof against a verifying key, like `snarkjs.groth16.verify`.
/// Public signals must be canonical (`< r`), as the Solidity verifier requires.
pub fn verify(vk: &VerifyingKey<Bn254>, proof: &SnarkjsProof, public_signals: &[String]) -> Result<bool, Error> {
    if public_signals.len() + 1 != vk.gamma_abc_g1.len() {
        return Ok(false);
    }
    let inputs: Vec<Fr> = public_signals
        .iter()
        .map(|s| parse_strict(s))
        .collect::<Result<_, _>>()?;
    let pvk = prepare_verifying_key(vk);
    Groth16::<Bn254, CircomReduction>::verify_proof(&pvk, &proof.to_ark()?, &inputs)
        .map_err(|e| Error::Proof(e.to_string()))
}

// ------------------------------------------------------------------------------------
// Prover
// ------------------------------------------------------------------------------------

/// Which Privacy Pools circuit an artifact set belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Circuit {
    /// `commitment` (ragequit): public signals `[commitment, nullifierHash, value, label]`.
    Commitment,
    /// `withdraw`: public signals `[newCommitmentHash, existingNullifierHash, withdrawnValue,
    /// stateRoot, stateTreeDepth, ASPRoot, ASPTreeDepth, context]`.
    Withdraw,
}

impl Circuit {
    pub fn n_public(self) -> usize {
        match self {
            Circuit::Commitment => 4,
            Circuit::Withdraw => 8,
        }
    }
}

/// A loaded circuit: witness generator + proving key. Build once, prove many times.
pub struct CircuitProver {
    circuit: Circuit,
    calc: WitnessCalculator,
    pk: ProvingKey<Bn254>,
    pvk: PreparedVerifyingKey<Bn254>,
    matrices: ConstraintMatrices<Fr>,
}

impl CircuitProver {
    /// Load from artifact bytes (`<circuit>.wasm`, `<circuit>.zkey`), e.g. fetched by the browser.
    ///
    /// On wasm32 the witness `.wasm` is compiled synchronously by the host engine; on a browser
    /// main thread prefer [`CircuitProver::load_async`] (see [`WitnessCalculator::new`]).
    pub fn load(circuit: Circuit, wasm: &[u8], zkey: &[u8]) -> Result<Self, Error> {
        Self::with_calc(circuit, WitnessCalculator::new(wasm)?, zkey)
    }

    /// wasm32 only: like [`CircuitProver::load`], but compiles the witness `.wasm` with
    /// `WebAssembly.compile` (async; no main-thread size cap). The zkey parse is still synchronous.
    #[cfg(target_arch = "wasm32")]
    pub async fn load_async(circuit: Circuit, wasm: &[u8], zkey: &[u8]) -> Result<Self, Error> {
        Self::with_calc(circuit, WitnessCalculator::new_async(wasm).await?, zkey)
    }

    fn with_calc(circuit: Circuit, calc: WitnessCalculator, zkey: &[u8]) -> Result<Self, Error> {
        let z = zkey::read_zkey(zkey)?;
        if z.n_public != circuit.n_public() {
            return Err(Error::Circuit(format!(
                "zkey has {} public signals, {circuit:?} has {}",
                z.n_public,
                circuit.n_public()
            )));
        }
        let n_vars = z.matrices.num_instance_variables + z.matrices.num_witness_variables;
        if calc.witness_size() != n_vars {
            return Err(Error::Circuit(format!(
                "wasm witness size {} != zkey nVars {n_vars}",
                calc.witness_size()
            )));
        }
        let domain = GeneralEvaluationDomain::<Fr>::new(z.matrices.num_constraints + z.matrices.num_instance_variables)
            .ok_or_else(|| Error::Zkey("domain too large".into()))?;
        if domain.size() != z.pk.h_query.len() {
            return Err(Error::Zkey("domain size does not match h_query".into()));
        }
        let pvk = prepare_verifying_key(&z.pk.vk);
        Ok(Self {
            circuit,
            calc,
            pk: z.pk,
            pvk,
            matrices: z.matrices,
        })
    }

    pub fn circuit(&self) -> Circuit {
        self.circuit
    }

    pub fn verifying_key(&self) -> &VerifyingKey<Bn254> {
        &self.pk.vk
    }

    /// The engine running the witness generator: `"webassembly"` (host engine, wasm32) or `"wasmi"`.
    pub fn witness_backend(&self) -> &'static str {
        self.calc.backend()
    }

    /// Full witness for raw `(signal, values)` inputs (exposed for differential testing).
    pub fn witness(&self, inputs: &[(&str, &[Fr])]) -> Result<zeroize::Zeroizing<Vec<Fr>>, Error> {
        self.calc.calculate(inputs)
    }

    fn prove_signals<R: RngCore + CryptoRng>(
        &self,
        inputs: &[(&str, &[Fr])],
        rng: &mut R,
    ) -> Result<ProofOutput, Error> {
        let w = self.calc.calculate(inputs)?;
        let mut r = Fr::rand(rng);
        let mut s = Fr::rand(rng);
        let proof = Groth16::<Bn254, CircomReduction>::create_proof_with_reduction_and_matrices(
            &self.pk,
            r,
            s,
            &self.matrices,
            self.matrices.num_instance_variables,
            self.matrices.num_constraints,
            &w,
        )
        .map_err(|e| Error::Proof(e.to_string()));
        r.zeroize();
        s.zeroize();
        let proof = proof?;
        let public: Vec<Fr> = w[1..=self.circuit.n_public()].to_vec();
        drop(w);
        // Self-check (3 pairings): never hand out a proof the chain would reject.
        let ok = Groth16::<Bn254, CircomReduction>::verify_proof(&self.pvk, &proof, &public)
            .map_err(|e| Error::Proof(e.to_string()))?;
        if !ok {
            return Err(Error::Proof(
                "generated proof does not verify (artifact mismatch?)".into(),
            ));
        }
        let public_signals: Vec<String> = public.iter().map(Fr::to_string).collect();
        let sp = SnarkjsProof::from_ark(&proof);
        let solidity = sp.to_solidity(&public_signals)?;
        Ok(ProofOutput {
            proof: sp,
            public_signals,
            solidity,
        })
    }
}

fn expect(p: &CircuitProver, c: Circuit) -> Result<(), Error> {
    if p.circuit == c {
        Ok(())
    } else {
        Err(Error::Circuit(format!("prover is for {:?}, not {c:?}", p.circuit)))
    }
}

/// Prove the `commitment` circuit (ragequit) with OS randomness.
pub fn prove_commitment(prover: &CircuitProver, inputs: &CommitmentInputs) -> Result<ProofOutput, Error> {
    prove_commitment_with_rng(prover, inputs, &mut rand_core::OsRng)
}

/// Prove the `withdraw` circuit with OS randomness.
pub fn prove_withdraw(prover: &CircuitProver, inputs: &WithdrawInputs) -> Result<ProofOutput, Error> {
    prove_withdraw_with_rng(prover, inputs, &mut rand_core::OsRng)
}

pub fn prove_commitment_with_rng<R: RngCore + CryptoRng>(
    prover: &CircuitProver,
    inputs: &CommitmentInputs,
    rng: &mut R,
) -> Result<ProofOutput, Error> {
    expect(prover, Circuit::Commitment)?;
    prover.prove_signals(&inputs.as_signals(), rng)
}

pub fn prove_withdraw_with_rng<R: RngCore + CryptoRng>(
    prover: &CircuitProver,
    inputs: &WithdrawInputs,
    rng: &mut R,
) -> Result<ProofOutput, Error> {
    expect(prover, Circuit::Withdraw)?;
    prover.prove_signals(&inputs.as_signals(), rng)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_parse() {
        assert_eq!(parse_strict::<Fr>("0").unwrap(), Fr::from(0u64));
        assert_eq!(parse_strict::<Fr>("0x10").unwrap(), Fr::from(16u64));
        assert!(parse_strict::<Fr>(&Fr::MODULUS.to_string()).is_err());
        assert!(parse_strict::<Fr>("-1").is_err());
        assert!(parse_strict::<Fr>("12a").is_err());
    }
}
