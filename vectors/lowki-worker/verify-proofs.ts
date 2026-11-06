// snarkjs 0.7.5 groth16.verify on proofs made by lowki-worker (native test or headless
// Chrome). Input: JSON array or JSON-lines of {circuit, name, proof, publicSignals}.
//   npx tsx verify-proofs.ts out/native-proofs.jsonl out/worker-proofs.jsonl
import { readFileSync } from 'node:fs';
import * as snarkjs from 'snarkjs';

const ART = process.env.LOWKI_PP_ARTIFACTS ?? new URL('../../../vendor/npm/circuits/package/artifacts/', import.meta.url).pathname;
export function load(f: string): any[] {
  const t = readFileSync(f, 'utf8').trim();
  return t.startsWith('[') ? JSON.parse(t) : t.split('\n').filter(Boolean).map((l) => JSON.parse(l));
}
let bad = 0;
let n = 0;
for (const f of process.argv.slice(2)) {
  for (const e of load(f)) {
    const vk = JSON.parse(readFileSync(`${ART}/${e.circuit}.vkey`, 'utf8'));
    const ok = await snarkjs.groth16.verify(vk, e.publicSignals, e.proof);
    const t = [...e.publicSignals];
    t[0] = (BigInt(t[0]) + 1n).toString();
    const tampered = await snarkjs.groth16.verify(vk, t, e.proof);
    n++;
    if (!ok || tampered) bad++;
    console.log(`${ok && !tampered ? 'PASS' : 'FAIL'} ${f}:${e.name} snarkjs=${ok} tampered=${tampered}`);
  }
}
console.log(bad ? `FAILED ${bad}/${n}` : `snarkjs verified ${n}/${n} lowki-worker proofs`);
process.exit(bad ? 1 : 0);
