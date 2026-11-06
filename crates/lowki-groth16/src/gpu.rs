//! WebGPU proving (feature `webgpu`): the same Groth16 proof as the CPU path, with the four
//! G1 MSMs (A, B_G1, L, H) and the G2 MSM (B) on the GPU via `lowki-gpu`.
//!
//! The witness (wasmi) and the H witness map (FFTs) stay on the CPU; the H-independent MSMs
//! are submitted *before* the FFTs, so the GPU runs them while the CPU computes `h`.
//! With the same RNG the proof is byte-identical to [`crate::prove_withdraw_with_rng`]; it is
//! pairing-checked before it is returned. On any GPU failure the caller gets
//! [`Error::Gpu`] and should fall back to the CPU functions (via [`GpuProver::cpu`]).

use std::rc::Rc;

use ark_ff::UniformRand;
use ark_std::rand::{CryptoRng, RngCore};
use lowki_gpu::{G1, G2, Gpu, GpuBases, MsmJob};
use zeroize::Zeroize;

pub use lowki_gpu::{GpuError, AdapterInfo};

use crate::stages::{Msms, bigints};
use crate::{Circuit, CircuitProver, CommitmentInputs, Error, Fr, ProofOutput, WithdrawInputs, expect};

/// A [`CircuitProver`] with its proving-key queries resident on the GPU.
pub struct GpuProver {
    cpu: CircuitProver,
    gpu: Rc<Gpu>,
    a: GpuBases<G1>,
    b1: GpuBases<G1>,
    b2: GpuBases<G2>,
    l: GpuBases<G1>,
    h: GpuBases<G1>,
}

/// Open the GPU (async because WebGPU adapter/device requests are). Returns
/// `Err(GpuError::Unavailable)` when there is no usable WebGPU.
pub async fn open_gpu() -> Result<Rc<Gpu>, GpuError> {
    Gpu::new().await.map(Rc::new)
}

impl GpuProver {
    /// Upload the proving-key queries (one-time cost per circuit). On error the
    /// `CircuitProver` is handed back so the caller can keep proving on the CPU.
    #[allow(clippy::result_large_err)]
    pub fn new(gpu: Rc<Gpu>, cpu: CircuitProver) -> Result<Self, (CircuitProver, GpuError)> {
        let up = || -> Result<_, GpuError> {
            let pk = &cpu.pk;
            Ok((
                gpu.upload_bases(&pk.a_query)?,
                gpu.upload_bases(&pk.b_g1_query)?,
                gpu.upload_bases(&pk.b_g2_query)?,
                gpu.upload_bases(&pk.l_query)?,
                gpu.upload_bases(&pk.h_query)?,
            ))
        };
        match up() {
            Ok((a, b1, b2, l, h)) => Ok(Self { cpu, gpu, a, b1, b2, l, h }),
            Err(e) => Err((cpu, e)),
        }
    }

    /// The CPU prover (for fallback, verifying key, witness).
    pub fn cpu(&self) -> &CircuitProver {
        &self.cpu
    }

    pub fn into_cpu(self) -> CircuitProver {
        self.cpu
    }

    pub fn gpu(&self) -> &Gpu {
        &self.gpu
    }

    /// GPU memory held by the resident bases (bytes).
    pub fn gpu_bytes(&self) -> u64 {
        self.a.gpu_bytes() + self.b1.gpu_bytes() + self.b2.gpu_bytes() + self.l.gpu_bytes() + self.h.gpu_bytes()
    }

    /// Override the Pippenger window size of every MSM (tuning/benchmarks).
    #[doc(hidden)]
    pub fn set_window_bits(&mut self, c: u32) {
        self.a.set_window_bits(c);
        self.b1.set_window_bits(c);
        self.b2.set_window_bits(c);
        self.l.set_window_bits(c);
        self.h.set_window_bits(c);
    }

    async fn prove_signals<R: RngCore + CryptoRng>(
        &self,
        inputs: &[(&str, &[Fr])],
        rng: &mut R,
        mut lap: impl FnMut(&'static str),
    ) -> Result<ProofOutput, Error> {
        let p = &self.cpu;
        let w = p.calc.calculate(inputs)?;
        lap("witness");
        // same draw order as the CPU path (r, then s)
        let mut r = Fr::rand(rng);
        let mut s = Fr::rand(rng);
        let res = async {
            let wb = bigints(&w);
            let ni = p.num_instance();
            let first = self
                .gpu
                .submit(&[
                    MsmJob::G1(&self.a, &wb.0),
                    MsmJob::G2(&self.b2, &wb.0),
                    MsmJob::G1(&self.b1, &wb.0),
                    MsmJob::G1(&self.l, &wb.0[ni..]),
                ])
                .map_err(Error::Gpu)?;
            lap("plan+submit A,B,L");
            // CPU computes h while the GPU works on A, B, L.
            let h = p.witness_map(&w)?;
            let hb = bigints(&h);
            drop(h);
            lap("H witness map (CPU, overlapped)");
            let second = self.gpu.submit(&[MsmJob::G1(&self.h, &hb.0)]).map_err(Error::Gpu)?;
            drop(hb);
            let o1 = first.wait().await.map_err(Error::Gpu)?;
            let o2 = second.wait().await.map_err(Error::Gpu)?;
            lap("GPU MSMs (wait)");
            let bad = || Error::Gpu(GpuError::Device("unexpected MSM output".into()));
            Ok::<_, Error>(Msms {
                a: o1[0].g1().ok_or_else(bad)?,
                b2: o1[1].g2().ok_or_else(bad)?,
                b1: o1[2].g1().ok_or_else(bad)?,
                l: o1[3].g1().ok_or_else(bad)?,
                h: o2[0].g1().ok_or_else(bad)?,
            })
        }
        .await;
        let out = res.map(|m| p.assemble(r, s, &m));
        r.zeroize();
        s.zeroize();
        let proof = out?;
        // A proof that fails the pairing self-check here means the GPU computed something wrong
        // (the CPU path uses the same assembly): report it as a GPU error so callers fall back.
        let o = p.finish(&proof, &w).map_err(|e| match e {
            Error::Proof(m) => Error::Gpu(GpuError::Device(format!("GPU proof rejected by self-check: {m}"))),
            e => e,
        });
        lap("assemble+self-check");
        o
    }

    /// Prove `withdraw` on the GPU with OS randomness.
    pub async fn prove_withdraw(&self, inputs: &WithdrawInputs) -> Result<ProofOutput, Error> {
        self.prove_withdraw_with_rng(inputs, &mut rand_core::OsRng).await
    }

    /// Prove `commitment` on the GPU with OS randomness.
    pub async fn prove_commitment(&self, inputs: &CommitmentInputs) -> Result<ProofOutput, Error> {
        self.prove_commitment_with_rng(inputs, &mut rand_core::OsRng).await
    }

    pub async fn prove_withdraw_with_rng<R: RngCore + CryptoRng>(
        &self,
        inputs: &WithdrawInputs,
        rng: &mut R,
    ) -> Result<ProofOutput, Error> {
        expect(&self.cpu, Circuit::Withdraw)?;
        self.prove_signals(&inputs.as_signals(), rng, |_| {}).await
    }

    pub async fn prove_commitment_with_rng<R: RngCore + CryptoRng>(
        &self,
        inputs: &CommitmentInputs,
        rng: &mut R,
    ) -> Result<ProofOutput, Error> {
        expect(&self.cpu, Circuit::Commitment)?;
        self.prove_signals(&inputs.as_signals(), rng, |_| {}).await
    }

    /// Like `prove_*_with_rng`, also returning per-stage timings measured with `now` (ms).
    #[doc(hidden)]
    pub async fn profile_with_rng<R: RngCore + CryptoRng>(
        &self,
        inputs: &[(&str, &[Fr])],
        rng: &mut R,
        now: &dyn Fn() -> f64,
    ) -> Result<(ProofOutput, crate::StageTimes), Error> {
        let mut t = now();
        let mut times = Vec::new();
        let out = self
            .prove_signals(inputs, rng, |name| {
                let n = now();
                times.push((name, n - t));
                t = n;
            })
            .await?;
        Ok((out, times))
    }
}

/// Prove `withdraw` on the GPU (OS randomness).
pub async fn prove_withdraw_gpu(prover: &GpuProver, inputs: &WithdrawInputs) -> Result<ProofOutput, Error> {
    prover.prove_withdraw(inputs).await
}

/// Prove `commitment` on the GPU (OS randomness).
pub async fn prove_commitment_gpu(prover: &GpuProver, inputs: &CommitmentInputs) -> Result<ProofOutput, Error> {
    prover.prove_commitment(inputs).await
}
