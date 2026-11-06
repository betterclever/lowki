//! Artifact pinning and zkey point validation (native).
//!
//! * `load` accepts only the sha256-pinned 1.0.5 artifacts; one flipped byte is rejected.
//! * The pinned zkeys pass the full G2 subgroup check (`load_unpinned` checks every proving-key
//!   G2 point). This is the one-time check that `load` relies on when it skips it for
//!   pinned bytes. Timings are printed with `--nocapture`.
//! * An on-curve G2 point outside the subgroup is rejected by `load_unpinned`.
#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;
use std::time::Instant;

use ark_bn254::{Fq2, G2Affine};
use ark_ff::UniformRand;
use lowki_groth16::*;
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;

fn read(name: &str) -> Vec<u8> {
    let dir = std::env::var("LOWKI_PP_ARTIFACTS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from("/Users/betterclever/projects/ethglobal/vendor/npm/circuits/package/artifacts")
        });
    std::fs::read(dir.join(name)).unwrap()
}

/// Byte offset of a zkey section's payload.
fn section(buf: &[u8], want: u32) -> usize {
    let u32_at = |p: usize| u32::from_le_bytes(buf[p..p + 4].try_into().unwrap());
    let mut pos = 12;
    for _ in 0..u32_at(8) {
        let (id, len) = (
            u32_at(pos),
            u64::from_le_bytes(buf[pos + 4..pos + 12].try_into().unwrap()) as usize,
        );
        if id == want {
            return pos + 12;
        }
        pos += 12 + len;
    }
    panic!("no section {want}");
}

#[test]
fn load_rejects_unpinned_artifacts() {
    for (c, n) in [(Circuit::Commitment, "commitment"), (Circuit::Withdraw, "withdraw")] {
        let (wasm, zkey) = (read(&format!("{n}.wasm")), read(&format!("{n}.zkey")));
        let t = Instant::now();
        let p = CircuitProver::load(c, &wasm, &zkey).unwrap();
        eprintln!("{n}: pinned load {:?}", t.elapsed());
        drop(p);
        let mut bad = zkey.clone();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        let e = CircuitProver::load(c, &wasm, &bad).err().unwrap();
        assert!(
            matches!(&e, Error::Circuit(m) if m.contains("sha256") && m.contains(".zkey")),
            "{e}"
        );
        let mut bad = wasm.clone();
        bad[100] ^= 1;
        let e = CircuitProver::load(c, &bad, &zkey).err().unwrap();
        assert!(
            matches!(&e, Error::Circuit(m) if m.contains("sha256") && m.contains(".wasm")),
            "{e}"
        );
        eprintln!("{n}: {e}");
    }
    // Swapped circuits: rejected by the pin (and, unpinned, by the structural checks).
    let (cw, cz, ww) = (read("commitment.wasm"), read("commitment.zkey"), read("withdraw.wasm"));
    assert!(matches!(
        CircuitProver::load(Circuit::Withdraw, &cw, &cz),
        Err(Error::Circuit(_))
    ));
    assert!(CircuitProver::load_unpinned(Circuit::Commitment, &ww, &cz).is_err());
    assert!(CircuitProver::load_unpinned(Circuit::Withdraw, &cw, &cz).is_err());
}

#[test]
fn pinned_zkeys_pass_full_g2_subgroup_checks() {
    for (c, n) in [(Circuit::Commitment, "commitment"), (Circuit::Withdraw, "withdraw")] {
        let (wasm, zkey) = (read(&format!("{n}.wasm")), read(&format!("{n}.zkey")));
        let t = Instant::now();
        CircuitProver::load_unpinned(c, &wasm, &zkey).unwrap();
        eprintln!("{n}: unpinned load (all G2 points subgroup-checked) {:?}", t.elapsed());
    }
}

#[test]
fn rejects_g2_point_outside_subgroup() {
    let (wasm, mut zkey) = (read("commitment.wasm"), read("commitment.zkey"));
    // An on-curve point of the twist that is not in the r-order subgroup.
    let mut rng = ChaCha20Rng::seed_from_u64(7);
    let p = loop {
        if let Some(p) = G2Affine::get_point_from_x_unchecked(Fq2::rand(&mut rng), false)
            && p.is_on_curve()
            && !p.is_in_correct_subgroup_assuming_on_curve()
        {
            break p;
        }
    };
    // Overwrite b_g2_query[1] (section 7; [0] is the constant signal) with it, Montgomery LE limbs.
    let off = section(&zkey, 7) + 128;
    for (i, f) in [p.x.c0, p.x.c1, p.y.c0, p.y.c1].iter().enumerate() {
        for (j, limb) in f.0.0.iter().enumerate() {
            zkey[off + 32 * i + 8 * j..off + 32 * i + 8 * j + 8].copy_from_slice(&limb.to_le_bytes());
        }
    }
    let e = CircuitProver::load_unpinned(Circuit::Commitment, &wasm, &zkey)
        .err()
        .unwrap();
    assert!(matches!(&e, Error::Zkey(m) if m.contains("subgroup")), "{e}");
    // And the pinned loader refuses the modified file before parsing it.
    assert!(matches!(
        CircuitProver::load(Circuit::Commitment, &wasm, &zkey),
        Err(Error::Circuit(_))
    ));
}
