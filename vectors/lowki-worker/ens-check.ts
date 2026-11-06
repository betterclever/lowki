// Checks the worker's ENS / stealth outputs (written by `cargo test --release --test ens` to
// out/ens-worker.json and out/stealth-tx.json) against viem: identity addresses (@scure bip32 at
// m/137'/0'/i'), the LowkiRegistrar `Register` and LowkiOwnerAccount `Execute` EIP-712
// signatures, the EIP-7702 authorisation, the records-auth personal_sign, and the transaction
// signed from a stealth address.
import { readFileSync } from 'node:fs'
import { mnemonicToAccount } from 'viem/accounts'
import { recoverAuthorizationAddress } from 'viem/utils'
import { parseTransaction, recoverMessageAddress, recoverTransactionAddress,
  verifyTypedData, zeroAddress } from 'viem'

const v = JSON.parse(readFileSync(new URL('./out/ens-worker.json', import.meta.url), 'utf8'))
let n = 0
const ok = (c: unknown, m: string) => { if (!c) throw new Error('FAIL ' + m); n++ }
for (const c of v.cases) {
  const acct = mnemonicToAccount(c.mnemonic, { path: `m/137'/0'/${c.index}'` as any })
  ok(acct.address === c.identity.address, `identity ${c.index}`)
  ok(await verifyTypedData({ address: acct.address, signature: c.registration.signature,
    domain: { name: 'Lowki Registrar', version: '1', chainId: v.chainId, verifyingContract: v.registrar },
    types: { Register: [{ name: 'label', type: 'string' }, { name: 'owner', type: 'address' }, { name: 'resolver', type: 'address' },
      { name: 'duration', type: 'uint64' }, { name: 'nonce', type: 'uint256' }, { name: 'deadline', type: 'uint256' }] },
    primaryType: 'Register', message: { label: 'test-abc', owner: acct.address, resolver: zeroAddress, duration: 31536000n, nonce: 0n, deadline: 1900000000n } }), 'Register')
  ok(await verifyTypedData({ address: acct.address, signature: c.setup.executeSignature,
    domain: { name: 'Lowki Owner Account', version: '1', chainId: v.chainId, verifyingContract: acct.address },
    types: { Execute: [{ name: 'calls', type: 'Call[]' }, { name: 'nonce', type: 'uint256' }, { name: 'deadline', type: 'uint256' }],
      Call: [{ name: 'target', type: 'address' }, { name: 'value', type: 'uint256' }, { name: 'data', type: 'bytes' }] },
    primaryType: 'Execute', message: { calls: c.setupCalls.map((x: any) => ({ ...x, value: 0n })), nonce: 0n, deadline: 1900000000n } }), 'Execute')
  const a = c.setup.authorization
  ok(await recoverAuthorizationAddress({ authorization: { address: a.address, chainId: Number(a.chainId), nonce: Number(a.nonce), r: a.r, s: a.s, yParity: a.yParity } }) === acct.address, '7702 authorisation')
  const viewing = mnemonicToAccount(c.mnemonic, { path: `m/5564'/0'/1'` as any })
  ok(await recoverMessageAddress({ message: `Lowki stealth records\nname: test-abc.lowki.eth\ntimestamp: 1800000000`, signature: c.recordsAuth.signature }) === viewing.address, 'records auth')
}
const t = JSON.parse(readFileSync(new URL('./out/stealth-tx.json', import.meta.url), 'utf8'))
ok(await recoverTransactionAddress({ serializedTransaction: t.signed.rawTransaction }) === t.from, 'stealth tx signer')
ok(parseTransaction(t.signed.rawTransaction).value === 12345n, 'stealth tx value')
console.log(`ens-check: ${n} checks passed`)
