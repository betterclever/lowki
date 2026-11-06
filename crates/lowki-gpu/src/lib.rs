//! WebGPU multi-scalar multiplication for BN254 G1 and G2 (wgpu + WGSL), for the Lowki
//! Groth16 prover. Runs natively (Metal / Vulkan) and in the browser (wasm32 + WebGPU).
//!
//! ```ignore
//! let gpu = Gpu::new().await?;                         // Err(GpuError::Unavailable) -> use the CPU
//! let bases = gpu.upload_bases(&pk.a_query)?;          // once; stays resident on the GPU
//! let pending = gpu.submit(&[MsmJob::G1(&bases, &scalars)])?;   // scalars: canonical BigInts
//! // ... CPU work overlaps with the GPU here ...
//! let out = pending.wait().await?;                     // Vec<MsmOutput>
//! ```
//!
//! Design (see README for the full story):
//! * Fq is 20 × 13-bit limbs in `u32`s, Montgomery with R = 2^260 ([`field`]).
//! * Pippenger with signed-digit windows. The CPU computes the digits and a counting sort of
//!   the (window, bucket) references and splits every bucket into tasks of at most
//!   [`plan::TASK_MAX`] points ([`plan`]). The GPU sums the tasks level by level with complete
//!   projective formulas, then turns buckets into two points per window with in-place running
//!   sums (chunks of √B buckets, then chunk sums). Every kernel is straight-line with at most
//!   one point addition per thread; the host issues one dispatch per sequential step (the
//!   Apple GPU compiler miscompiled loops around point additions, see README). The CPU
//!   combines the `2·W` window outputs (a few hundred doublings).
//! * Bases are uploaded once ([`GpuBases`]). Per MSM only scalar-derived data is uploaded;
//!   all of it (and the GPU scratch holding bucket sums) is overwritten with zeros after
//!   the result is read, then destroyed. CPU copies are zeroized.

pub mod field;
pub mod plan;
pub mod shader;

use core::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ark_bn254::{Fr, G1Projective, G2Projective};
use ark_ec::short_weierstrass::{Affine, Projective, SWCurveConfig};
use ark_ec::{AdditiveGroup, CurveGroup};
use ark_ff::{BigInteger256, Field, PrimeField, Zero};
use zeroize::{Zeroize, Zeroizing};

use field::{Conv, GpuField};
use plan::Plan;
pub use shader::CurveKind;
pub use wgpu::AdapterInfo;

/// Errors. Every failure to get or keep a working GPU is [`GpuError::Unavailable`] or
/// [`GpuError::Device`]; callers should fall back to the CPU prover on either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuError {
    /// No WebGPU / no suitable adapter / device request failed / device lost.
    Unavailable(String),
    /// The GPU reported an error (validation, out of memory, mapping) or returned garbage.
    Device(String),
    /// Bad arguments (e.g. more scalars than bases, buffer over the device limit).
    Input(String),
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GpuError::Unavailable(m) => write!(f, "GPU unavailable: {m}"),
            GpuError::Device(m) => write!(f, "GPU error: {m}"),
            GpuError::Input(m) => write!(f, "invalid GPU MSM input: {m}"),
        }
    }
}

impl std::error::Error for GpuError {}

/// A curve the kernels support: BN254 G1 (`ark_bn254::g1::Config`) or G2.
pub trait GpuCurve: SWCurveConfig<ScalarField = Fr>
where
    Self::BaseField: GpuField,
{
    const KIND: CurveKind;
}

impl GpuCurve for ark_bn254::g1::Config {
    const KIND: CurveKind = CurveKind::G1;
}

impl GpuCurve for ark_bn254::g2::Config {
    const KIND: CurveKind = CurveKind::G2;
}

/// G1 alias.
pub type G1 = ark_bn254::g1::Config;
/// G2 alias.
pub type G2 = ark_bn254::g2::Config;

#[derive(Clone, Copy)]
enum Kernel {
    AccBase,
    AccScr,
    Run,
    Sum,
}

struct Pipes {
    layout: wgpu::BindGroupLayout,
    selftest: wgpu::ComputePipeline,
    acc_base_step: wgpu::ComputePipeline,
    acc_scr_step: wgpu::ComputePipeline,
    run_step: wgpu::ComputePipeline,
    sum_step: wgpu::ComputePipeline,
}

/// A WebGPU device with the MSM pipelines compiled.
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    info: wgpu::AdapterInfo,
    limits: wgpu::Limits,
    g1: Pipes,
    g2: Pipes,
    lost: Arc<AtomicBool>,
    conv: Conv,
    debug_full_scratch: AtomicBool,
}

const WG: u32 = shader::WORKGROUP_SIZE;
/// Native: give up on a submission that has not finished after this long.
#[cfg(not(target_arch = "wasm32"))]
const GPU_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const PARAM_STRIDE: u64 = 256;

fn unavailable(e: impl fmt::Display) -> GpuError {
    GpuError::Unavailable(e.to_string())
}

/// Wait for a wgpu callback: natively by polling the device, on the web by yielding to the
/// browser's event loop.
async fn recv<T>(device: &wgpu::Device, rx: futures_channel::oneshot::Receiver<T>) -> Result<T, GpuError> {
    #[cfg(not(target_arch = "wasm32"))]
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(GPU_TIMEOUT),
        })
        .map_err(|e| GpuError::Device(format!("poll: {e}")))?;
    #[cfg(target_arch = "wasm32")]
    let _ = device;
    rx.await
        .map_err(|_| GpuError::Unavailable("GPU callback dropped (device lost?)".into()))
}

impl Gpu {
    /// Request a high-performance adapter and device and compile the G1/G2 MSM pipelines.
    /// Returns [`GpuError::Unavailable`] when there is no WebGPU, no adapter (or only a CPU
    /// software adapter), or the device request fails.
    pub async fn new() -> Result<Gpu, GpuError> {
        #[cfg(target_arch = "wasm32")]
        if !wgpu::util::is_browser_webgpu_supported().await {
            return Err(GpuError::Unavailable("WebGPU not available in this browser context".into()));
        }
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::PRIMARY;
        let instance = wgpu::Instance::new(desc);
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                ..Default::default()
            })
            .await
            .map_err(unavailable)?;
        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu {
            return Err(GpuError::Unavailable(format!(
                "only a CPU software adapter is available ({})",
                info.name
            )));
        }
        let al = adapter.limits();
        let limits = wgpu::Limits {
            max_storage_buffer_binding_size: al.max_storage_buffer_binding_size,
            max_buffer_size: al.max_buffer_size,
            ..Default::default()
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("lowki-gpu"),
                required_features: wgpu::Features::empty(),
                required_limits: limits.clone(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(unavailable)?;
        let lost = Arc::new(AtomicBool::new(false));
        {
            let lost = lost.clone();
            device.set_device_lost_callback(move |_, _| lost.store(true, Ordering::SeqCst));
        }
        // Errors outside our error scopes would otherwise panic; treat them as a lost device.
        {
            let lost = lost.clone();
            device.on_uncaptured_error(Arc::new(move |_| lost.store(true, Ordering::SeqCst)));
        }
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let g1 = Self::pipes(&device, CurveKind::G1);
        let g2 = Self::pipes(&device, CurveKind::G2);
        if let Some(e) = scope.pop().await {
            return Err(GpuError::Unavailable(format!("pipeline creation failed: {e}")));
        }
        Ok(Gpu {
            device,
            queue,
            info,
            limits,
            g1,
            g2,
            lost,
            conv: Conv::new(),
            debug_full_scratch: AtomicBool::new(false),
        })
    }

    fn pipes(device: &wgpu::Device, kind: CurveKind) -> Pipes {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(match kind {
                CurveKind::G1 => "msm-g1",
                CurveKind::G2 => "msm-g2",
            }),
            source: wgpu::ShaderSource::Wgsl(shader::msm_module(kind).into()),
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
            label: Some("msm"),
            entries: &[
                storage(0, true),
                storage(1, true),
                storage(2, true),
                storage(3, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
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
            label: Some("msm"),
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
        Pipes {
            acc_base_step: make("acc_base_step"),
            acc_scr_step: make("acc_scr_step"),
            run_step: make("run_step"),
            sum_step: make("sum_step"),
            selftest: make("point_selftest"),
            layout,
        }
    }

    /// Adapter name, vendor, backend, driver.
    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.info
    }

    /// `maxStorageBufferBindingSize` granted for this device.
    pub fn max_storage_binding(&self) -> u64 {
        self.limits.max_storage_buffer_binding_size
    }

    fn check_alive(&self) -> Result<(), GpuError> {
        if self.lost.load(Ordering::SeqCst) {
            Err(GpuError::Unavailable("GPU device lost".into()))
        } else {
            Ok(())
        }
    }

    fn buffer(&self, label: &str, size: u64, usage: wgpu::BufferUsages) -> Result<wgpu::Buffer, GpuError> {
        let size = size.max(16).next_multiple_of(4);
        if usage.contains(wgpu::BufferUsages::STORAGE) && size > self.limits.max_storage_buffer_binding_size {
            return Err(GpuError::Input(format!(
                "{label}: {size} bytes exceeds maxStorageBufferBindingSize {}",
                self.limits.max_storage_buffer_binding_size
            )));
        }
        if size > self.limits.max_buffer_size {
            return Err(GpuError::Input(format!("{label}: {size} bytes exceeds maxBufferSize")));
        }
        Ok(self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage,
            mapped_at_creation: false,
        }))
    }

    /// Upload fixed bases (e.g. a proving-key query) once; they stay resident until dropped.
    /// Points at infinity are allowed (their scalars are ignored).
    pub fn upload_bases<P: GpuCurve>(&self, bases: &[Affine<P>]) -> Result<GpuBases<P>, GpuError>
    where
        P::BaseField: GpuField,
    {
        self.check_alive()?;
        if bases.len() >= 1 << 31 {
            return Err(GpuError::Input("too many bases".into()));
        }
        let fw = P::KIND.fw();
        let mut words = Vec::with_capacity(bases.len() * 2 * fw);
        let mut skip = Vec::with_capacity(bases.len());
        for b in bases {
            skip.push(b.infinity);
            if b.infinity {
                words.extend(std::iter::repeat_n(0u32, 2 * fw));
            } else {
                b.x.push_words(&self.conv, &mut words);
                b.y.push_words(&self.conv, &mut words);
            }
        }
        let buf = self.buffer(
            "bases",
            4 * words.len() as u64,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        )?;
        self.queue.write_buffer(&buf, 0, as_bytes(&words));
        Ok(GpuBases {
            buf,
            n: bases.len(),
            skip,
            c: plan::window_bits(bases.len()),
            _p: core::marker::PhantomData,
        })
    }

    /// Encode and submit a batch of MSMs. The GPU runs while the caller continues; collect
    /// the results with [`Pending::wait`].
    pub fn submit(&self, jobs: &[MsmJob<'_>]) -> Result<Pending, GpuError> {
        self.check_alive()?;
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
        let r = self.encode(jobs, &mut pending);
        // pop in reverse push order
        pending.scopes.push(Box::pin(oom.pop()));
        pending.scopes.push(Box::pin(val.pop()));
        r.map(|_| pending)
    }

    fn encode(&self, jobs: &[MsmJob<'_>], pending: &mut Pending) -> Result<(), GpuError> {
        let mut enc = self.device.create_command_encoder(&Default::default());
        let mut copies = Vec::new();
        let mut staging_words = 0u64;
        for job in jobs {
            let (kind, bases_buf, n, skip, c, scalars) = match job {
                MsmJob::G1(b, s) => (CurveKind::G1, &b.buf, b.n, &b.skip[..], b.c, *s),
                MsmJob::G2(b, s) => (CurveKind::G2, &b.buf, b.n, &b.skip[..], b.c, *s),
            };
            if scalars.len() > n {
                return Err(GpuError::Input(format!("{} scalars for {n} bases", scalars.len())));
            }
            if scalars.iter().any(|s| *s >= Fr::MODULUS) {
                return Err(GpuError::Input("scalar not reduced mod r".into()));
            }
            let pipes = match kind {
                CurveKind::G1 => &self.g1,
                CurveKind::G2 => &self.g2,
            };
            let plan = Plan::new(scalars, skip, c);
            let fw = kind.fw() as u64;
            let point_bytes = 3 * fw * 4;

            let entries = self.buffer(
                "entries",
                4 * plan.entries.len() as u64,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            )?;
            pending.secret.push((entries.clone(), entries.size()));
            self.queue.write_buffer(&entries, 0, as_bytes(&plan.entries));
            let tasks = self.buffer(
                "tasks",
                16 * plan.tasks.len() as u64,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            )?;
            pending.secret.push((tasks.clone(), tasks.size()));
            {
                let mut flat: Zeroizing<Vec<u32>> = Zeroizing::new(plan.tasks.iter().flatten().copied().collect());
                if !flat.is_empty() {
                    self.queue.write_buffer(&tasks, 0, as_bytes(&flat));
                }
                flat.zeroize();
            }
            // The dispatch sequence; every kernel does at most one point addition per thread.
            // (kernel, [off, count, a, b, c, d, e, f]); f is the step.
            let (m, nc, nw) = (plan.chunk, plan.chunks, plan.windows);
            let wc = nw * nc;
            let mut ops: Vec<(Kernel, [u32; 8])> = Vec::new();
            // level 0 sums signed bases, levels >= 1 sum the previous level's partials
            for (lvl, (&(off, _), st)) in plan.levels.iter().zip(&plan.steps).enumerate() {
                let kern = if lvl == 0 { Kernel::AccBase } else { Kernel::AccScr };
                for (k, &cnt) in st.iter().enumerate() {
                    ops.push((kern, [off, cnt, 0, 0, 0, 0, 0, k as u32]));
                }
            }
            // running sums inside each chunk of m buckets (in place)
            for f in 1..m {
                ops.push((Kernel::Run, [0, wc, m, m, 0, 0, 0, f]));
            }
            // U_i = Σ_k running sums; T_i = first running sum
            for f in 0..m {
                ops.push((Kernel::Sum, [0, wc, m, 0, 0, plan.u_slot, 1, f]));
            }
            ops.push((Kernel::Sum, [0, wc, m, 0, 0, plan.t_slot, 1, 0]));
            // suffix sums of T inside each window (in place)
            for f in 1..nc {
                ops.push((Kernel::Run, [0, nw, nc, nc, plan.t_slot, 0, 0, f]));
            }
            // out[2w] = Σ_j U_j, out[2w+1] = Σ_{k≥1} suffix_k  (CPU: out[2w] + m·out[2w+1])
            for f in 0..nc {
                ops.push((Kernel::Sum, [0, nw, nc, 0, plan.u_slot, plan.out_slot, 2, f]));
            }
            for f in 0..nc.saturating_sub(1) {
                ops.push((Kernel::Sum, [0, nw, nc, 0, plan.t_slot + 1, plan.out_slot + 1, 2, f]));
            }
            ops.retain(|(_, p)| p[1] > 0);
            let params: Vec<[u32; 8]> = ops.iter().map(|(_, p)| *p).collect();
            let mut pbytes = Zeroizing::new(vec![0u8; params.len() * PARAM_STRIDE as usize]);
            for (i, p) in params.iter().enumerate() {
                let o = i * PARAM_STRIDE as usize;
                pbytes[o..o + 32].copy_from_slice(as_bytes(p));
            }
            let pbuf = self.buffer(
                "params",
                pbytes.len() as u64,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            )?;
            self.queue.write_buffer(&pbuf, 0, &pbytes);
            pending.secret.push((pbuf.clone(), pbuf.size()));
            let scratch = self.buffer(
                "scratch",
                u64::from(plan.slots) * point_bytes,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            )?;
            pending.secret.push((scratch.clone(), scratch.size()));

            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("msm"),
                layout: &pipes.layout,
                entries: &[
                    bind(0, bases_buf),
                    bind(1, &entries),
                    bind(2, &tasks),
                    bind(3, &scratch),
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &pbuf,
                            offset: 0,
                            size: wgpu::BufferSize::new(32),
                        }),
                    },
                ],
            });
            // empty buckets (and unwritten outputs) must read as the point at infinity
            enc.clear_buffer(&scratch, 0, None);
            {
                // WebGPU orders dispatches within a pass (each is its own usage scope), so each
                // step sees the previous step's storage writes.
                let mut pass = enc.begin_compute_pass(&Default::default());
                for (slot, (k, p)) in ops.iter().enumerate() {
                    pass.set_pipeline(match k {
                        Kernel::AccBase => &pipes.acc_base_step,
                        Kernel::AccScr => &pipes.acc_scr_step,
                        Kernel::Run => &pipes.run_step,
                        Kernel::Sum => &pipes.sum_step,
                    });
                    pass.set_bind_group(0, &bg, &[slot as u32 * PARAM_STRIDE as u32]);
                    let (x, y) = grid(p[1]);
                    pass.dispatch_workgroups(x, y, 1);
                }
            }
            let (src_off, words) = if self.debug_full_scratch.load(Ordering::SeqCst) {
                (0, scratch.size() / 4)
            } else {
                (u64::from(plan.out_slot) * point_bytes, 2 * u64::from(plan.windows) * 3 * fw)
            };
            copies.push((scratch.clone(), src_off, staging_words * 4, words * 4));
            pending.jobs.push(JobMeta {
                kind,
                c: plan.c,
                chunk: plan.chunk,
                windows: plan.windows,
                word_off: staging_words as usize,
            });
            staging_words += words;
        }
        let staging = self.buffer(
            "staging",
            staging_words * 4,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        )?;
        for (src, so, dst_off, size) in copies {
            enc.copy_buffer_to_buffer(&src, so, &staging, dst_off, size);
        }
        pending.staging = Some((staging, staging_words * 4));
        self.queue.submit([enc.finish()]);
        Ok(())
    }

    /// Tests: run one G1 job and return the whole scratch buffer (words), to compare every
    /// intermediate slot with the CPU simulator of the same plan.
    #[doc(hidden)]
    pub async fn debug_scratch_g1(&self, bases: &GpuBases<G1>, scalars: &[BigInteger256]) -> Result<Vec<u32>, GpuError> {
        self.debug_full_scratch.store(true, Ordering::SeqCst);
        let p = self.submit(&[MsmJob::G1(bases, scalars)]);
        self.debug_full_scratch.store(false, Ordering::SeqCst);
        let mut p = p?;
        let (st, size) = p.staging.take().ok_or_else(|| GpuError::Device("no staging".into()))?;
        let w = map_read(&self.device, &st, size).await;
        st.destroy();
        w
    }

    /// Single G1 MSM: `Σ scalars[i]·bases[i]`.
    pub async fn msm_g1(&self, bases: &GpuBases<G1>, scalars: &[Fr]) -> Result<G1Projective, GpuError> {
        let b = BigInts(scalars.iter().map(|s| s.into_bigint()).collect());
        let out = self.submit(&[MsmJob::G1(bases, &b.0)])?.wait().await?;
        out.into_iter().next().and_then(MsmOutput::g1).ok_or_else(|| GpuError::Device("no output".into()))
    }

    /// Single G2 MSM.
    pub async fn msm_g2(&self, bases: &GpuBases<G2>, scalars: &[Fr]) -> Result<G2Projective, GpuError> {
        let b = BigInts(scalars.iter().map(|s| s.into_bigint()).collect());
        let out = self.submit(&[MsmJob::G2(bases, &b.0)])?.wait().await?;
        out.into_iter().next().and_then(MsmOutput::g2).ok_or_else(|| GpuError::Device("no output".into()))
    }

    /// Run the point-formula self-test kernel on `(P, Q, R)` triples (P, Q homogeneous
    /// projective `(x, y, z)` with `z != 0`, R affine): returns
    /// `[P+Q, P+R (mixed), 2P (dbl), P+P (add)]` as arkworks points.
    #[doc(hidden)]
    pub async fn point_selftest<P: GpuCurve>(
        &self,
        triples: &[PointTriple<P>],
    ) -> Result<Vec<[Projective<P>; 4]>, GpuError>
    where
        P::BaseField: GpuField,
    {
        let fw = P::KIND.fw();
        let pipes = match P::KIND {
            CurveKind::G1 => &self.g1,
            CurveKind::G2 => &self.g2,
        };
        let mut w = Vec::new();
        for (p, q, r) in triples {
            for x in p.iter().chain(q.iter()) {
                x.push_words(&self.conv, &mut w);
            }
            r.x.push_words(&self.conv, &mut w);
            r.y.push_words(&self.conv, &mut w);
        }
        let n = triples.len() as u64;
        let inp = self.buffer("in", 4 * w.len() as u64, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST)?;
        self.queue.write_buffer(&inp, 0, as_bytes(&w));
        let dummy = self.buffer("dummy", 16, wgpu::BufferUsages::STORAGE)?;
        let osz = 4 * 4 * 3 * fw as u64 * n;
        let out = self.buffer("out", osz, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC)?;
        let st = self.buffer("st", osz, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST)?;
        let pbuf = self.buffer("p", 256, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST)?;
        self.queue.write_buffer(&pbuf, 0, as_bytes(&[0u32, n as u32, 0, 0, 0, 0, 0, 0]));
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipes.layout,
            entries: &[
                bind(0, &inp),
                bind(1, &dummy),
                bind(2, &dummy),
                bind(3, &out),
                wgpu::BindGroupEntry {
                    binding: 4,
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
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipes.selftest);
            pass.set_bind_group(0, &bg, &[0]);
            pass.dispatch_workgroups((n as u32).div_ceil(WG), 1, 1);
        }
        enc.copy_buffer_to_buffer(&out, 0, &st, 0, osz);
        self.queue.submit([enc.finish()]);
        let words = map_read(&self.device, &st, osz).await?;
        let bad = || GpuError::Device("bad limbs".into());
        let mut res = Vec::new();
        for t in words.chunks_exact(4 * 3 * fw) {
            let mut four = [Projective::<P>::zero(); 4];
            for (k, pt) in t.chunks_exact(3 * fw).enumerate() {
                let x = P::BaseField::from_words(&self.conv, &pt[..fw]).ok_or_else(bad)?;
                let y = P::BaseField::from_words(&self.conv, &pt[fw..2 * fw]).ok_or_else(bad)?;
                let z = P::BaseField::from_words(&self.conv, &pt[2 * fw..]).ok_or_else(bad)?;
                four[k] = homogeneous_to_jacobian::<P>(x, y, z);
            }
            res.push(four);
        }
        Ok(res)
    }

    /// Run the Fq field self-test kernel: for each `(a, b)` returns `(a·b, a+b, a−b)`.
    #[doc(hidden)]
    pub async fn field_selftest(
        &self,
        pairs: &[(ark_bn254::Fq, ark_bn254::Fq)],
    ) -> Result<Vec<[ark_bn254::Fq; 3]>, GpuError> {
        use ark_bn254::Fq;
        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("field-test"),
            source: wgpu::ShaderSource::Wgsl(shader::field_test_module().into()),
        });
        let pipe = self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("field-test"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let mut w = Vec::new();
        for (a, b) in pairs {
            a.push_words(&self.conv, &mut w);
            b.push_words(&self.conv, &mut w);
        }
        let inp = self.buffer("in", 4 * w.len() as u64, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST)?;
        self.queue.write_buffer(&inp, 0, as_bytes(&w));
        let osz = 4 * 60 * pairs.len() as u64;
        let out = self.buffer("out", osz, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC)?;
        let st = self.buffer("st", osz, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST)?;
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipe.get_bind_group_layout(0),
            entries: &[bind(0, &inp), bind(1, &out)],
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipe);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups((pairs.len() as u32).div_ceil(WG), 1, 1);
        }
        enc.copy_buffer_to_buffer(&out, 0, &st, 0, osz);
        self.queue.submit([enc.finish()]);
        let words = map_read(&self.device, &st, osz).await?;
        let conv = &self.conv;
        let f = |o: usize| Fq::from_words(conv, &words[o..o + 20]).ok_or_else(|| GpuError::Device("bad limbs".into()));
        (0..pairs.len())
            .map(|i| Ok([f(i * 60)?, f(i * 60 + 20)?, f(i * 60 + 40)?]))
            .collect()
    }
}

/// Self-test input: P and Q as homogeneous `(x, y, z)`, R affine.
pub type PointTriple<P> = (
    [<P as ark_ec::CurveConfig>::BaseField; 3],
    [<P as ark_ec::CurveConfig>::BaseField; 3],
    Affine<P>,
);

fn bind(binding: u32, buf: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buf.as_entire_binding(),
    }
}

/// 2-D dispatch grid for `count` threads (x ≤ 65535 workgroups).
fn grid(count: u32) -> (u32, u32) {
    let groups = count.div_ceil(WG).max(1);
    let y = groups.div_ceil(65535);
    (groups.div_ceil(y), y)
}

fn as_bytes<T: Copy>(v: &[T]) -> &[u8] {
    // SAFETY: only used with u32 / [u32; N] slices (no padding, any bit pattern valid).
    unsafe { core::slice::from_raw_parts(v.as_ptr().cast::<u8>(), core::mem::size_of_val(v)) }
}

async fn map_read(device: &wgpu::Device, buf: &wgpu::Buffer, size: u64) -> Result<Vec<u32>, GpuError> {
    let (tx, rx) = futures_channel::oneshot::channel();
    buf.map_async(wgpu::MapMode::Read, 0..size, move |r| {
        let _ = tx.send(r);
    });
    recv(device, rx)
        .await?
        .map_err(|e| GpuError::Device(format!("map: {e}")))?;
    let words = match buf.get_mapped_range(0..size) {
        Ok(view) => Ok(view
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b))
            .collect()),
        Err(e) => Err(GpuError::Device(format!("map range: {e}"))),
    };
    buf.unmap();
    words
}

/// Bases resident on the GPU for one curve.
pub struct GpuBases<P: GpuCurve>
where
    P::BaseField: GpuField,
{
    buf: wgpu::Buffer,
    n: usize,
    skip: Vec<bool>,
    c: u32,
    _p: core::marker::PhantomData<P>,
}

impl<P: GpuCurve> GpuBases<P>
where
    P::BaseField: GpuField,
{
    pub fn len(&self) -> usize {
        self.n
    }
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
    /// Override the Pippenger window size (bits, 2..=16).
    pub fn set_window_bits(&mut self, c: u32) {
        self.c = c.clamp(2, 16);
    }
    pub fn window_bits(&self) -> u32 {
        self.c
    }
    /// Bytes of GPU memory used.
    pub fn gpu_bytes(&self) -> u64 {
        self.buf.size()
    }
}

impl<P: GpuCurve> Drop for GpuBases<P>
where
    P::BaseField: GpuField,
{
    fn drop(&mut self) {
        self.buf.destroy();
    }
}

/// One MSM in a batch: resident bases and canonical (non-Montgomery) scalars `< r`.
/// Fewer scalars than bases is allowed (the rest count as zero).
pub enum MsmJob<'a> {
    G1(&'a GpuBases<G1>, &'a [BigInteger256]),
    G2(&'a GpuBases<G2>, &'a [BigInteger256]),
}

/// Result of one MSM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsmOutput {
    G1(G1Projective),
    G2(G2Projective),
}

impl MsmOutput {
    pub fn g1(self) -> Option<G1Projective> {
        match self {
            MsmOutput::G1(p) => Some(p),
            MsmOutput::G2(_) => None,
        }
    }
    pub fn g2(self) -> Option<G2Projective> {
        match self {
            MsmOutput::G2(p) => Some(p),
            MsmOutput::G1(_) => None,
        }
    }
}

struct JobMeta {
    kind: CurveKind,
    c: u32,
    chunk: u32,
    windows: u32,
    word_off: usize,
}

type ScopeFuture = core::pin::Pin<Box<dyn core::future::Future<Output = Option<wgpu::Error>>>>;

/// A submitted MSM batch. Always finish it with [`Pending::wait`]; if it is dropped instead,
/// the secret buffers are still zeroed and destroyed (without waiting).
pub struct Pending {
    device: wgpu::Device,
    queue: wgpu::Queue,
    staging: Option<(wgpu::Buffer, u64)>,
    /// Buffers holding scalar-derived data, with their sizes.
    secret: Vec<(wgpu::Buffer, u64)>,
    jobs: Vec<JobMeta>,
    scopes: Vec<ScopeFuture>,
    conv: Conv,
    cleaned: bool,
}

impl Pending {
    /// Wait for the GPU, read the window sums, finish each MSM on the CPU, then wipe and
    /// destroy every per-proof buffer.
    pub async fn wait(mut self) -> Result<Vec<MsmOutput>, GpuError> {
        let res = self.read().await;
        self.cleanup();
        // wait for the zeroing to complete before releasing memory
        let (tx, rx) = futures_channel::oneshot::channel();
        self.queue.on_submitted_work_done(move || {
            let _ = tx.send(());
        });
        let done = recv(&self.device, rx).await;
        self.destroy();
        let mut scope_err = None;
        for f in self.scopes.drain(..) {
            if let Some(e) = f.await {
                scope_err.get_or_insert(e.to_string());
            }
        }
        if let Some(e) = scope_err {
            return Err(GpuError::Device(e));
        }
        let out = res?;
        done?;
        Ok(out)
    }

    async fn read(&mut self) -> Result<Vec<MsmOutput>, GpuError> {
        let Some((staging, size)) = &self.staging else {
            return Err(GpuError::Device("nothing submitted".into()));
        };
        let mut words = Zeroizing::new(map_read(&self.device, staging, *size).await?);
        let mut out = Vec::with_capacity(self.jobs.len());
        for j in &self.jobs {
            let n = 2 * j.windows as usize * 3 * j.kind.fw();
            let w = &words[j.word_off..j.word_off + n];
            out.push(match j.kind {
                CurveKind::G1 => MsmOutput::G1(finish::<G1>(&self.conv, w, j.c, j.chunk)?),
                CurveKind::G2 => MsmOutput::G2(finish::<G2>(&self.conv, w, j.c, j.chunk)?),
            });
        }
        words.zeroize();
        Ok(out)
    }

    /// Overwrite every scalar-derived buffer with zeros (queued; runs after the MSMs).
    fn cleanup(&mut self) {
        if self.cleaned {
            return;
        }
        self.cleaned = true;
        let mut enc = self.device.create_command_encoder(&Default::default());
        for (buf, size) in &self.secret {
            if buf.usage().contains(wgpu::BufferUsages::STORAGE)
                && buf.usage().contains(wgpu::BufferUsages::COPY_SRC)
            {
                // scratch (bucket sums): GPU-side clear
                enc.clear_buffer(buf, 0, None);
            } else {
                let zeros = vec![0u8; *size as usize];
                self.queue.write_buffer(buf, 0, &zeros);
            }
        }
        // the staging buffer is never left mapped (`map_read` unmaps on every path)
        if let Some((st, _)) = &self.staging {
            enc.clear_buffer(st, 0, None);
        }
        self.queue.submit([enc.finish()]);
    }

    fn destroy(&mut self) {
        for (b, _) in self.secret.drain(..) {
            b.destroy();
        }
        if let Some((st, _)) = self.staging.take() {
            st.destroy();
        }
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.cleanup();
        // destroy() is deferred by the implementation until queued work using it completes
        self.destroy();
    }
}

/// Convert the GPU window sums (homogeneous projective, GPU Montgomery form) and combine them.
fn finish<P: GpuCurve>(conv: &Conv, w: &[u32], c: u32, m: u32) -> Result<Projective<P>, GpuError>
where
    P::BaseField: GpuField,
{
    let fw = P::KIND.fw();
    let bad = || GpuError::Device("GPU returned a malformed field element".into());
    let mut pts = Vec::with_capacity(w.len() / (3 * fw));
    for p in w.chunks_exact(3 * fw) {
        let x = P::BaseField::from_words(conv, &p[..fw]).ok_or_else(bad)?;
        let y = P::BaseField::from_words(conv, &p[fw..2 * fw]).ok_or_else(bad)?;
        let z = P::BaseField::from_words(conv, &p[2 * fw..]).ok_or_else(bad)?;
        pts.push(homogeneous_to_jacobian::<P>(x, y, z));
    }
    let r = combine_windows(&pts, c, m);
    // a GPU fault must not slip through as a plausible-looking value
    let a = r.into_affine();
    if !a.is_on_curve() {
        return Err(GpuError::Device("GPU result is not on the curve".into()));
    }
    Ok(r)
}

/// (X : Y : Z) with x = X/Z, y = Y/Z  →  arkworks Jacobian (X·Z, Y·Z², Z).
fn homogeneous_to_jacobian<P: SWCurveConfig>(x: P::BaseField, y: P::BaseField, z: P::BaseField) -> Projective<P> {
    if z.is_zero() {
        return Projective::zero();
    }
    Projective::new_unchecked(x * z, y * z.square(), z)
}

/// `Σ_w 2^{c·w} · (u_w + m·s_w)` (Horner) for `pairs = [u_0, s_0, u_1, s_1, ...]`.
pub(crate) fn combine_windows<P: SWCurveConfig>(pairs: &[Projective<P>], c: u32, m: u32) -> Projective<P> {
    let mut acc = Projective::<P>::zero();
    for p in pairs.as_chunks::<2>().0.iter().rev() {
        for _ in 0..c {
            acc.double_in_place();
        }
        let mut s = p[1];
        for _ in 0..m.trailing_zeros() {
            s.double_in_place();
        }
        acc += p[0] + s;
    }
    acc
}

/// Canonical scalars, zeroized on drop.
pub struct BigInts(pub Vec<BigInteger256>);

impl BigInts {
    pub fn from_fr(s: &[Fr]) -> Self {
        BigInts(s.iter().map(|x| x.into_bigint()).collect())
    }
}

impl Drop for BigInts {
    fn drop(&mut self) {
        for b in self.0.iter_mut() {
            b.0.zeroize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_bn254::{Fq, G1Affine};
    use ark_ec::PrimeGroup;
    use ark_ff::UniformRand;

    #[test]
    fn homogeneous_conversion() {
        let mut rng = ark_std::test_rng();
        let p = (G1Projective::generator() * Fr::rand(&mut rng)).into_affine();
        let z = Fq::rand(&mut rng);
        let j = homogeneous_to_jacobian::<G1>(p.x * z, p.y * z, z);
        assert_eq!(j.into_affine(), p);
        assert!(homogeneous_to_jacobian::<G1>(Fq::zero(), Fq::from(5u64), Fq::zero()).is_zero());
        let _ = G1Affine::identity();
    }

    /// Every intermediate GPU scratch slot (bucket sums, partials, chunk sums, outputs)
    /// equals the CPU simulator of the same plan. Skips without a GPU.
    #[test]
    fn gpu_scratch_matches_simulator() {
        use ark_ec::short_weierstrass::Projective;
        let Ok(g) = pollster::block_on(Gpu::new()) else {
            eprintln!("SKIP (no GPU)");
            return;
        };
        let mut rng = ark_std::test_rng();
        for (n, c, small) in [(255usize, 6u32, false), (255, 4, false), (2000, 5, true), (3000, 9, false), (40, 2, false)] {
            let p = G1Projective::rand(&mut rng);
            let bases = G1Projective::normalize_batch(&(0..n).map(|i| p * Fr::from(i as u64 + 1)).collect::<Vec<_>>());
            let sc: Vec<Fr> =
                (0..n).map(|i| if small { Fr::from((i % 3) as u64) } else { Fr::rand(&mut rng) }).collect();
            let big = BigInts::from_fr(&sc);
            let mut gb = g.upload_bases(&bases).unwrap();
            gb.set_window_bits(c);
            let words = pollster::block_on(g.debug_scratch_g1(&gb, &big.0)).unwrap();
            let plan = Plan::new(&big.0, &gb.skip, c);
            let sim: Vec<Projective<G1>> = plan::sim::scratch(&plan, &bases);
            for (sl, want) in sim.iter().enumerate() {
                let o = sl * 60;
                let f = |a: usize| Fq::from_words(&g.conv, &words[o + a..o + a + 20]).unwrap();
                let gp = homogeneous_to_jacobian::<G1>(f(0), f(20), f(40));
                assert_eq!(gp.into_affine(), want.into_affine(), "n={n} c={c}: slot {sl} of {}", plan.slots);
            }
        }
    }

    #[test]
    fn grid_covers() {
        for n in [0u32, 1, 64, 65, 65535 * 64, 65535 * 64 + 1, 5_000_000] {
            let (x, y) = grid(n);
            assert!(x <= 65535 && u64::from(x) * u64::from(y) * 64 >= u64::from(n));
        }
    }
}
