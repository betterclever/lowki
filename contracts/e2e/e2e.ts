// End-to-end: Lowki ENSv2 stealth names on an anvil fork of Sepolia (real ENSv2 contracts).
//
// Roles: keys, calldata, EIP-712/EIP-7702 signatures, record parsing, stealth derivation and
// scanning are all Rust (lowki-ens + lowki-stealth, through the `ens_e2e` example binary).
// viem is used only to (a) send transactions as the operator/sender and (b) play an UNMODIFIED
// wallet: `getEnsAddress({ name })` with no custom CCIP handling, as MetaMask would.
// The ScopeLift SDK is the independent reference for the client-side derivation.
//
// Inputs come from run.sh (env): RPC, deployment JSON paths, gateway URL, operator key, CLI.
// Nothing here is hardcoded: addresses come from the deployment files, keys are random per run.

import { execFileSync } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { generateStealthAddress } from '@scopelift/stealth-address-sdk'
import {
  type Address,
  type Hex,
  createPublicClient,
  hexToBytes,
  createWalletClient,
  http,
  keccak256,
  namehash,
  parseAbi,
  parseEther,
  toHex,
  verifyMessage,
  zeroAddress,
} from 'viem'
import { privateKeyToAccount } from 'viem/accounts'
import { sepolia } from 'viem/chains'
import { packetToBytes } from 'viem/ens'

const env = (k: string) => {
  const v = process.env[k]
  if (!v) throw new Error(`missing env ${k}`)
  return v
}
const RPC = env('E2E_RPC')
const CLI = env('E2E_CLI')
const GATEWAY_BASE = env('E2E_GATEWAY_BASE') // e.g. http://127.0.0.1:8787
const ens = JSON.parse(readFileSync(env('E2E_ENS_CONFIG'), 'utf8')).ens as Record<string, Address>
const lowki = JSON.parse(readFileSync(env('E2E_LOWKI_DEPLOYMENT'), 'utf8')) as Record<string, any>
const operator = privateKeyToAccount(env('E2E_OPERATOR_KEY') as Hex)
const LABEL = process.env.E2E_LABEL ?? `alice${randomBytes(3).toString('hex')}`
const NAME = `${LABEL}.${lowki.parentName}`

const results: Record<string, unknown> = { name: NAME }
const step = (n: string) => console.log(`\n=== ${n}`)
const ok = (cond: unknown, msg: string) => {
  if (!cond) throw new Error(`FAILED: ${msg}`)
  console.log(`  ok  ${msg}`)
}

function cli<T = any>(cmd: string, input: unknown): T {
  const out = execFileSync(CLI, [cmd], { input: JSON.stringify(input), encoding: 'utf8' })
  return JSON.parse(out) as T
}

// An unmodified client: chain = sepolia (so the UniversalResolver is viem's built-in default,
// the 0xeeee…eeee proxy), transport = the fork. No ccipRead override.
const plain = createPublicClient({ chain: sepolia, transport: http(RPC) })
const operatorWallet = createWalletClient({ account: operator, chain: sepolia, transport: http(RPC) })

const registryAbi = parseAbi([
  'function getResolver(string label) view returns (address)',
  'function getState(uint256 anyId) view returns ((uint8 status, uint64 expiry, address latestOwner, uint256 tokenId, uint256 resource))',
  'function isEmancipated() view returns (bool)',
])
const registrarAbi = parseAbi(['function nonces(address) view returns (uint256)'])
const urAbi = parseAbi([
  'function resolve(bytes name, bytes data) view returns (bytes, address)',
])

async function send(to: Address, data: Hex, extra: Record<string, unknown> = {}) {
  // Pad the estimate: on Sepolia's current gas schedule (state creation priced separately)
  // node estimates for 7702 batches that deploy contracts come out short.
  const estimate = await plain.estimateGas({ account: operator, to, data, ...extra } as any)
  const hash = await operatorWallet.sendTransaction({ to, data, gas: (estimate * 13n) / 10n, ...extra } as any)
  const r = await plain.waitForTransactionReceipt({ hash })
  if (r.status !== 'success') throw new Error(`tx reverted: ${hash}`)
  return r
}

async function main() {
  const chainId = await plain.getChainId()
  const now = Number((await plain.getBlock()).timestamp)

  step('0. setup is what the scripts left: emancipated Lowki registry under ' + lowki.parentName)
  ok(await plain.readContract({ address: lowki.LowkiRegistry, abi: registryAbi, functionName: 'isEmancipated' }), 'Lowki registry isEmancipated()')
  const parentResolver = await plain.getEnsResolver({ name: lowki.parentName })
  ok(parentResolver.toLowerCase() === lowki.StealthResolver.toLowerCase(), `${lowki.parentName} resolver = StealthResolver ${lowki.StealthResolver}`)
  ok((await plain.getEnsText({ name: lowki.parentName, key: 'description' }))?.startsWith('Lowki'), 'parent text record resolves through StealthResolver -> Lowki PermissionedResolver')

  // Fresh wallet seed per run (test only). The owner key, spending and viewing keys derive from it in Rust.
  const seed = toHex(randomBytes(64))
  const keys = cli('keys', { seed })
  console.log(`  owner (m/137'/0'/0') ${keys.owner}`)
  console.log(`  meta-address ${keys.meta_uri.slice(0, 24)}…`)
  ok((await plain.getBalance({ address: keys.owner })) === 0n, 'owner key has 0 ETH')

  step(`2. sponsored registration of ${NAME} (owner signs EIP-712, operator pays)`)
  const nonce = await plain.readContract({ address: lowki.LowkiRegistrar, abi: registrarAbi, functionName: 'nonces', args: [keys.owner] })
  const reg = cli('sign-registration', {
    seed, chain_id: chainId, registrar: lowki.LowkiRegistrar, label: LABEL,
    resolver: zeroAddress, duration: 365 * 86400, nonce: Number(nonce), deadline: now + 3600,
  })
  const regReceipt = await send(lowki.LowkiRegistrar, reg.calldata)
  const state = await plain.readContract({ address: lowki.LowkiRegistry, abi: registryAbi, functionName: 'getState', args: [BigInt(keccak256(toHex(LABEL)))] })
  ok(state.status === 2 && state.latestOwner.toLowerCase() === keys.owner.toLowerCase(), `registered to the owner (tx ${regReceipt.transactionHash}, gas ${regReceipt.gasUsed})`)
  results.registration = { tx: regReceipt.transactionHash, gasUsed: regReceipt.gasUsed.toString() }

  step('3. owner sets up its own resolver + ENSIP-30 record (one sponsored EIP-7702 batch)')
  const setup = cli('owner-setup', {
    seed, chain_id: chainId, name: NAME,
    owner_account: lowki.LowkiOwnerAccount,
    auth_nonce: await plain.getTransactionCount({ address: keys.owner }),
    exec_nonce: 0, deadline: now + 3600,
    factory: ens.VerifiableFactory, permissioned_resolver_impl: ens.PermissionedResolverImpl,
    stealth_resolver: lowki.StealthResolver, registry: lowki.LowkiRegistry,
    gateway_url: lowki.gatewayUrl,
  })
  const setupReceipt = await send(keys.owner, setup.calldata, {
    authorizationList: [{ ...setup.authorization, chainId: Number(setup.authorization.chainId), nonce: Number(setup.authorization.nonce) }],
  })
  ok((await plain.getCode({ address: keys.owner }))?.startsWith('0xef0100'), 'owner EOA is 7702-delegated to LowkiOwnerAccount')
  ok((await plain.readContract({ address: lowki.LowkiRegistry, abi: registryAbi, functionName: 'getResolver', args: [LABEL] })).toLowerCase() === lowki.StealthResolver.toLowerCase(), 'owner pointed the name at StealthResolver (ROLE_SET_RESOLVER)')
  ok((await plain.getBalance({ address: keys.owner })) === 0n, 'owner key still has 0 ETH')
  results.ownerSetup = { tx: setupReceipt.transactionHash, gasUsed: setupReceipt.gasUsed.toString(), recordsResolver: setup.records_resolver }

  step('4. client-side path: lowki-ens reads the record, lowki-stealth derives; compare with ScopeLift SDK')
  const eph = toHex(randomBytes(32))
  const cs = cli('read-derive', { rpc: RPC, universal_resolver: ens.UniversalResolver, name: NAME, ephemeral: eph })
  ok(cs.client_side_allowed === true, `record stealth-meta-address[1][60] = 0x01 || meta (${(cs.record.length - 2) / 2} bytes), client-side derivation permitted`)
  ok(cs.meta_uri === keys.meta_uri, 'record holds the owner\'s meta-address')
  const sdk = generateStealthAddress({ stealthMetaAddressURI: cs.meta_uri, schemeId: 1, ephemeralPrivateKey: hexToBytes(eph) })
  ok(sdk.stealthAddress.toLowerCase() === cs.stealth_address.toLowerCase(), `Rust ${cs.stealth_address} == ScopeLift SDK ${sdk.stealthAddress}`)
  ok(sdk.ephemeralPublicKey.toLowerCase() === cs.ephemeral_pubkey.toLowerCase() && Number(sdk.viewTag) === cs.view_tag, 'ephemeral public key and view tag match')
  const viaViem = await plain.readContract({ address: ens.UniversalResolver, abi: urAbi, functionName: 'resolve',
    args: [toHex(packetToBytes(NAME)), (await import('viem')).encodeFunctionData({ abi: parseAbi(['function data(bytes32,string) view returns (bytes)']), functionName: 'data', args: [namehash(NAME), 'stealth-meta-address[1][60]'] })] })
  ok((await import('viem')).decodeAbiParameters([{ type: 'bytes' }], viaViem[0])[0] === cs.record, 'viem reads the same record through the UniversalResolver')
  const legacy = await plain.getEnsText({ name: NAME, key: 'stealth-meta-address[1]' })
  ok(legacy === keys.meta_uri, 'legacy text record stealth-meta-address[1] = st:eth:0x… (forwarded to the owner\'s PermissionedResolver)')
  results.clientSide = { stealthAddress: cs.stealth_address, sdk: sdk.stealthAddress }

  step('5. gateway path: plain viem getEnsAddress({ name }) -> StealthResolver -> CCIP-Read -> lowki-server -> verified on chain')
  const lookups: Address[] = []
  for (let i = 0; i < 3; i++) {
    const a = await plain.getEnsAddress({ name: NAME })
    ok(a && a !== zeroAddress, `lookup ${i + 1}: ${a}`)
    lookups.push(a!)
  }
  ok(new Set(lookups.map((a) => a.toLowerCase())).size === 3, 'a different address on every lookup')
  const rustCcip = cli('resolve-addr', { rpc: RPC, universal_resolver: ens.UniversalResolver, name: NAME })
  ok(rustCcip.address && !lookups.includes(rustCcip.address), `lowki-ens's own CCIP-Read client resolves too: ${rustCcip.address}`)
  const unregistered = await plain.getEnsAddress({ name: `nobody${LABEL}.${lowki.parentName}` })
  ok(unregistered === null, 'an unregistered sibling resolves to nothing (the parent lists no stealth chains)')
  results.gatewayLookups = lookups

  step('6. a tampered gateway response is rejected on chain')
  const tamper = (mutate: (data: Hex) => Hex) =>
    createPublicClient({
      chain: sepolia, transport: http(RPC),
      ccipRead: {
        async request({ data, sender, urls }) {
          // Forward to the real gateway, then corrupt its answer in transit.
          const res = await fetch(urls[0].includes('{data}') ? urls[0].replace('{sender}', sender.toLowerCase()).replace('{data}', data) : urls[0], urls[0].includes('{data}') ? undefined : {
            method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ data, sender }),
          })
          const body = (await res.json()) as { data: Hex }
          return isStealthAnswer(body.data) ? mutate(body.data) : body.data
        },
      },
    })
  // The local batch gateway passes each inner request through `request`; only the stealth
  // gateway's answer is abi.encode(address, bytes) with a 96-byte verificationData:
  // [address][offset 0x40][length 96][e 32][S.x 32][S.y 32] = 192 bytes.
  const isStealthAnswer = (d: Hex) => d.length === 2 + 2 * 192
  const honest = await tamper((d) => d).getEnsAddress({ name: NAME })
  ok(honest && honest !== zeroAddress, 'control: the same interception with no change resolves')
  const attacker = privateKeyToAccount(toHex(randomBytes(32))).address
  const cases: [string, (d: Hex) => Hex][] = [
    ['address replaced by the attacker\'s', (d) => (d.slice(0, 26) + attacker.slice(2).toLowerCase() + d.slice(66)) as Hex],
    ['ephemeral key bit flipped', (d) => flipNibble(d, 2 + 2 * 96 + 63)],
    ['shared-secret point bit flipped', (d) => flipNibble(d, 2 + 2 * 192 - 1)],
  ]
  for (const [what, mutate] of cases) {
    let rejected = false
    try {
      const r = await tamper(mutate).getEnsAddress({ name: NAME })
      rejected = r === null
    } catch {
      rejected = true
    }
    ok(rejected, `rejected: ${what}`)
  }

  step('7. pay two resolved addresses, then the recipient (viewing + spending keys only) finds both')
  const senderKey = toHex(randomBytes(32))
  const sender = privateKeyToAccount(senderKey)
  await plain.request({ method: 'anvil_setBalance' as any, params: [sender.address, toHex(parseEther('1'))] as any })
  const senderWallet = createWalletClient({ account: sender, chain: sepolia, transport: http(RPC) })
  const payees = [await plain.getEnsAddress({ name: NAME }), await plain.getEnsAddress({ name: NAME })] as Address[]
  const amounts = [parseEther('0.01'), parseEther('0.02')]
  for (let i = 0; i < 2; i++) {
    const hash = await senderWallet.sendTransaction({ to: payees[i], value: amounts[i] })
    await plain.waitForTransactionReceipt({ hash })
    console.log(`  paid ${amounts[i]} wei to ${payees[i]}`)
  }
  const scan = cli('scan', { rpc: RPC, gateway_base: GATEWAY_BASE, name: NAME, seed, proof_message: 'lowki e2e: I control this stealth address' })
  console.log(`  gateway returned ${scan.records} records for ${NAME}; ${scan.matched} matched the viewing key`)
  for (let i = 0; i < 2; i++) {
    const hit = scan.found.find((f: any) => f.stealth_address.toLowerCase() === payees[i].toLowerCase())
    ok(hit && BigInt(hit.balance) === amounts[i], `found ${payees[i]} with balance ${hit?.balance}`)
    ok(await verifyMessage({ address: payees[i], message: 'lowki e2e: I control this stealth address', signature: hit.proof_signature }),
      'derived its spending key (signature by the stealth key verifies)')
  }
  ok(scan.matched === scan.records, 'every gateway record belongs to the viewing key (no foreign records)')
  results.payments = payees
  results.scan = { records: scan.records, matched: scan.matched, funded: scan.found.filter((f: any) => BigInt(f.balance) > 0n).length }

  console.log('\nALL E2E CHECKS PASSED')
  console.log(JSON.stringify(results, (_, v) => (typeof v === 'bigint' ? v.toString() : v), 2))
}

function flipNibble(d: Hex, pos: number): Hex {
  const c = parseInt(d[pos], 16) ^ 1
  return (d.slice(0, pos) + c.toString(16) + d.slice(pos + 1)) as Hex
}

main().catch((e) => {
  console.error(e)
  process.exit(1)
})
