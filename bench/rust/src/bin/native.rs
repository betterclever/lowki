//! Native harness: same cases and the same sampling rules as js/harness.ts
//! (warm-up >= 3 samples and >= 300 ms, then 10..200 samples within ~2.5 s, or fixed counts).
//! Usage: native <out.json> [--no-micro] [--no-g16] [--g16-iters N]
use serde_json::{Value, json};
use std::time::Instant;

fn stats(samples: &[f64], batch: usize) -> Value {
    let mut s = samples.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = s.len();
    let med = if n % 2 == 1 { s[n / 2] } else { (s[n / 2 - 1] + s[n / 2]) / 2.0 };
    let q90 = s[((0.9 * n as f64).ceil() as usize).max(1) - 1];
    let b = batch as f64;
    json!({ "median_ms": med / b, "p90_ms": q90 / b, "min_ms": s[0] / b, "mean_ms": s.iter().sum::<f64>() / n as f64 / b })
}

fn time_samples(mut f: impl FnMut(), warmup: Option<usize>, iters: Option<usize>) -> (Vec<f64>, usize) {
    let tw = Instant::now();
    let mut w = 0;
    let min_w = warmup.unwrap_or(3);
    while w < min_w || (warmup.is_none() && tw.elapsed().as_secs_f64() < 0.3 && w < 50) {
        f();
        w += 1;
    }
    let (min_n, max_n) = (iters.unwrap_or(10), iters.unwrap_or(200));
    let t0 = Instant::now();
    let mut v = Vec::new();
    while v.len() < min_n || (v.len() < max_n && t0.elapsed().as_secs_f64() < 2.5) {
        let t = Instant::now();
        f();
        v.push(t.elapsed().as_secs_f64() * 1e3);
    }
    (v, w)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out_path = args.get(1).expect("usage: native <out.json> [--no-micro] [--no-g16] [--g16-iters N]").clone();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let g16_iters: usize = args.iter().position(|a| a == "--g16-iters").map(|i| args[i + 1].parse().unwrap()).unwrap_or(10);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let read = |n: &str| std::fs::read_to_string(root.join("fixtures").join(n)).unwrap();
    let mut results: Vec<Value> = Vec::new();

    if !flag("--no-micro") {
        let fx = json!({
            "keys": serde_json::from_str::<Value>(&read("keys.json")).unwrap(),
            "pp": serde_json::from_str::<Value>(&read("pp.json")).unwrap(),
            "stealth": serde_json::from_str::<Value>(&read("stealth.json")).unwrap(),
            "aa": serde_json::from_str::<Value>(&read("aa.json")).unwrap(),
        });
        for mut c in lowki_bench::cases::cases(&fx) {
            let (samples, w) = time_samples(&mut c.run, c.warmup, c.iters);
            let mut st = stats(&samples, c.batch);
            st["id"] = json!(c.id);
            st["batch"] = json!(c.batch);
            st["n"] = json!(samples.len());
            st["warmup"] = json!(w);
            st["check"] = json!((c.check)());
            eprintln!("{}: median {:.4} ms/op  p90 {:.4} (n={})", c.id, st["median_ms"].as_f64().unwrap(), st["p90_ms"].as_f64().unwrap(), samples.len());
            results.push(st);
        }
    }

    #[cfg(feature = "g16")]
    if !flag("--no-g16") {
        use lowki_bench::g16::*;
        let art = std::env::var("LOWKI_PP_ARTIFACTS")
            .unwrap_or_else(|_| root.join("../../vendor/npm/circuits/package/artifacts").to_string_lossy().into());
        let g16: Value = serde_json::from_str(&read("g16.json")).unwrap();
        let mode = if cfg!(feature = "parallel") { "native_par" } else { "native_1t" };
        let mut proofs = Vec::new();
        for name in ["commitment", "withdraw"] {
            let wasm = std::fs::read(format!("{art}/{name}.wasm")).unwrap();
            let zkey = std::fs::read(format!("{art}/{name}.zkey")).unwrap();
            let input = g16[name]["input"].to_string();
            let t = Instant::now();
            let p = lowki_groth16::CircuitProver::load(circuit(name), &wasm, &zkey).unwrap();
            let load_ms = t.elapsed().as_secs_f64() * 1e3;
            let (ws, _) = time_samples(|| { std::hint::black_box(witness(&p, &input)); }, Some(2), Some(g16_iters));
            let (ps, _) = time_samples(|| { std::hint::black_box(prove(&p, &input)); }, Some(2), Some(g16_iters));
            let out = prove(&p, &input);
            let ok = lowki_groth16::verify(p.verifying_key(), &out.proof, &out.public_signals).unwrap();
            let mut st = stats(&ps, 1);
            st["id"] = json!(format!("g16.{name}.{mode}.cpu"));
            st["n"] = json!(ps.len());
            st["witness"] = stats(&ws, 1);
            st["load_ms"] = json!(load_ms);
            st["verified_rust"] = json!(ok);
            st["check"] = json!(out.public_signals.join(","));
            eprintln!("{}: prove median {:.1} ms p90 {:.1}, witness {:.1} ms, load {:.1} ms, verify={ok}", st["id"], st["median_ms"].as_f64().unwrap(), st["p90_ms"].as_f64().unwrap(), st["witness"]["median_ms"].as_f64().unwrap(), load_ms);
            results.push(st);
            proofs.push(json!({ "id": format!("g16.{name}.{mode}.cpu"), "circuit": name, "out": serde_json::from_str::<Value>(&to_json(&out)).unwrap() }));

            #[cfg(feature = "webgpu")]
            if !cfg!(feature = "parallel") {
                let t = Instant::now();
                let gpu = pollster::block_on(lowki_groth16::open_gpu()).expect("no GPU");
                let open_ms = t.elapsed().as_secs_f64() * 1e3;
                let t = Instant::now();
                let gp = lowki_groth16::GpuProver::new(gpu, lowki_groth16::CircuitProver::load(circuit(name), &wasm, &zkey).unwrap()).map_err(|e| e.1).unwrap();
                let up_ms = t.elapsed().as_secs_f64() * 1e3;
                let (gs, _) = time_samples(|| { std::hint::black_box(pollster::block_on(prove_gpu(&gp, &input))); }, Some(2), Some(g16_iters));
                let out = pollster::block_on(prove_gpu(&gp, &input));
                let ok = lowki_groth16::verify(gp.cpu().verifying_key(), &out.proof, &out.public_signals).unwrap();
                let mut st = stats(&gs, 1);
                st["id"] = json!(format!("g16.{name}.native.metal"));
                st["n"] = json!(gs.len());
                st["gpu_open_ms"] = json!(open_ms);
                st["upload_ms"] = json!(up_ms);
                st["verified_rust"] = json!(ok);
                st["check"] = json!(out.public_signals.join(","));
                eprintln!("{}: prove median {:.1} ms p90 {:.1}, open {:.0} ms, upload {:.0} ms, verify={ok}", st["id"], st["median_ms"].as_f64().unwrap(), st["p90_ms"].as_f64().unwrap(), open_ms, up_ms);
                results.push(st);
                proofs.push(json!({ "id": format!("g16.{name}.native.metal"), "circuit": name, "out": serde_json::from_str::<Value>(&to_json(&out)).unwrap() }));
            }
        }
        std::fs::write(format!("{out_path}.proofs.json"), serde_json::to_string_pretty(&proofs).unwrap()).unwrap();
    }

    std::fs::write(&out_path, serde_json::to_string_pretty(&results).unwrap()).unwrap();
}
