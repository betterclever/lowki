// Rust seal -> JS open. Reads out/sealed.json (written by `cargo test --test flow
// seal_app_key_writes_js_check`), opens each HPKE ciphertext with @hpke/core 1.10.0 +
// @hpke/chacha20poly1305 1.8.0 (RFC 9180: DHKEM(X25519,HKDF-SHA256), HKDF-SHA256,
// ChaCha20-Poly1305, base mode, info "lowki/delegation/v1", aad = grantHash), and checks:
// the plaintext is the app key of the grant's account, grantHash == viem hashTypedData(grant),
// the grant signature recovers to the account, and a tampered aad fails to open.
import { readFileSync } from 'node:fs';
import { CipherSuite, DhkemX25519HkdfSha256, HkdfSha256 } from '@hpke/core';
import { Chacha20Poly1305 } from '@hpke/chacha20poly1305';
import { hashTypedData, recoverTypedDataAddress, hexToBytes, bytesToHex } from 'viem';
import { privateKeyToAccount } from 'viem/accounts';

const s = JSON.parse(readFileSync(new URL('./out/sealed.json', import.meta.url), 'utf8'));
const suite = new CipherSuite({ kem: new DhkemX25519HkdfSha256(), kdf: new HkdfSha256(), aead: new Chacha20Poly1305() });
const skp = await suite.kem.importKey('raw', hexToBytes(s.enclaveSecretKey).buffer as ArrayBuffer, false);
const info = new TextEncoder().encode('lowki/delegation/v1');
let ok = 0;
for (const c of s.cases) {
  const r = c.sealed;
  const { EIP712Domain, ...types } = r.grant.types;
  const td = { ...r.grant, types };
  const h = hashTypedData(td);
  if (h !== r.grantHash) throw new Error(`grantHash mismatch ${h} ${r.grantHash}`);
  const signer = await recoverTypedDataAddress({ ...td, signature: r.grantSignature });
  if (signer !== r.address) throw new Error('grant signer mismatch');
  const recipient = await suite.createRecipientContext({ recipientKey: skp, enc: hexToBytes(r.enc).buffer as ArrayBuffer, info });
  const pt = new Uint8Array(await recipient.open(hexToBytes(r.ciphertext).buffer as ArrayBuffer, hexToBytes(r.grantHash).buffer as ArrayBuffer));
  const acct = privateKeyToAccount(bytesToHex(pt));
  if (acct.address !== r.address) throw new Error('plaintext is not the account key');
  const bad = hexToBytes(r.grantHash);
  bad[0] ^= 1;
  const r2 = await suite.createRecipientContext({ recipientKey: skp, enc: hexToBytes(r.enc).buffer as ArrayBuffer, info });
  let opened = true;
  try { await r2.open(hexToBytes(r.ciphertext).buffer as ArrayBuffer, bad.buffer as ArrayBuffer); } catch { opened = false; }
  if (opened) throw new Error('tampered aad opened');
  ok++;
  console.log(`PASS ${c.origin}: opened app key of ${r.address}, grant hash+signature ok, tampered aad rejected`);
}
console.log(`HPKE: ${ok}/${s.cases.length} sealed app keys opened by @hpke/core`);
