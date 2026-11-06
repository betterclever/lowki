//! snarkjs `.zkey` (Groth16) reader, ported from `ark-circom` 0.5.0 `src/zkey.rs`
//! (MIT/Apache-2.0, <https://github.com/arkworks-rs/circom-compat>). Changes vs upstream:
//! reads from an in-memory byte slice (browser-fetched bytes), returns errors instead of
//! panicking on malformed input, checks the curve/field, and skips the (slow) G2 subgroup
//! checks; every point is still checked to be on the curve. G1 on bn254 has cofactor 1, so
//! the on-curve check is also a subgroup check there.
//!
//! Sections: 1 header (prover type, 1 = Groth16), 2 Groth header, 3 IC, 4 coefficients,
//! 5 A, 6 B1, 7 B2, 8 C (= arkworks `l_query`), 9 H, 10 contributions.

use ark_bn254::{Bn254, Fq, Fq2, Fr, G1Affine, G2Affine};
use ark_ff::{BigInteger256, PrimeField, Zero};
use ark_groth16::{ProvingKey, VerifyingKey};
use ark_relations::r1cs::ConstraintMatrices;

use crate::Error;

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn at(buf: &'a [u8], pos: usize) -> Self {
        Self { buf, pos }
    }
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.buf.len());
        let end = end.ok_or_else(|| Error::Zkey("unexpected end of file".into()))?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }
    fn bigint(&mut self) -> Result<BigInteger256, Error> {
        let b = self.bytes(32)?;
        let mut limbs = [0u64; 4];
        for (i, l) in limbs.iter_mut().enumerate() {
            *l = u64::from_le_bytes(b[8 * i..8 * i + 8].try_into().unwrap());
        }
        Ok(BigInteger256::new(limbs))
    }
    /// Base-field element stored in Montgomery form (snarkjs "LEM"), same R as arkworks.
    fn fq(&mut self) -> Result<Fq, Error> {
        let b = self.bigint()?;
        if b >= Fq::MODULUS {
            return Err(Error::Zkey("Fq element not reduced".into()));
        }
        Ok(Fq::new_unchecked(b))
    }
    fn g1(&mut self) -> Result<G1Affine, Error> {
        let (x, y) = (self.fq()?, self.fq()?);
        if x.is_zero() && y.is_zero() {
            return Ok(G1Affine::identity());
        }
        let p = G1Affine::new_unchecked(x, y);
        if !p.is_on_curve() {
            return Err(Error::Zkey("G1 point not on curve".into()));
        }
        Ok(p)
    }
    fn g2(&mut self) -> Result<G2Affine, Error> {
        let x = Fq2::new(self.fq()?, self.fq()?);
        let y = Fq2::new(self.fq()?, self.fq()?);
        if x.is_zero() && y.is_zero() {
            return Ok(G2Affine::identity());
        }
        let p = G2Affine::new_unchecked(x, y);
        if !p.is_on_curve() {
            return Err(Error::Zkey("G2 point not on curve".into()));
        }
        Ok(p)
    }
}

/// Parsed Groth16 proving key + the A/B constraint matrices (C is implied by circom's QAP).
pub struct Zkey {
    pub pk: ProvingKey<Bn254>,
    pub matrices: ConstraintMatrices<Fr>,
    pub n_public: usize,
}

pub fn read_zkey(buf: &[u8]) -> Result<Zkey, Error> {
    let mut r = Reader::at(buf, 0);
    if r.bytes(4)? != b"zkey" {
        return Err(Error::Zkey("bad magic (not a .zkey file)".into()));
    }
    let _version = r.u32()?;
    let n_sections = r.u32()?;
    let mut sections: [Option<usize>; 11] = [None; 11];
    for _ in 0..n_sections {
        let id = r.u32()? as usize;
        let len = usize::try_from(r.u64()?).map_err(|_| Error::Zkey("section too large".into()))?;
        if id < sections.len() && sections[id].is_none() {
            sections[id] = Some(r.pos);
        }
        r.bytes(len)?;
    }
    let sec = |id: usize| -> Result<Reader<'_>, Error> {
        sections[id]
            .map(|p| Reader::at(buf, p))
            .ok_or_else(|| Error::Zkey(format!("missing section {id}")))
    };

    if sec(1)?.u32()? != 1 {
        return Err(Error::Zkey("not a Groth16 zkey".into()));
    }

    // Section 2: Groth16 header.
    let mut h = sec(2)?;
    let (n8q, q) = (h.u32()?, h.bigint()?);
    let (n8r, rr) = (h.u32()?, h.bigint()?);
    if n8q != 32 || n8r != 32 || q != Fq::MODULUS || rr != Fr::MODULUS {
        return Err(Error::Zkey("zkey is not over bn128".into()));
    }
    let n_vars = h.u32()? as usize;
    let n_public = h.u32()? as usize;
    let domain_size = h.u32()? as usize;
    if !domain_size.is_power_of_two() || n_public >= n_vars {
        return Err(Error::Zkey("bad Groth16 header".into()));
    }
    let alpha_g1 = h.g1()?;
    let beta_g1 = h.g1()?;
    let beta_g2 = h.g2()?;
    let gamma_g2 = h.g2()?;
    let delta_g1 = h.g1()?;
    let delta_g2 = h.g2()?;

    let g1s = |id: usize, n: usize| -> Result<Vec<G1Affine>, Error> {
        let mut s = sec(id)?;
        (0..n).map(|_| s.g1()).collect()
    };
    let gamma_abc_g1 = g1s(3, n_public + 1)?;
    let a_query = g1s(5, n_vars)?;
    let b_g1_query = g1s(6, n_vars)?;
    let b_g2_query = {
        let mut s = sec(7)?;
        (0..n_vars).map(|_| s.g2()).collect::<Result<Vec<_>, _>>()?
    };
    let l_query = g1s(8, n_vars - n_public - 1)?;
    let h_query = g1s(9, domain_size)?;

    // Section 4: coefficients of A and B. Values are stored multiplied by R^2 (i.e.
    // Montgomery form of a Montgomery-form value), hence the double conversion.
    let mut c = sec(4)?;
    let n_coeffs = c.u32()? as usize;
    let mut m: [Vec<Vec<(Fr, usize)>>; 2] = [vec![Vec::new(); domain_size], vec![Vec::new(); domain_size]];
    let mut max_constraint = 0usize;
    for _ in 0..n_coeffs {
        let matrix = c.u32()? as usize;
        let constraint = c.u32()? as usize;
        let signal = c.u32()? as usize;
        let raw = c.bigint()?;
        if matrix > 1 || constraint >= domain_size || signal >= n_vars || raw >= Fr::MODULUS {
            return Err(Error::Zkey("bad coefficient entry".into()));
        }
        let value = Fr::new_unchecked(Fr::new_unchecked(raw).into_bigint());
        max_constraint = max_constraint.max(constraint);
        m[matrix][constraint].push((value, signal));
    }
    // The last n_public+1 "constraints" are the public-input constraints that arkworks'
    // reduction adds itself; drop them (same as ark-circom).
    let num_constraints = max_constraint
        .checked_sub(n_public)
        .ok_or_else(|| Error::Zkey("bad constraint count".into()))?;
    if num_constraints + n_public + 1 > domain_size {
        return Err(Error::Zkey("domain too small".into()));
    }
    let [mut a, mut b] = m;
    a.truncate(num_constraints);
    b.truncate(num_constraints);
    let matrices = ConstraintMatrices {
        num_instance_variables: n_public + 1,
        // (ark-circom uses n_vars - n_public, counting the constant `1` twice; unused by the prover)
        num_witness_variables: n_vars - n_public - 1,
        num_constraints,
        a_num_non_zero: a.iter().map(Vec::len).sum(),
        b_num_non_zero: b.iter().map(Vec::len).sum(),
        c_num_non_zero: 0,
        a,
        b,
        c: vec![],
    };

    let vk = VerifyingKey::<Bn254> {
        alpha_g1,
        beta_g2,
        gamma_g2,
        delta_g2,
        gamma_abc_g1,
    };
    let pk = ProvingKey::<Bn254> {
        vk,
        beta_g1,
        delta_g1,
        a_query,
        b_g1_query,
        b_g2_query,
        h_query,
        l_query,
    };
    Ok(Zkey { pk, matrices, n_public })
}
