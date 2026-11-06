//! CPU planning of one Pippenger MSM: signed-digit windows, a counting sort of the
//! (window, bucket) references, and a list of bounded-size summation tasks in levels.
//!
//! * Window `c` bits, `W = ceil(255 / c)` windows, `B = 2^(c-1)` buckets per window. A digit
//!   `d > 2^(c-1)` becomes `d - 2^c` with a carry into the next window, so `|d| <= 2^(c-1)` and
//!   the top window never overflows for scalars `< r < 2^254`.
//! * Entry `= base index | sign << 31`, sorted by bucket key `w·B + |d| - 1`.
//! * Scratch layout (projective points): `[0, W·B)` bucket sums, then partial sums, then the
//!   bucket-reduction outputs `U`, `T` (`W·chunks` each), then the `W` window sums.
//! * A bucket with more than `K` entries is split into `ceil(len/K)` tasks whose partial sums
//!   are contiguous; those partials are summed at the next level, and so on. Every task
//!   therefore adds at most `K` points no matter how skewed the scalars are (witness vectors
//!   are full of 0/1 and other small values).
//!
//! Everything here is derived from secret scalars and is zeroized on drop.

use ark_ff::BigInteger256;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Max points summed by one GPU thread per task.
pub const TASK_MAX: usize = 32;

/// Pick the window size for `n` bases. Tuned on Apple M5 Max / Metal with the real withdraw
/// proof (whole proof best at c = 8; a random 2^16 G1 MSM is flat between c = 8 and 13).
/// Larger windows make the (fixed-cost) bucket reduction dominate, and witness scalars are
/// mostly small, so most of their windows are empty anyway.
pub fn window_bits(n: usize) -> u32 {
    let lg = usize::BITS - n.max(1).leading_zeros(); // floor(log2 n) + 1
    lg.saturating_sub(4).clamp(4, 8)
}

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct Plan {
    pub c: u32,
    pub windows: u32,
    pub buckets: u32,
    pub chunk: u32,
    pub chunks: u32,
    pub entries: Vec<u32>,
    /// `[start, len, out, kind]`; kind 0 = `start..start+len` index `entries`, 1 = scratch.
    pub tasks: Vec<[u32; 4]>,
    /// `(first task, task count)` per level, in dispatch order. Within a level tasks are
    /// sorted by `len`, descending.
    pub levels: Vec<(u32, u32)>,
    /// Per level: number of tasks with `len > k`, for each accumulation step `k`.
    pub steps: Vec<Vec<u32>>,
    /// First scratch slot of `U` (`windows·chunks` slots).
    pub u_slot: u32,
    /// First scratch slot of `T` (`windows·chunks` slots).
    pub t_slot: u32,
    /// First scratch slot of the `2·windows` outputs (`ΣU`, `Σ suffix` per window).
    pub out_slot: u32,
    /// Total scratch slots.
    pub slots: u32,
}

fn window(s: &BigInteger256, bit: u32, c: u32) -> u64 {
    let (w, sh) = ((bit / 64) as usize, bit % 64);
    if w >= 4 {
        return 0;
    }
    let mut v = s.0[w] >> sh;
    if sh + c > 64 && w + 1 < 4 {
        v |= s.0[w + 1] << (64 - sh);
    }
    v & ((1u64 << c) - 1)
}

/// Signed digits of `s` into `out` (length `windows`).
fn digits(s: &BigInteger256, c: u32, out: &mut [i32]) {
    let half = 1i64 << (c - 1);
    let mut carry = 0i64;
    for (w, d) in out.iter_mut().enumerate() {
        let mut v = window(s, w as u32 * c, c) as i64 + carry;
        if v > half {
            v -= 1i64 << c;
            carry = 1;
        } else {
            carry = 0;
        }
        *d = v as i32;
    }
    debug_assert_eq!(carry, 0, "scalar must be < 2^254");
}

impl Plan {
    /// `skip[i]` marks bases that are the point at infinity (their scalars are ignored).
    pub fn new(scalars: &[BigInteger256], skip: &[bool], c: u32) -> Plan {
        assert!((2..=16).contains(&c));
        let windows = 255u32.div_ceil(c);
        let buckets = 1u32 << (c - 1);
        let nb = (windows * buckets) as usize;
        let w = windows as usize;

        let mut dig = zeroize::Zeroizing::new(vec![0i32; scalars.len() * w]);
        let mut counts = zeroize::Zeroizing::new(vec![0u32; nb + 1]);
        for (i, s) in scalars.iter().enumerate() {
            if skip.get(i).copied().unwrap_or(false) || s.0 == [0; 4] {
                continue;
            }
            let d = &mut dig[i * w..(i + 1) * w];
            digits(s, c, d);
            for (wi, &x) in d.iter().enumerate() {
                if x != 0 {
                    counts[wi * buckets as usize + x.unsigned_abs() as usize - 1] += 1;
                }
            }
        }
        // exclusive prefix sum -> start offsets
        let mut offs = zeroize::Zeroizing::new(vec![0u32; nb + 1]);
        for k in 0..nb {
            offs[k + 1] = offs[k] + counts[k];
        }
        let total = offs[nb] as usize;
        let mut entries = vec![0u32; total];
        let mut cur = zeroize::Zeroizing::new(offs[..nb].to_vec());
        for (i, s) in scalars.iter().enumerate() {
            if skip.get(i).copied().unwrap_or(false) || s.0 == [0; 4] {
                continue;
            }
            for (wi, &x) in dig[i * w..(i + 1) * w].iter().enumerate() {
                if x != 0 {
                    let k = wi * buckets as usize + x.unsigned_abs() as usize - 1;
                    entries[cur[k] as usize] = i as u32 | (u32::from(x < 0) << 31);
                    cur[k] += 1;
                }
            }
        }

        // tasks, by level
        let mut levels: Vec<Vec<[u32; 4]>> = Vec::new();
        let mut next = nb as u32;
        for k in 0..nb {
            let len = counts[k] as usize;
            if len == 0 {
                continue;
            }
            let (mut start, mut len, mut kind, mut lvl) = (offs[k], len, 0u32, 0usize);
            loop {
                if levels.len() <= lvl {
                    levels.push(Vec::new());
                }
                if len <= TASK_MAX {
                    levels[lvl].push([start, len as u32, k as u32, kind]);
                    break;
                }
                let q = len.div_ceil(TASK_MAX);
                let first = next;
                next += q as u32;
                let mut s = start;
                for t in 0..q {
                    let sz = (len / q + usize::from(t < len % q)) as u32;
                    levels[lvl].push([s, sz, first + t as u32, kind]);
                    s += sz;
                }
                (start, len, kind, lvl) = (first, q, 1, lvl + 1);
            }
        }
        let mut tasks = Vec::with_capacity(levels.iter().map(Vec::len).sum());
        let mut lv = Vec::with_capacity(levels.len());
        let mut steps = Vec::with_capacity(levels.len());
        for l in levels.iter_mut() {
            l.sort_by(|x, y| y[1].cmp(&x[1]));
            let maxlen = l.first().map_or(0, |t| t[1]) as usize;
            let mut st = vec![0u32; maxlen];
            for t in l.iter() {
                for c in st.iter_mut().take(t[1] as usize) {
                    *c += 1;
                }
            }
            steps.push(st);
            lv.push((tasks.len() as u32, l.len() as u32));
            tasks.append(l);
        }

        // bucket reduction chunking: m ≈ sqrt(B)
        let chunk = 1u32 << ((c - 1) / 2);
        let chunks = buckets / chunk;
        let u_slot = next;
        let t_slot = u_slot + windows * chunks;
        let out_slot = t_slot + windows * chunks;
        Plan {
            c,
            windows,
            buckets,
            chunk,
            chunks,
            entries,
            tasks,
            levels: lv,
            steps,
            u_slot,
            t_slot,
            out_slot,
            slots: out_slot + 2 * windows,
        }
    }
}

#[cfg(test)]
pub(crate) mod sim {
    //! Runs a [`Plan`] on the CPU with arkworks arithmetic, step for step like the kernels.
    use super::*;
    use ark_ec::short_weierstrass::{Affine, Projective, SWCurveConfig};
    use ark_ec::PrimeGroup;
    use ark_ff::Zero;

    pub fn run<P: SWCurveConfig>(plan: &Plan, bases: &[Affine<P>]) -> Projective<P> {
        let scr = scratch(plan, bases);
        crate::combine_windows(&scr[plan.out_slot as usize..], plan.c, plan.chunk)
    }

    /// The whole scratch buffer after all kernels.
    pub fn scratch<P: SWCurveConfig>(plan: &Plan, bases: &[Affine<P>]) -> Vec<Projective<P>> {
        let mut scr = vec![Projective::<P>::zero(); plan.slots as usize];
        for &(off, cnt) in &plan.levels {
            for t in &plan.tasks[off as usize..(off + cnt) as usize] {
                let [start, len, out, kind] = *t;
                let mut acc = Projective::<P>::zero();
                for k in start..start + len {
                    if kind == 0 {
                        let e = plan.entries[k as usize];
                        let b = bases[(e & 0x7fff_ffff) as usize];
                        acc += if e >> 31 == 1 { -b } else { b };
                    } else {
                        acc += scr[k as usize];
                    }
                }
                scr[out as usize] = acc;
            }
        }
        let (m, nc, w) = (plan.chunk, plan.chunks, plan.windows);
        let cnt = w * nc;
        // 1. in-place running sums inside each chunk of m buckets
        for i in 0..cnt {
            for f in 1..m {
                let sl = (i * m + m - 1 - f) as usize;
                scr[sl] = scr[sl] + scr[sl + 1];
            }
        }
        // 2. U_i = Σ running sums, T_i = first running sum
        for i in 0..cnt {
            let mut u = Projective::<P>::zero();
            for f in 0..m {
                u += scr[(i * m + f) as usize];
            }
            scr[(plan.u_slot + i) as usize] = u;
            scr[(plan.t_slot + i) as usize] = scr[(i * m) as usize];
        }
        // 3. in-place suffix sums of T inside each window, then the two window outputs
        for wi in 0..w {
            for f in 1..nc {
                let sl = (plan.t_slot + wi * nc + nc - 1 - f) as usize;
                scr[sl] = scr[sl] + scr[sl + 1];
            }
            let mut su = Projective::<P>::zero();
            for f in 0..nc {
                su += scr[(plan.u_slot + wi * nc + f) as usize];
            }
            let mut st = Projective::<P>::zero();
            for f in 1..nc {
                st += scr[(plan.t_slot + wi * nc + f) as usize];
            }
            scr[(plan.out_slot + 2 * wi) as usize] = su;
            scr[(plan.out_slot + 2 * wi + 1) as usize] = st;
        }
        let _ = Projective::<P>::generator();
        scr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_bn254::{Fr, G1Affine, G1Projective, G2Affine, G2Projective};
    use ark_ec::{CurveGroup, VariableBaseMSM};
    use ark_ff::{PrimeField, UniformRand};
    use rand_chacha::ChaCha20Rng;
    use rand_chacha::rand_core::SeedableRng;

    #[test]
    fn digits_recompose() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        for c in 2..=16u32 {
            let w = 255u32.div_ceil(c) as usize;
            let mut d = vec![0i32; w];
            for s in (0..50).map(|_| Fr::rand(&mut rng)).chain([-Fr::from(1u64), Fr::from(0u64), Fr::from(1u64)]) {
                digits(&s.into_bigint(), c, &mut d);
                let mut acc = Fr::from(0u64);
                for &x in d.iter().rev() {
                    acc = acc * Fr::from(1u64 << c) + if x < 0 { -Fr::from((-x) as u64) } else { Fr::from(x as u64) };
                    assert!(x.unsigned_abs() <= 1 << (c - 1));
                }
                assert_eq!(acc, s, "c={c}");
            }
        }
    }

    fn check<P: ark_ec::short_weierstrass::SWCurveConfig<ScalarField = Fr>>(n: usize, c: u32, small: bool) {
        let mut rng = ChaCha20Rng::seed_from_u64(n as u64);
        let g = ark_ec::short_weierstrass::Projective::<P>::rand(&mut rng);
        let mut bases: Vec<_> = (0..n).map(|i| (g * Fr::from(i as u64 + 1)).into_affine()).collect();
        if n > 3 {
            bases[1] = Default::default(); // infinity (ark's Default affine is the identity)
            bases[2] = bases[0];
        }
        let skip: Vec<bool> = bases.iter().map(|b| b.infinity).collect();
        let mut scalars: Vec<Fr> = (0..n)
            .map(|i| if small { Fr::from((i % 5) as u64) } else { Fr::rand(&mut rng) })
            .collect();
        if n > 4 {
            scalars[3] = -Fr::from(1u64);
            scalars[4] = Fr::from(0u64);
        }
        let big: Vec<_> = scalars.iter().map(|s| s.into_bigint()).collect();
        let plan = Plan::new(&big, &skip, c);
        let got = sim::run(&plan, &bases).into_affine();
        let want = ark_ec::short_weierstrass::Projective::<P>::msm(&bases, &scalars).unwrap().into_affine();
        assert_eq!(got, want, "n={n} c={c} small={small}");
    }

    #[test]
    fn plan_matches_ark() {
        for &n in &[1usize, 2, 3, 7, 255, 256, 1000] {
            for c in [2, 3, 4, 5, 8, 11] {
                check::<ark_bn254::g1::Config>(n, c, false);
                check::<ark_bn254::g1::Config>(n, c, true);
            }
        }
        check::<ark_bn254::g2::Config>(300, 6, false);
        check::<ark_bn254::g2::Config>(300, 6, true);
        let _ = (G1Affine::default(), G1Projective::default(), G2Affine::default(), G2Projective::default());
    }
}
