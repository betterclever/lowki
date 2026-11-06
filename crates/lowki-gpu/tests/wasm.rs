//! GPU MSM differential tests inside wasm32 in a real browser (WebGPU), e.g. headless Chrome:
//!
//!   cargo test --release --target wasm32-unknown-unknown --no-run
//!   (cd browser && CHROMEDRIVER=... wasm-bindgen-test-runner <test .wasm>)   # see README
//!
//! Without WebGPU (e.g. Node) the tests print `SKIP` and pass.
#![cfg(target_arch = "wasm32")]

use ark_bn254::{Fr, G1Affine, G1Projective, G2Projective};
use ark_ec::{CurveGroup, VariableBaseMSM};
use ark_ff::{UniformRand, Zero};
use lowki_gpu::*;
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn now() -> f64 {
    js_sys::Date::now()
}

async fn gpu() -> Option<Gpu> {
    let t = now();
    match Gpu::new().await {
        Ok(g) => {
            let i = g.adapter_info();
            console_log!(
                "LOWKI_GPU adapter name={:?} vendor={:?} device={:?} backend={:?} type={:?} driver={:?} maxStorageBufferBindingSize={}MiB setup={:.0}ms",
                i.name, i.vendor, i.device, i.backend, i.device_type, i.driver_info, g.max_storage_binding() >> 20, now() - t
            );
            Some(g)
        }
        Err(e) => {
            console_log!("SKIP: {e}");
            None
        }
    }
}

#[wasm_bindgen_test]
async fn wasm_gpu_msm_matches_ark() {
    let Some(g) = gpu().await else { return };
    let mut rng = ChaCha20Rng::seed_from_u64(11);
    for &n in &[1usize, 2, 3, 255, 256, 1000, 1 << 16] {
        let p = G1Projective::rand(&mut rng);
        let mut bases: Vec<G1Affine> =
            G1Projective::normalize_batch(&(0..n).map(|i| p * Fr::from(i as u64 + 1)).collect::<Vec<_>>());
        if n >= 3 {
            bases[0] = G1Affine::identity();
            bases[1] = bases[2];
        }
        let mut sc: Vec<Fr> = (0..n).map(|_| Fr::rand(&mut rng)).collect();
        if n >= 3 {
            sc[1] = -Fr::from(1u64);
            sc[2] = Fr::zero();
        }
        let gb = g.upload_bases(&bases).unwrap();
        for scal in [sc.clone(), vec![Fr::zero(); n]] {
            let t = now();
            let got = g.msm_g1(&gb, &scal).await.unwrap().into_affine();
            let dt = now() - t;
            let want = G1Projective::msm(&bases, &scal).unwrap().into_affine();
            assert_eq!(got, want, "G1 n={n}");
            console_log!("LOWKI_MSM G1 n={n} ok gpu={dt:.0}ms");
        }
    }
    let q = G2Projective::rand(&mut rng);
    let b2 = G2Projective::normalize_batch(&(0..1000u64).map(|i| q * Fr::from(i + 1)).collect::<Vec<_>>());
    let sc: Vec<Fr> = (0..1000).map(|_| Fr::rand(&mut rng)).collect();
    let gb = g.upload_bases(&b2).unwrap();
    let got = g.msm_g2(&gb, &sc).await.unwrap().into_affine();
    assert_eq!(got, G2Projective::msm(&b2, &sc).unwrap().into_affine());
    console_log!("LOWKI_MSM G2 n=1000 ok");
}
