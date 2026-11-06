//! WGSL source generation. The field code is emitted with unrolled limb operations and the
//! modulus limbs as literals (only the outer Montgomery loop is a loop), and the curve/MSM code
//! is shared between G1 (`F = Fq`) and G2 (`F = Fq2`) through a small set of `f*` wrappers.

use core::fmt::Write;

use ark_bn254::Fq2;
use ark_ec::short_weierstrass::SWCurveConfig;
use ark_ff::Field;

use crate::field::{FieldConsts, LIMBS, LIMB_BITS, fq2_const_words, fq_consts, fr_consts};

/// Which curve a shader module is generated for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveKind {
    G1,
    G2,
}

impl CurveKind {
    /// u32 words per base-field element.
    pub const fn fw(self) -> usize {
        match self {
            CurveKind::G1 => LIMBS,
            CurveKind::G2 => 2 * LIMBS,
        }
    }
}

fn arr(l: &[u32]) -> String {
    let v: Vec<String> = l.iter().map(|x| format!("{x}u")).collect();
    format!("BigInt(array<u32, {LIMBS}>({}))", v.join(", "))
}

/// Fq arithmetic on `BigInt` (20 × 13-bit limbs, Montgomery, R = 2^260, values < p).
fn fq_wgsl(c: &FieldConsts) -> String {
    let n = LIMBS;
    let mask = (1u32 << LIMB_BITS) - 1;
    let mut s = String::new();
    let w = &mut s;
    writeln!(w, "struct BigInt {{ l: array<u32, {n}> }}").unwrap();
    writeln!(w, "const MASK: u32 = {mask}u;").unwrap();
    writeln!(w, "fn fq_zero() -> BigInt {{ return BigInt(array<u32, {n}>()); }}").unwrap();
    writeln!(w, "fn fq_one() -> BigInt {{ return {}; }}", arr(&c.one)).unwrap();
    // is_zero
    write!(w, "fn fq_is_zero(a: BigInt) -> bool {{ return (").unwrap();
    for j in 0..n {
        write!(w, "{}a.l[{j}]", if j > 0 { " | " } else { "" }).unwrap();
    }
    writeln!(w, ") == 0u; }}").unwrap();
    // conditional subtract p (input < 2p)
    writeln!(w, "fn fq_csub(a: BigInt) -> BigInt {{\n  var d: BigInt; var br = 0u; var t: u32;").unwrap();
    for j in 0..n {
        writeln!(w, "  t = a.l[{j}] - {}u - br; d.l[{j}] = t & MASK; br = t >> 31u;", c.p[j]).unwrap();
    }
    writeln!(w, "  if (br == 0u) {{ return d; }}\n  return a;\n}}").unwrap();
    // add
    writeln!(w, "fn fq_add(a: BigInt, b: BigInt) -> BigInt {{\n  var r: BigInt; var c = 0u; var t: u32;").unwrap();
    for j in 0..n {
        writeln!(w, "  t = a.l[{j}] + b.l[{j}] + c; r.l[{j}] = t & MASK; c = t >> {LIMB_BITS}u;").unwrap();
    }
    writeln!(w, "  return fq_csub(r);\n}}").unwrap();
    // sub
    writeln!(w, "fn fq_sub(a: BigInt, b: BigInt) -> BigInt {{\n  var d: BigInt; var br = 0u; var t: u32;").unwrap();
    for j in 0..n {
        writeln!(w, "  t = a.l[{j}] - b.l[{j}] - br; d.l[{j}] = t & MASK; br = t >> 31u;").unwrap();
    }
    writeln!(w, "  if (br == 0u) {{ return d; }}\n  var c = 0u;").unwrap();
    for j in 0..n {
        writeln!(w, "  t = d.l[{j}] + {}u + c; d.l[{j}] = t & MASK; c = t >> {LIMB_BITS}u;", c.p[j]).unwrap();
    }
    writeln!(w, "  return d;\n}}").unwrap();
    writeln!(w, "fn fq_neg(a: BigInt) -> BigInt {{ return fq_sub(fq_zero(), a); }}").unwrap();
    // Montgomery product, CIOS: inner limb loop unrolled with the modulus limbs as literals.
    writeln!(w, "fn fq_mul(a: BigInt, b: BigInt) -> BigInt {{").unwrap();
    for j in 0..n {
        writeln!(w, "  let b{j} = b.l[{j}]; var s{j} = 0u;").unwrap();
    }
    writeln!(w, "  var t: u32; var c: u32; var q: u32; var ai: u32;").unwrap();
    let body = |w: &mut String, ai: &str| {
        writeln!(w, "  ai = {ai};").unwrap();
        writeln!(
            w,
            "  t = s0 + ai * b0; q = ((t & MASK) * {}u) & MASK; t = t + q * {}u; c = t >> {LIMB_BITS}u;",
            c.n0, c.p[0]
        )
        .unwrap();
        for j in 1..n {
            writeln!(
                w,
                "  t = s{j} + ai * b{j} + q * {}u + c; s{} = t & MASK; c = t >> {LIMB_BITS}u;",
                c.p[j],
                j - 1
            )
            .unwrap();
        }
        writeln!(w, "  s{} = c;", n - 1).unwrap();
    };
    // The outer loop stays a loop: fully unrolling it made the kernels ~20x larger and the
    // Metal compiler then hung or miscompiled them.
    writeln!(w, "  for (var i = 0u; i < {n}u; i++) {{").unwrap();
    body(w, "a.l[i]");
    writeln!(w, "  }}").unwrap();
    write!(w, "  return fq_csub(BigInt(array<u32, {n}>(").unwrap();
    for j in 0..n {
        write!(w, "{}s{j}", if j > 0 { ", " } else { "" }).unwrap();
    }
    writeln!(w, ")));\n}}").unwrap();
    s
}

/// `F` wrappers for G1: F = Fq, b3 = 3·b = 9 (as additions).
const G1_F: &str = r#"
alias F = BigInt;
fn fzero() -> F { return fq_zero(); }
fn fone() -> F { return fq_one(); }
fn fis_zero(a: F) -> bool { return fq_is_zero(a); }
fn fadd(a: F, b: F) -> F { return fq_add(a, b); }
fn fsub(a: F, b: F) -> F { return fq_sub(a, b); }
fn fneg(a: F) -> F { return fq_neg(a); }
fn fmul(a: F, b: F) -> F { return fq_mul(a, b); }
fn fmul_b3(a: F) -> F { let a2 = fq_add(a, a); let a4 = fq_add(a2, a2); let a8 = fq_add(a4, a4); return fq_add(a8, a); }
"#;

/// `F` wrappers for G2: F = Fq2 = Fq[u]/(u^2 + 1), b3 = 3·b' with b' = 3/(9+u).
fn g2_f(b3: &[u32]) -> String {
    format!(
        r#"
struct Fq2 {{ c0: BigInt, c1: BigInt }}
alias F = Fq2;
fn fzero() -> F {{ return Fq2(fq_zero(), fq_zero()); }}
fn fone() -> F {{ return Fq2(fq_one(), fq_zero()); }}
fn fis_zero(a: F) -> bool {{ return fq_is_zero(a.c0) && fq_is_zero(a.c1); }}
fn fadd(a: F, b: F) -> F {{ return Fq2(fq_add(a.c0, b.c0), fq_add(a.c1, b.c1)); }}
fn fsub(a: F, b: F) -> F {{ return Fq2(fq_sub(a.c0, b.c0), fq_sub(a.c1, b.c1)); }}
fn fneg(a: F) -> F {{ return Fq2(fq_neg(a.c0), fq_neg(a.c1)); }}
fn fmul(a: F, b: F) -> F {{
  let v0 = fq_mul(a.c0, b.c0);
  let v1 = fq_mul(a.c1, b.c1);
  let m = fq_mul(fq_add(a.c0, a.c1), fq_add(b.c0, b.c1));
  return Fq2(fq_sub(v0, v1), fq_sub(fq_sub(m, v0), v1));
}}
fn fmul_b3(a: F) -> F {{ return fmul(a, Fq2({}, {})); }}
"#,
        arr(&b3[..LIMBS]),
        arr(&b3[LIMBS..])
    )
}

/// Points, complete addition formulas (Renes–Costello–Batina 2016, a = 0, homogeneous
/// projective), storage access and the three MSM kernels.
const CURVE_AND_KERNELS: &str = r#"
struct Aff { x: F, y: F }
struct Proj { x: F, y: F, z: F }

fn proj_inf() -> Proj { return Proj(fzero(), fone(), fzero()); }

// RCB Algorithm 7: complete addition (handles P == Q and the point at infinity).
fn padd(p: Proj, q: Proj) -> Proj {
  var t0 = fmul(p.x, q.x); var t1 = fmul(p.y, q.y); var t2 = fmul(p.z, q.z);
  var t3 = fadd(p.x, p.y); var t4 = fadd(q.x, q.y); t3 = fmul(t3, t4);
  t4 = fadd(t0, t1); t3 = fsub(t3, t4); t4 = fadd(p.y, p.z);
  var x3 = fadd(q.y, q.z); t4 = fmul(t4, x3); x3 = fadd(t1, t2);
  t4 = fsub(t4, x3); x3 = fadd(p.x, p.z); var y3 = fadd(q.x, q.z);
  x3 = fmul(x3, y3); y3 = fadd(t0, t2); y3 = fsub(x3, y3);
  x3 = fadd(t0, t0); t0 = fadd(x3, t0); t2 = fmul_b3(t2);
  var z3 = fadd(t1, t2); t1 = fsub(t1, t2); y3 = fmul_b3(y3);
  x3 = fmul(t4, y3); t2 = fmul(t3, t1); x3 = fsub(t2, x3);
  y3 = fmul(y3, t0); t1 = fmul(t1, z3); y3 = fadd(t1, y3);
  t0 = fmul(t0, t3); z3 = fmul(z3, t4); z3 = fadd(z3, t0);
  return Proj(x3, y3, z3);
}

// RCB Algorithm 8: complete mixed addition, q affine (never the point at infinity).
fn padd_mixed(p: Proj, q: Aff) -> Proj {
  var t0 = fmul(p.x, q.x); var t1 = fmul(p.y, q.y); var t3 = fadd(q.x, q.y);
  var t4 = fadd(p.x, p.y); t3 = fmul(t3, t4); t4 = fadd(t0, t1);
  t3 = fsub(t3, t4); t4 = fmul(q.y, p.z); t4 = fadd(t4, p.y);
  var y3 = fmul(q.x, p.z); y3 = fadd(y3, p.x); var x3 = fadd(t0, t0);
  t0 = fadd(x3, t0); var t2 = fmul_b3(p.z); var z3 = fadd(t1, t2);
  t1 = fsub(t1, t2); y3 = fmul_b3(y3); x3 = fmul(t4, y3);
  t2 = fmul(t3, t1); x3 = fsub(t2, x3); y3 = fmul(y3, t0);
  t1 = fmul(t1, z3); y3 = fadd(t1, y3); t0 = fmul(t0, t3);
  z3 = fmul(z3, t4); z3 = fadd(z3, t0);
  return Proj(x3, y3, z3);
}

// RCB Algorithm 9: doubling.
fn pdbl(p: Proj) -> Proj {
  var t0 = fmul(p.y, p.y); var z3 = fadd(t0, t0); z3 = fadd(z3, z3);
  z3 = fadd(z3, z3); var t1 = fmul(p.y, p.z); var t2 = fmul(p.z, p.z);
  t2 = fmul_b3(t2); var x3 = fmul(t2, z3); var y3 = fadd(t0, t2);
  z3 = fmul(t1, z3); t1 = fadd(t2, t2); t2 = fadd(t1, t2);
  t0 = fsub(t0, t2); y3 = fmul(t0, y3); y3 = fadd(x3, y3);
  t1 = fmul(p.x, p.y); x3 = fmul(t0, t1); x3 = fadd(x3, x3);
  return Proj(x3, y3, z3);
}

struct Params { off: u32, count: u32, a: u32, b: u32, c: u32, d: u32, e: u32, f: u32 }

@group(0) @binding(0) var<storage, read> bases: array<u32>;
@group(0) @binding(1) var<storage, read> entries: array<u32>;
@group(0) @binding(2) var<storage, read> tasks: array<vec4<u32>>;
@group(0) @binding(3) var<storage, read_write> scratch: array<u32>;
@group(0) @binding(4) var<uniform> params: Params;

const FW: u32 = {{FW}}u;

fn load_base(i: u32) -> Aff {
  let o = i * 2u * FW;
  return Aff(ld_base(o), ld_base(o + FW));
}

// Scratch slots hold projective points; an all-zero slot (cleared bucket) and any z == 0
// point are read as the point at infinity.
fn load_proj(i: u32) -> Proj {
  let o = i * 3u * FW;
  let z = ld_scr(o + 2u * FW);
  if (fis_zero(z)) { return proj_inf(); }
  return Proj(ld_scr(o), ld_scr(o + FW), z);
}

fn store_proj(i: u32, p: Proj) {
  let o = i * 3u * FW;
  st_scr(o, p.x); st_scr(o + FW, p.y); st_scr(o + 2u * FW, p.z);
}

fn flat_index(gid: vec3<u32>, nwg: vec3<u32>) -> u32 { return gid.x + gid.y * nwg.x * {{WG}}u; }

// Every kernel below is straight-line: at most one point addition per thread per dispatch.
// Sequential sums are done by the host issuing one dispatch per step (`params.f` = step).
// Loops whose body contains a whole point addition were miscompiled by the Apple GPU compiler
// (hangs / wrong points), so there are none; see README, "Metal compiler".

// Accumulation step k = f: thread i owns task tasks[off + i] = (start, len, out, kind) with
// len > k (tasks are sorted by len, descending) and adds its k-th element into slot `out`.
// Level 0 (acc_base_step): elements are signed base references in `entries`.
@compute @workgroup_size({{WG}})
fn acc_base_step(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
  let idx = flat_index(gid, nwg);
  if (idx >= params.count) { return; }
  let t = tasks[params.off + idx];
  let k = params.f;
  let e = entries[t.x + k];
  var b = load_base(e & 0x7fffffffu);
  if ((e >> 31u) != 0u) { b.y = fneg(b.y); }
  var acc = Proj(b.x, b.y, fone());
  if (k != 0u) { acc = padd_mixed(load_proj(t.z), b); }
  store_proj(t.z, acc);
}

// Levels >= 1 (acc_scr_step): elements are consecutive scratch points (partial sums).
@compute @workgroup_size({{WG}})
fn acc_scr_step(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
  let idx = flat_index(gid, nwg);
  if (idx >= params.count) { return; }
  let t = tasks[params.off + idx];
  let k = params.f;
  var acc = load_proj(t.x + k);
  if (k != 0u) { acc = padd(load_proj(t.z), acc); }
  store_proj(t.z, acc);
}

// In-place running-sum step: for segment i (slots c + i·a .. c + i·a + b), step f ≥ 1 sets
// slot[j] += slot[j + 1] for j = b - 1 - f, so after steps 1..b-1 slot[j] = Σ_{j' ≥ j} slot[j'].
@compute @workgroup_size({{WG}})
fn run_step(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
  let idx = flat_index(gid, nwg);
  if (idx >= params.count) { return; }
  let s = params.c + idx * params.a + params.b - 1u - params.f;
  store_proj(s, padd(load_proj(s), load_proj(s + 1u)));
}

// Segment-sum step: out[d + i·e] (+)= slot[c + i·a + f]; step 0 copies.
@compute @workgroup_size({{WG}})
fn sum_step(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
  let idx = flat_index(gid, nwg);
  if (idx >= params.count) { return; }
  let o = params.d + idx * params.e;
  let x = load_proj(params.c + idx * params.a + params.f);
  if (params.f == 0u) {
    store_proj(o, x);
  } else {
    store_proj(o, padd(load_proj(o), x));
  }
}

// Self-test (tests only): for i, read P (projective), Q (projective), R (affine) from `bases`
// and write padd(P,Q), padd_mixed(P,R), pdbl(P), padd(P,P) to scratch slots 4i..4i+3.
@compute @workgroup_size({{WG}})
fn point_selftest(@builtin(global_invocation_id) gid: vec3<u32>) {
  let i = gid.x;
  if (i >= params.count) { return; }
  let o = i * 8u * FW;
  let p = Proj(ld_base(o), ld_base(o + FW), ld_base(o + 2u * FW));
  let q = Proj(ld_base(o + 3u * FW), ld_base(o + 4u * FW), ld_base(o + 5u * FW));
  let r = Aff(ld_base(o + 6u * FW), ld_base(o + 7u * FW));
  store_proj(4u * i, padd(p, q));
  store_proj(4u * i + 1u, padd_mixed(p, r));
  store_proj(4u * i + 2u, pdbl(p));
  store_proj(4u * i + 3u, padd(p, p));
}
"#;

/// Storage accessors for `F` (unrolled word copies).
fn accessors(kind: CurveKind) -> String {
    let mut s = String::new();
    let w = &mut s;
    let bigint_ld = |w: &mut String, name: &str, buf: &str| {
        writeln!(w, "fn {name}(o: u32) -> BigInt {{\n  var r: BigInt;").unwrap();
        for j in 0..LIMBS {
            writeln!(w, "  r.l[{j}] = {buf}[o + {j}u];").unwrap();
        }
        writeln!(w, "  return r;\n}}").unwrap();
    };
    bigint_ld(w, "ld_base_bi", "bases");
    bigint_ld(w, "ld_scr_bi", "scratch");
    writeln!(w, "fn st_scr_bi(o: u32, v: BigInt) {{").unwrap();
    for j in 0..LIMBS {
        writeln!(w, "  scratch[o + {j}u] = v.l[{j}];").unwrap();
    }
    writeln!(w, "}}").unwrap();
    match kind {
        CurveKind::G1 => {
            w.push_str("fn ld_base(o: u32) -> F { return ld_base_bi(o); }\n");
            w.push_str("fn ld_scr(o: u32) -> F { return ld_scr_bi(o); }\n");
            w.push_str("fn st_scr(o: u32, v: F) { st_scr_bi(o, v); }\n");
        }
        CurveKind::G2 => {
            writeln!(w, "fn ld_base(o: u32) -> F {{ return Fq2(ld_base_bi(o), ld_base_bi(o + {LIMBS}u)); }}").unwrap();
            writeln!(w, "fn ld_scr(o: u32) -> F {{ return Fq2(ld_scr_bi(o), ld_scr_bi(o + {LIMBS}u)); }}").unwrap();
            writeln!(w, "fn st_scr(o: u32, v: F) {{ st_scr_bi(o, v.c0); st_scr_bi(o + {LIMBS}u, v.c1); }}").unwrap();
        }
    }
    s
}

/// Threads per workgroup of the MSM kernels.
pub const WORKGROUP_SIZE: u32 = 64;

/// Complete WGSL module for one curve.
pub fn msm_module(kind: CurveKind) -> String {
    let mut s = fq_wgsl(&fq_consts());
    match kind {
        CurveKind::G1 => s.push_str(G1_F),
        CurveKind::G2 => {
            let b3: Fq2 = ark_bn254::g2::Config::COEFF_B * Fq2::from(3u64);
            s.push_str(&g2_f(&fq2_const_words(&b3)));
        }
    }
    s.push_str(
        &CURVE_AND_KERNELS
            .replace("{{FW}}", &kind.fw().to_string())
            .replace("{{WG}}", &WORKGROUP_SIZE.to_string()),
    );
    s.push_str(&accessors(kind));
    s
}

/// snarkjs' QAP witness map on the GPU, over Fr (`fq_*` here is Fr arithmetic, same 20 × 13-bit
/// Montgomery form with R = 2^260). Buffers: `raw` = a ‖ b as arkworks words (8 × u32, Montgomery
/// R = 2^256), `x` = three length-n vectors (a, b, c) of GPU elements, `tw` = ω^k then ω^-k for
/// k < n/2, `coset` = g^k / n, `outp` = h as canonical 8 × u32 words. One dispatch per step:
/// `qap_load`, then per transform `qap_bitrev` + log n × `qap_stage`, `qap_coset` in between, and
/// `qap_finish`. Every kernel is straight-line (at most one field product chain per thread).
pub fn qap_module() -> String {
    let c = fr_consts();
    let mut s = fq_wgsl(&c);
    let w = &mut s;
    // 2^264 mod r as a plain integer: fq_mul(raw, K) = raw · 2^4 turns arkworks' Montgomery
    // words (v · 2^256) into ours (v · 2^260).
    let k: ark_bn254::Fr = ark_bn254::Fr::from(2u64).pow([264u64]);
    let k = crate::field::to_limbs(&ark_ff::PrimeField::into_bigint(k));
    writeln!(w, "const K264: BigInt = {};", arr(&k)).unwrap();
    let mut one = [0u32; LIMBS];
    one[0] = 1;
    writeln!(w, "const INT_ONE: BigInt = {};", arr(&one)).unwrap();
    w.push_str(QAP_KERNELS);
    for (name, buf) in [("ld_x", "x"), ("ld_tw", "tw"), ("ld_coset", "coset")] {
        writeln!(w, "fn {name}(o: u32) -> BigInt {{\n  var r: BigInt;").unwrap();
        for j in 0..LIMBS {
            writeln!(w, "  r.l[{j}] = {buf}[o + {j}u];").unwrap();
        }
        writeln!(w, "  return r;\n}}").unwrap();
    }
    writeln!(w, "fn st_x(o: u32, v: BigInt) {{").unwrap();
    for j in 0..LIMBS {
        writeln!(w, "  x[o + {j}u] = v.l[{j}];").unwrap();
    }
    writeln!(w, "}}").unwrap();
    // 8 × 32-bit words <-> 20 × 13-bit limbs
    writeln!(w, "fn ld_raw(o: u32) -> BigInt {{\n  var r: BigInt;").unwrap();
    for i in 0..LIMBS as u32 {
        let (wd, sh) = (i * LIMB_BITS / 32, i * LIMB_BITS % 32);
        if wd >= 8 {
            break;
        }
        write!(w, "  r.l[{i}] = ((raw[o + {wd}u] >> {sh}u)").unwrap();
        if sh + LIMB_BITS > 32 && wd + 1 < 8 {
            write!(w, " | (raw[o + {}u] << {}u)", wd + 1, 32 - sh).unwrap();
        }
        writeln!(w, ") & MASK;").unwrap();
    }
    writeln!(w, "  return r;\n}}").unwrap();
    writeln!(w, "fn st_out(o: u32, v: BigInt) {{\n  var r = array<u32, 8>();").unwrap();
    for i in 0..LIMBS as u32 {
        let (wd, sh) = (i * LIMB_BITS / 32, i * LIMB_BITS % 32);
        if wd < 8 {
            writeln!(w, "  r[{wd}] = r[{wd}] | (v.l[{i}] << {sh}u);").unwrap();
        }
        if sh + LIMB_BITS > 32 && wd + 1 < 8 {
            writeln!(w, "  r[{}] = r[{}] | (v.l[{i}] >> {}u);", wd + 1, wd + 1, 32 - sh).unwrap();
        }
    }
    for j in 0..8 {
        writeln!(w, "  outp[o + {j}u] = r[{j}];").unwrap();
    }
    writeln!(w, "}}").unwrap();
    s.replace("{{WG}}", &WORKGROUP_SIZE.to_string())
}

const QAP_KERNELS: &str = r#"
struct QapParams { count: u32, n: u32, log_n: u32, half: u32, tw_stride: u32, tw_off: u32, p6: u32, p7: u32 }
@group(0) @binding(0) var<storage, read> raw: array<u32>;
@group(0) @binding(1) var<storage, read_write> x: array<u32>;
@group(0) @binding(2) var<storage, read> tw: array<u32>;
@group(0) @binding(3) var<storage, read> coset: array<u32>;
@group(0) @binding(4) var<storage, read_write> outp: array<u32>;
@group(0) @binding(5) var<uniform> params: QapParams;

fn qidx(gid: vec3<u32>, nwg: vec3<u32>) -> u32 { return gid.x + gid.y * nwg.x * {{WG}}u; }

// a, b from arkworks words; c = a·b (equal to snarkjs' c: b is zero on the input rows).
@compute @workgroup_size({{WG}})
fn qap_load(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
  let i = qidx(gid, nwg);
  if (i >= params.count) { return; }
  let a = fq_mul(ld_raw(i * 8u), K264);
  let b = fq_mul(ld_raw((params.n + i) * 8u), K264);
  st_x(i * 20u, a);
  st_x((params.n + i) * 20u, b);
  st_x((2u * params.n + i) * 20u, fq_mul(a, b));
}

// Bit-reversal permutation of each of the three vectors (thread t: vector t / n, index t % n).
@compute @workgroup_size({{WG}})
fn qap_bitrev(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
  let t = qidx(gid, nwg);
  if (t >= params.count) { return; }
  let v = t / params.n;
  let i = t % params.n;
  let j = reverseBits(i) >> (32u - params.log_n);
  if (i < j) {
    let oi = (v * params.n + i) * 20u;
    let oj = (v * params.n + j) * 20u;
    let xi = ld_x(oi);
    st_x(oi, ld_x(oj));
    st_x(oj, xi);
  }
}

// One radix-2 decimation-in-time stage (butterflies of span `half`) on all three vectors.
@compute @workgroup_size({{WG}})
fn qap_stage(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
  let t = qidx(gid, nwg);
  if (t >= params.count) { return; }
  let hn = params.n / 2u;
  let v = t / hn;
  let k = t % hn;
  let idx = k % params.half;
  let i0 = v * params.n + (k / params.half) * 2u * params.half + idx;
  let i1 = i0 + params.half;
  let w = ld_tw((params.tw_off + idx * params.tw_stride) * 20u);
  let u = ld_x(i0 * 20u);
  let p = fq_mul(w, ld_x(i1 * 20u));
  st_x(i0 * 20u, fq_add(u, p));
  st_x(i1 * 20u, fq_sub(u, p));
}

// x[v][k] *= g^k / n.
@compute @workgroup_size({{WG}})
fn qap_coset(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
  let t = qidx(gid, nwg);
  if (t >= params.count) { return; }
  st_x(t * 20u, fq_mul(ld_x(t * 20u), ld_coset((t % params.n) * 20u)));
}

// h = a·b − c, out of Montgomery form (· 1 · 2^-260), as canonical 8 × u32 words.
@compute @workgroup_size({{WG}})
fn qap_finish(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
  let i = qidx(gid, nwg);
  if (i >= params.count) { return; }
  let h = fq_sub(fq_mul(ld_x(i * 20u), ld_x((params.n + i) * 20u)), ld_x((2u * params.n + i) * 20u));
  st_out(i * 8u, fq_mul(h, INT_ONE));
}
"#;

/// Field self-test kernel (tests only): `out[i] = op(a[i], b[i])` on Fq for op in
/// {mul, add, sub}, laid out as 3 consecutive results.
pub fn field_test_module() -> String {
    let mut s = fq_wgsl(&fq_consts());
    s.push_str(
        r#"
@group(0) @binding(0) var<storage, read> inp: array<u32>;
@group(0) @binding(1) var<storage, read_write> outp: array<u32>;
fn ld(o: u32) -> BigInt { var r: BigInt; for (var j = 0u; j < 20u; j++) { r.l[j] = inp[o + j]; } return r; }
fn st(o: u32, v: BigInt) { for (var j = 0u; j < 20u; j++) { outp[o + j] = v.l[j]; } }
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  let i = gid.x;
  if (i >= arrayLength(&inp) / 40u) { return; }
  let a = ld(i * 40u);
  let b = ld(i * 40u + 20u);
  st(i * 60u, fq_mul(a, b));
  st(i * 60u + 20u, fq_add(a, b));
  st(i * 60u + 40u, fq_sub(a, b));
}
"#,
    );
    s
}

