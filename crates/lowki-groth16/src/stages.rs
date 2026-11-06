//! The Groth16 prover split into its stages: witness → H witness map → five MSMs →
//! assembly. Used for profiling ([`CircuitProver::profile_with_rng`]) and by the WebGPU
//! prover (feature `webgpu`), which swaps the G1 MSMs for GPU ones.
//!
//! The arithmetic is exactly ark-groth16 0.5 `create_proof_with_assignment`:
//!
//! ```text
//! A = α + Σ_i w_i·a_query[i] + r·δ                      (w_0 = 1, i over all variables)
//! B = β + Σ_i w_i·b_g2_query[i] + s·δ  (G2), and the same in G1 with b_g1_query
//! C = s·A + r·B_G1 − r·s·δ + Σ_j w_{ℓ+j}·l_query[j] + Σ_k h_k·h_query[k]
//! ```
//!
//! ark computes `query[0] + MSM(query[1..], w[1..])`; with `w_0 = 1` that equals
//! `MSM(query, w)`, and every result is compared as an affine point, so the output is the
//! same proof.

use ark_bn254::{Fr, G1Projective, G2Projective};
use ark_ec::{CurveGroup, VariableBaseMSM};
use ark_ff::{BigInteger256, PrimeField, Zero};
use ark_groth16::r1cs_to_qap::R1CSToQAP;
use ark_groth16::{Groth16, Proof};
use ark_poly::GeneralEvaluationDomain;
use ark_std::rand::{CryptoRng, RngCore};
use ark_std::UniformRand;
use zeroize::{Zeroize, Zeroizing};

use crate::{Bn254, CircomReduction, CircuitProver, Error};

/// `(stage, milliseconds)` pairs from the profiling entry points.
pub type StageTimes = Vec<(&'static str, f64)>;

/// The five MSM results of one proof (projective, before blinding).
pub(crate) struct Msms {
    /// Σ w_i·a_query[i] (all variables, w_0 = 1).
    pub a: G1Projective,
    /// Σ w_i·b_g1_query[i].
    pub b1: G1Projective,
    /// Σ w_i·b_g2_query[i].
    pub b2: G2Projective,
    /// Σ w_{ℓ+j}·l_query[j] (witness variables only).
    pub l: G1Projective,
    /// Σ h_k·h_query[k].
    pub h: G1Projective,
}

/// Canonical (non-Montgomery) scalars, zeroized on drop.
pub(crate) struct Bigints(pub Vec<BigInteger256>);

impl Drop for Bigints {
    fn drop(&mut self) {
        for b in self.0.iter_mut() {
            b.0.zeroize();
        }
    }
}

pub(crate) fn bigints(s: &[Fr]) -> Bigints {
    Bigints(s.iter().map(|x| x.into_bigint()).collect())
}

impl CircuitProver {
    /// snarkjs' R1CS→QAP witness map: the `h` scalars for `h_query` (secret).
    pub(crate) fn witness_map(&self, w: &[Fr]) -> Result<Zeroizing<Vec<Fr>>, Error> {
        CircomReduction::witness_map_from_matrices::<Fr, GeneralEvaluationDomain<Fr>>(
            &self.matrices,
            self.matrices.num_instance_variables,
            self.matrices.num_constraints,
            w,
        )
        .map(Zeroizing::new)
        .map_err(|e| Error::Proof(e.to_string()))
    }

    /// The witness map's starting vectors `a`, `b` (secret); the GPU runs the rest of it.
    #[cfg(feature = "webgpu")]
    pub(crate) fn qap_inputs(&self, w: &[Fr]) -> (Zeroizing<Vec<Fr>>, Zeroizing<Vec<Fr>>) {
        let m = &self.matrices;
        let (a, b) = crate::qap::constraint_evaluations(
            m,
            m.num_instance_variables,
            m.num_constraints,
            w,
            self.pk.h_query.len(),
        );
        (Zeroizing::new(a), Zeroizing::new(b))
    }

    /// The parsed proving key (exposed for differential tests of GPU MSMs).
    #[doc(hidden)]
    pub fn proving_key(&self) -> &ark_groth16::ProvingKey<Bn254> {
        &self.pk
    }

    pub(crate) fn num_instance(&self) -> usize {
        self.matrices.num_instance_variables
    }

    /// Combine the MSM results with α, β, δ and the blinding factors `r`, `s`.
    pub(crate) fn assemble(&self, r: Fr, s: Fr, m: &Msms) -> Proof<Bn254> {
        let pk = &self.pk;
        let a = m.a + pk.delta_g1 * r + pk.vk.alpha_g1;
        let b1 = if r.is_zero() {
            G1Projective::zero()
        } else {
            m.b1 + pk.delta_g1 * s + pk.beta_g1
        };
        let b2 = m.b2 + pk.vk.delta_g2 * s + pk.vk.beta_g2;
        let mut rs = r * s;
        let c = a * s + b1 * r - pk.delta_g1 * rs + m.l + m.h;
        rs.zeroize();
        Proof {
            a: a.into_affine(),
            b: b2.into_affine(),
            c: c.into_affine(),
        }
    }

    /// Self-check (3 pairings) and conversion to the output formats.
    pub(crate) fn finish(&self, proof: &Proof<Bn254>, w: &[Fr]) -> Result<crate::ProofOutput, Error> {
        let public: Vec<Fr> = w[1..=self.circuit.n_public()].to_vec();
        let ok = Groth16::<Bn254, CircomReduction>::verify_proof(&self.pvk, proof, &public)
            .map_err(|e| Error::Proof(e.to_string()))?;
        if !ok {
            return Err(Error::Proof(
                "generated proof does not verify (artifact mismatch?)".into(),
            ));
        }
        let public_signals: Vec<String> = public.iter().map(Fr::to_string).collect();
        let sp = crate::SnarkjsProof::from_ark(proof);
        let solidity = sp.to_solidity(&public_signals)?;
        Ok(crate::ProofOutput {
            proof: sp,
            public_signals,
            solidity,
        })
    }

    /// Prove with the stages run one by one on the CPU and timed with `now` (milliseconds).
    /// Returns the proof (identical to the default path for the same RNG) and
    /// `(stage, ms)` pairs. For profiling only; the default path is [`crate::prove_withdraw`].
    #[doc(hidden)]
    pub fn profile_with_rng<R: RngCore + CryptoRng>(
        &self,
        inputs: &[(&str, &[Fr])],
        rng: &mut R,
        now: &dyn Fn() -> f64,
    ) -> Result<(crate::ProofOutput, StageTimes), Error> {
        let mut t = now();
        let mut times = Vec::new();
        let mut lap = |name: &'static str, times: &mut Vec<(&'static str, f64)>| {
            let n = now();
            times.push((name, n - t));
            t = n;
        };
        let w = self.calc.calculate(inputs)?;
        lap("witness", &mut times);
        let mut r = Fr::rand(rng);
        let mut s = Fr::rand(rng);
        let h = self.witness_map(&w)?;
        lap("H witness map (FFTs)", &mut times);
        let wb = bigints(&w);
        let hb = bigints(&h);
        drop(h);
        lap("scalars to bigint", &mut times);
        let pk = &self.pk;
        let a = G1Projective::msm_bigint(&pk.a_query, &wb.0);
        lap("MSM A (G1)", &mut times);
        let b2 = G2Projective::msm_bigint(&pk.b_g2_query, &wb.0);
        lap("MSM B (G2)", &mut times);
        let b1 = G1Projective::msm_bigint(&pk.b_g1_query, &wb.0);
        lap("MSM B (G1)", &mut times);
        let l = G1Projective::msm_bigint(&pk.l_query, &wb.0[self.num_instance()..]);
        lap("MSM C/L (G1)", &mut times);
        let hm = G1Projective::msm_bigint(&pk.h_query, &hb.0);
        lap("MSM H (G1)", &mut times);
        let proof = self.assemble(r, s, &Msms { a, b1, b2, l, h: hm });
        r.zeroize();
        s.zeroize();
        lap("assemble", &mut times);
        let out = self.finish(&proof, &w)?;
        lap("self-check (pairings)", &mut times);
        Ok((out, times))
    }
}
