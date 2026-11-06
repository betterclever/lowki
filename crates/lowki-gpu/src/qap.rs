//! snarkjs' R1CS→QAP witness map ("H" scalars) on the GPU: for constraint evaluations `a`, `b`
//! over a domain of size n = 2^k with generator ω,
//!
//!   c = a ∘ b,  then for v in {a, b, c}: v ← FFT(g^i · IFFT(v)),  h = a ∘ b − c,
//!
//! with g the generator of the 2n domain, i.e. exactly what the CPU path
//! (`lowki-groth16`'s `CircomReduction::witness_map_from_matrices`) computes, as exact field
//! arithmetic, so the H MSM and the proof are byte-identical. Radix-2 decimation-in-time NTTs,
//! one dispatch per stage over all three vectors (WGSL in [`crate::shader::qap_module`]).
//!
//! `a`, `b`, the intermediate vectors and `h` are secret (witness-derived): every per-proof
//! buffer is zeroed on the GPU and destroyed afterwards, like the MSM buffers.

use ark_bn254::Fr;
use ark_ff::{BigInteger256, Field, PrimeField};
use zeroize::{Zeroize, Zeroizing};

use crate::field::{Conv, LIMBS, fr_words};
use crate::shader::qap_module;
use crate::{BigInts, Gpu, GpuError, PARAM_STRIDE, Pending, as_bytes, bind, grid, map_read};

pub(crate) struct QapPipes {
    layout: wgpu::BindGroupLayout,
    load: wgpu::ComputePipeline,
    bitrev: wgpu::ComputePipeline,
    stage: wgpu::ComputePipeline,
    coset: wgpu::ComputePipeline,
    finish: wgpu::ComputePipeline,
}

impl QapPipes {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("qap"),
            source: wgpu::ShaderSource::Wgsl(qap_module().into()),
        });
        let storage = |binding, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("qap"),
            entries: &[
                storage(0, true),
                storage(1, false),
                storage(2, true),
                storage(3, true),
                storage(4, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(32),
                    },
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("qap"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let make = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pl),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        QapPipes {
            load: make("qap_load"),
            bitrev: make("qap_bitrev"),
            stage: make("qap_stage"),
            coset: make("qap_coset"),
            finish: make("qap_finish"),
            layout,
        }
    }
}

/// Twiddles (ω^k, ω^-k for k < n/2) and coset factors (g^k / n) for one domain, resident on
/// the GPU (public data; 2 × n × 80 bytes).
pub struct QapTables {
    n: usize,
    tw: wgpu::Buffer,
    coset: wgpu::Buffer,
}

impl QapTables {
    pub fn domain_size(&self) -> usize {
        self.n
    }
    pub fn gpu_bytes(&self) -> u64 {
        self.tw.size() + self.coset.size()
    }
}

impl Drop for QapTables {
    fn drop(&mut self) {
        self.tw.destroy();
        self.coset.destroy();
    }
}

#[derive(Clone, Copy)]
enum Step {
    Load,
    Bitrev,
    Stage,
    Coset,
    Finish,
}

impl Gpu {
    /// Upload the tables for a domain of size `n` (a power of two ≥ 2) with generator `omega`;
    /// `g` is the generator of the domain of size 2n (the coset shift).
    pub fn qap_tables(&self, n: usize, omega: Fr, g: Fr) -> Result<QapTables, GpuError> {
        self.check_alive()?;
        if !n.is_power_of_two() || !(2..=1 << 27).contains(&n) {
            return Err(GpuError::Input(format!(
                "QAP domain size {n} is not a power of two in 2..=2^27"
            )));
        }
        if omega.pow([n as u64]) != Fr::ONE
            || omega.pow([n as u64 / 2]) == Fr::ONE
            || g.square() != omega
        {
            return Err(GpuError::Input(
                "omega / g are not the domain generators".into(),
            ));
        }
        let r260 = Fr::from(2u64).pow([260u64]);
        let mut tw = Vec::with_capacity(n * LIMBS);
        for inv in [false, true] {
            let w = if inv { omega.inverse().unwrap() } else { omega };
            let mut x = Fr::ONE;
            for _ in 0..n / 2 {
                tw.extend_from_slice(&fr_words(&x, &r260));
                x *= w;
            }
        }
        let n_inv = Fr::from(n as u64).inverse().unwrap();
        let mut coset = Vec::with_capacity(n * LIMBS);
        let mut x = n_inv;
        for _ in 0..n {
            coset.extend_from_slice(&fr_words(&x, &r260));
            x *= g;
        }
        let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let (tb, cb) = (
            self.buffer("qap twiddles", 4 * tw.len() as u64, usage)?,
            self.buffer("qap coset", 4 * coset.len() as u64, usage)?,
        );
        self.queue.write_buffer(&tb, 0, as_bytes(&tw));
        self.queue.write_buffer(&cb, 0, as_bytes(&coset));
        Ok(QapTables {
            n,
            tw: tb,
            coset: cb,
        })
    }

    /// Submit the witness map for constraint evaluations `a`, `b` (each of length n). Returns
    /// immediately; read `h` with [`QapPending::read`]. Work submitted later (e.g. MSMs) queues
    /// behind it.
    pub fn submit_qap(&self, t: &QapTables, a: &[Fr], b: &[Fr]) -> Result<QapPending, GpuError> {
        self.check_alive()?;
        let n = t.n;
        if a.len() != n || b.len() != n {
            return Err(GpuError::Input(format!("QAP inputs must have length {n}")));
        }
        let val = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let oom = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let mut pending = Pending {
            device: self.device.clone(),
            queue: self.queue.clone(),
            staging: None,
            secret: Vec::new(),
            jobs: Vec::new(),
            scopes: Vec::new(),
            conv: Conv::new(),
            cleaned: false,
        };
        let r = self.encode_qap(t, a, b, &mut pending);
        pending.scopes.push(Box::pin(oom.pop()));
        pending.scopes.push(Box::pin(val.pop()));
        r.map(|_| QapPending { inner: pending, n })
    }

    fn encode_qap(
        &self,
        t: &QapTables,
        a: &[Fr],
        b: &[Fr],
        pending: &mut Pending,
    ) -> Result<(), GpuError> {
        let (n, log_n) = (t.n as u32, t.n.trailing_zeros());
        let p = &self.qap;
        // arkworks' Montgomery words, a then b
        let mut raw: Zeroizing<Vec<u32>> = Zeroizing::new(Vec::with_capacity(16 * t.n));
        for x in a.iter().chain(b) {
            for l in x.0.0 {
                raw.extend_from_slice(&[l as u32, (l >> 32) as u32]);
            }
        }
        let secret = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST;
        let rb = self.buffer("qap raw", 4 * raw.len() as u64, secret)?;
        pending.secret.push((rb.clone(), rb.size()));
        self.queue.write_buffer(&rb, 0, as_bytes(&raw));
        raw.zeroize();
        let xb = self.buffer("qap vectors", 3 * 4 * (LIMBS as u64) * u64::from(n), secret)?;
        pending.secret.push((xb.clone(), xb.size()));
        let ob = self.buffer("qap h", 32 * u64::from(n), secret)?;
        pending.secret.push((ob.clone(), ob.size()));

        // [count, n, log_n, half, tw_stride, tw_off, 0, 0]
        let mut ops: Vec<(Step, [u32; 8])> = vec![(Step::Load, [n, n, log_n, 0, 0, 0, 0, 0])];
        for inverse in [true, false] {
            ops.push((Step::Bitrev, [3 * n, n, log_n, 0, 0, 0, 0, 0]));
            for s in 1..=log_n {
                let half = 1u32 << (s - 1);
                let tw_off = if inverse { n / 2 } else { 0 };
                ops.push((
                    Step::Stage,
                    [3 * n / 2, n, log_n, half, n / (2 * half), tw_off, 0, 0],
                ));
            }
            if inverse {
                ops.push((Step::Coset, [3 * n, n, log_n, 0, 0, 0, 0, 0]));
            }
        }
        ops.push((Step::Finish, [n, n, log_n, 0, 0, 0, 0, 0]));
        let mut pbytes = vec![0u8; ops.len() * PARAM_STRIDE as usize];
        for (i, (_, prm)) in ops.iter().enumerate() {
            let o = i * PARAM_STRIDE as usize;
            pbytes[o..o + 32].copy_from_slice(as_bytes(prm));
        }
        let pbuf = self.buffer(
            "qap params",
            pbytes.len() as u64,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        )?;
        self.queue.write_buffer(&pbuf, 0, &pbytes);
        pending.secret.push((pbuf.clone(), pbuf.size()));
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("qap"),
            layout: &p.layout,
            entries: &[
                bind(0, &rb),
                bind(1, &xb),
                bind(2, &t.tw),
                bind(3, &t.coset),
                bind(4, &ob),
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &pbuf,
                        offset: 0,
                        size: wgpu::BufferSize::new(32),
                    }),
                },
            ],
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            // each dispatch is its own usage scope, so every step sees the previous one's writes
            let mut pass = enc.begin_compute_pass(&Default::default());
            for (slot, (k, prm)) in ops.iter().enumerate() {
                pass.set_pipeline(match k {
                    Step::Load => &p.load,
                    Step::Bitrev => &p.bitrev,
                    Step::Stage => &p.stage,
                    Step::Coset => &p.coset,
                    Step::Finish => &p.finish,
                });
                pass.set_bind_group(0, &bg, &[slot as u32 * PARAM_STRIDE as u32]);
                let (x, y) = grid(prm[0]);
                pass.dispatch_workgroups(x, y, 1);
            }
        }
        let staging = self.buffer(
            "qap staging",
            ob.size(),
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        )?;
        enc.copy_buffer_to_buffer(&ob, 0, &staging, 0, ob.size());
        pending.staging = Some((staging, ob.size()));
        self.queue.submit([enc.finish()]);
        Ok(())
    }
}

/// A submitted QAP witness map. Call [`QapPending::read`] for `h`, then [`QapPending::finish`]
/// (after submitting follow-up work) to wipe the buffers and surface GPU errors. If dropped,
/// the buffers are still zeroed and destroyed.
pub struct QapPending {
    inner: Pending,
    n: usize,
}

impl QapPending {
    /// The `h` scalars (canonical), as soon as the QAP kernels are done.
    pub async fn read(&mut self) -> Result<BigInts, GpuError> {
        let Some((staging, size)) = &self.inner.staging else {
            return Err(GpuError::Device("nothing submitted".into()));
        };
        let mut words = Zeroizing::new(map_read(&self.inner.device, staging, *size).await?);
        let mut h = BigInts(Vec::with_capacity(self.n));
        for w in words.as_chunks::<8>().0 {
            let l = |i: usize| u64::from(w[2 * i]) | u64::from(w[2 * i + 1]) << 32;
            let x = BigInteger256::new([l(0), l(1), l(2), l(3)]);
            if x >= Fr::MODULUS {
                return Err(GpuError::Device(
                    "GPU returned an unreduced QAP scalar".into(),
                ));
            }
            h.0.push(x);
        }
        words.zeroize();
        if h.0.len() != self.n {
            return Err(GpuError::Device("short QAP readback".into()));
        }
        Ok(h)
    }

    /// Zero and release the per-proof buffers; reports any GPU error from the QAP kernels.
    pub async fn finish(self) -> Result<(), GpuError> {
        self.inner.settle(Ok(())).await
    }
}
