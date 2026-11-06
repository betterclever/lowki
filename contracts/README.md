# Lowki ENSv2 contracts

`<label>.lowki.eth` names that resolve to a fresh ERC-5564 stealth address on every lookup,
through ENSv2 on Sepolia, ENSIP-30 (draft, [ensdomains/ensips#88](https://github.com/ensdomains/ensips/pull/88))
and Fluidkey's reference `StealthResolver` in **verified** mode: the gateway proposes an address,
and the resolver re-derives it on chain from the name's published meta-address before any wallet
sees it. An unmodified wallet (viem `getEnsAddress`, MetaMask) gets a new address per lookup.

```
wallet ── UniversalResolver (0xeeee…eeee) ── root ── .eth ── lowki (UserRegistry proxy) ── alice
                                                              │                              │
                                                       LowkiRegistrar                 resolver = StealthResolver (Fluidkey)
                                                 (sponsored, owner-signed)              │  verified mode: meta-address on chain
                                                                                        │  records resolver = alice's own PermissionedResolver
                                                                                        └─ OffchainLookup ─▶ lowki-server /v1/ens/gateway
```

## Contracts

| Contract | What it is |
|---|---|
| Lowki registry | Not new code: a `VerifiableFactory` proxy of ENS's canonical `UserRegistryImpl` (the ENSv2 `PermissionedRegistry` template), attached as `lowki.eth`'s subregistry. **Emancipated**: the Lowki admin keeps only `ROLE_REGISTRAR_ADMIN`, `ROLE_RENEW_ADMIN`, `ROLE_REGISTER_RESERVED_ADMIN`, `ROLE_SET_PARENT(+_ADMIN)`, `ROLE_SET_URI(+_ADMIN)` on the root resource, none of which can touch a registered name. `DeployLowki` asserts `isEmancipated()` and `VerifiableFactory.verifyContract(registry) == UserRegistryImpl`. |
| [`LowkiRegistrar`](src/LowkiRegistrar.sol) | Gas-sponsored registrar. Holds `ROLE_REGISTRAR | ROLE_RENEW` only. An operator submits `register(label, owner, resolver, duration, deadline, signature)`; `signature` is the owner's EIP-712 `Register(string label,address owner,address resolver,uint64 duration,uint256 nonce,uint256 deadline)` (domain `Lowki Registrar`/`1`), checked with OpenZeppelin `SignatureChecker` (EOA or ERC-1271). The owner receives `ROLE_SET_RESOLVER(+_ADMIN) | ROLE_SET_SUBREGISTRY(+_ADMIN) | ROLE_CAN_TRANSFER_ADMIN` (the `.eth` set) and an expiry of `now + duration` (28 to 366 days). Label rules: 3–32 bytes of `a-z0-9-`, no leading/trailing `-`, no `--` at positions 3–4; every accepted label is already ENSIP-15-normalized, ASCII only (no confusables in payment names). Operators only, so free registrations cannot be squatted by throwaway keys; the owner's signature means the operator can neither give someone a name without consent nor choose its resolver. |
| [`LowkiOwnerAccount`](src/LowkiOwnerAccount.sol) | EIP-7702 delegate for the name-owner key. The owner signs an EIP-7702 authorisation and an EIP-712 `Execute(Call[] calls,uint256 nonce,uint256 deadline)`; the operator sends both in one type-4 transaction. That makes everything after registration sponsored too (deploying the owner's `PermissionedResolver`, configuring `StealthResolver`, setting the resolver), so **the owner key never holds ETH**. Nonce in an ERC-7201 slot, ERC-1271, ERC-1155 receiver, no owner, no upgrades. |
| Fluidkey `StealthResolver` + `Scheme1Verifier` | Used **unchanged**, compiled from the pinned vendor clone with Fluidkey's compiler settings and deployed through the canonical CREATE2 deployer with Fluidkey's own salts (`keccak256("ENSIP-30 StealthResolver")`, `keccak256("ENSIP-30 Scheme1Verifier")`), so the addresses equal any other deployment of the same bytecode and constructor arguments. Lowki has **no modified copy**; nothing in `src/` derives from them. |

### Per-name setup (what the owner's sponsored batch does)

1. `VerifiableFactory.deployProxy(PermissionedResolverImpl, namehash, initialize([owner: all roles], [setText(name, "stealth-meta-address[1]", "st:eth:0x…")]))`: the owner's **own** records resolver. The text record is the older ENSIP draft's form, kept for compatibility with clients that read text records.
2. `StealthResolver.setConfig(name, ownRecordsResolver, [gateway URL])`
3. `StealthResolver.setChains(name, [60], true)`
4. `StealthResolver.setMetaAddress(name, 0x01 || spendPub || viewPub)`: the ENSIP-30 Section 1 `data()` record `stealth-meta-address[1][60]` (flag bit 0 = client-side derivation permitted). Its presence is what makes resolution **verified**.
5. `LowkiRegistry.setResolver(labelhash, StealthResolver)`, using the owner's `ROLE_SET_RESOLVER`, last, so the name never resolves half-configured.

All calldata and signatures come from `crates/lowki-ens` (Rust). The owner key is derived at
`m/137'/0'/{i}'` (`lowki-ens::identity`), separate from app accounts and stealth keys.

## Dependencies (pinned)

| What | Pin |
|---|---|
| [fluidkey/stealth-ens-resolver](https://github.com/fluidkey/stealth-ens-resolver) | `491cc21e0a4d061bdab32679cf5b06ed85f1c310` (2026-10-23, VERSION 0.1.0), cloned read-only at `../../vendor/stealth-ens-resolver`. README: "not audited, not deployed". |
| ensdomains/namechain (contracts-v2) | `62778d80038e50cb23514eab1b7d66ceb8a41e1c`, tag `v2-mainnet-deployment-1`: the code ENS deployed to Sepolia on 2026-10-01 (Fluidkey's submodule) |
| forge-std | `f3dae6e6ee381f25eb6a246f7da9b85c91a68219` (v1.17.0, Fluidkey's submodule) |
| Foundry | forge/anvil/cast 1.9.0 (`6056c5845d`), installed with `foundryup`. Fluidkey's CI pins 1.8.6; only their gas snapshots depend on that. |
| Compiler | solc 0.8.25, `cancun`, optimizer 1000 runs, `bytecode_hash = none`, no CBOR metadata (ENSv2 production settings, as Fluidkey uses) |

Nothing is copied from the vendor tree: `foundry.toml` remaps into `../../vendor/stealth-ens-resolver`.

## ENS addresses

One file, [`deployments/sepolia-ensv2.json`](deployments/sepolia-ensv2.json), read by every
script (`LOWKI_ENS_CONFIG` overrides the path), the gateway (via env) and the e2e. Source: the
ENS docs [deployments page](https://docs.ens.domains/learn/deployments), section "Sepolia (ENSv2)",
cross-checked with ensjs `packages/ensjs/src/clients/chain.ts` at `495ffc7`. ENS redeploys
change them: re-check before a live run. The batch gateway provider is not in the docs; scripts
read it from `UniversalResolverV2.BATCH_GATEWAY_PROVIDER()`.

## Tests

```sh
cd contracts
forge build
forge test            # 37 tests: registrar, registry setup, owner account, full verified flow
```

- `LowkiRegistrar.t.sol` (30): emancipation, verifiable proxy, roles held, registration and its
  event, owner controls the resolver while operator/admin/registrar cannot, transfer, operator-only,
  wrong signer, every signed field bound, replay, deadline, taken, re-registration after expiry
  drops old roles, reserved labels, durations, zero owner, invalid labels, label rules (cases +
  fuzz against a reference), EIP-712 digest by hand and through forge's `eip712HashTypedData`,
  ERC-1271 path with a 7702-delegated owner, renewal and its cap, admin.
- `LowkiStealthFlow.t.sol` (7): namechain's ENSv2 fixture + Fluidkey's `StealthResolver` and
  `Scheme1Verifier` + ScopeLift-SDK vectors (Fluidkey's): sponsored registration, the sponsored
  7702 setup batch, `UniversalResolverV2` resolution of the `data()` record, the legacy text record
  (forwarded) and both address profiles through CCIP-Read; substituted address, flipped `e`, flipped
  `S`, and another meta-address's valid derivation all revert; owner-account signature, expiry,
  replay, atomicity and cross-account binding; `executeDigest` against forge's `eip712HashTypedData`
  (array-of-struct typed data).

## End to end on a Sepolia fork

```sh
contracts/e2e/run.sh          # SEPOLIA_RPC_URL defaults to https://sepolia.gateway.tenderly.co
                              # ANVIL_PORT=8545, GW_PORT=8797; E2E_KEEP=1 leaves anvil + server up
```

Real ENSv2 contracts on an anvil fork. It registers `lowki.eth` through ETHRegistrar (commit,
+61 s, register, MockUSDC), deploys everything with the scripts below, starts lowki-server with
the gateway, and then `e2e/e2e.ts` checks: sponsored registration; the sponsored owner batch
(owner balance stays 0); the client-side path (lowki-ens reads the record, lowki-stealth derives,
equal to the ScopeLift SDK); plain viem `getEnsAddress({ name })` returning a new verified address
each time; lowki-ens's own CCIP-Read client; tampered responses rejected; two payments found by the
recipient from the gateway records with only the viewing and spending keys, with the spending keys
proven by signatures.

## Live Sepolia deployment (runbook)

Nothing has been sent to Sepolia yet. When a funded key exists:

```sh
cd contracts
export LOWKI_SEPOLIA_KEY=0x…            # read by the scripts from the environment, never printed
export RPC=https://sepolia.gateway.tenderly.co   # or your own Sepolia RPC
export LOWKI_GATEWAY_URL=https://<your lowki-server host>/v1/ens/gateway
#   POST form (no {data} in the URL): clients POST {sender,data}; names stay out of URLs and access logs.

# 0. Re-check deployments/sepolia-ensv2.json against https://docs.ens.domains/learn/deployments.

# 1. Availability. If lowki.eth is taken, set LOWKI_PARENT_SUFFIX=<x> to use lowki-<x>.eth,
#    and export LOWKI_PARENT_LABEL=lowki-<x> for every later step.
forge script script/RegisterParent.s.sol --sig "check()" --rpc-url $RPC

# 2. Commit (keep LOWKI_COMMIT_SECRET for step 3), wait >= 60 s, register.
#    MockUSDC is minted to the deployer by the script (public mint); ~8 USDC for 1 year.
export LOWKI_COMMIT_SECRET=0x$(openssl rand -hex 32)
forge script script/RegisterParent.s.sol --sig "commit()" --rpc-url $RPC --broadcast
sleep 70
forge script script/RegisterParent.s.sol --sig "register()" --rpc-url $RPC --broadcast

# 3. Deploy and attach. Writes deployments/sepolia-lowki.json.
#    LOWKI_OPERATOR=<address> to use a separate operator for sponsored registrations.
forge script script/DeployLowki.s.sol --rpc-url $RPC --broadcast

# 4. Run lowki-server with the gateway (see crates/lowki-server/README.md, "ENS stealth gateway"):
#    LOWKI_ENS_GATEWAY=true LOWKI_ENS_RPC_URL=$RPC LOWKI_ENS_PARENT=lowki.eth \
#    LOWKI_ENS_UNIVERSAL_RESOLVER=$(jq -r .ens.UniversalResolver deployments/sepolia-ensv2.json) \
#    LOWKI_ENS_STEALTH_RESOLVER=$(jq -r .StealthResolver deployments/sepolia-lowki.json)
```

Gas, measured on the fork. anvil 1.9.0 forks with its `Amsterdam` schedule, and live Sepolia
prices contract code the same way (about 1,550 gas per deployed byte): live `eth_estimateGas`
results were within 1 % of the fork's (the `StealthResolver` deployment is 32.9 M live, 32.8 M on
the fork).

| Step | Gas |
|---|---|
| Parent `commit` | 127,458 |
| Parent `register` (+ MockUSDC mint and approve) | 921,011 (3 txs) |
| `DeployLowki` | 57,929,340 (12 txs; `StealthResolver` alone 32.8 M, `LowkiRegistrar` 11.6 M, `LowkiOwnerAccount` 7.2 M, `Scheme1Verifier` 2.8 M) |
| Per user: sponsored registration | ≈ 743,500 |
| Per user: sponsored owner batch (7702 delegation + resolver deploy + StealthResolver config + setResolver) | ≈ 3,315,600 |

Sepolia gas prices are tiny (the public RPC quoted 18 wei on 2026-11-06), so 0.05 Sepolia ETH is
ample. `StealthResolver` and `Scheme1Verifier` are deployed once per chain (CREATE2): if someone
else deploys the same bytecode first, the script reuses it. Pad gas on the owner batch: node
estimates for 7702 batches that deploy contracts came out short on this schedule (the e2e adds
30 %).

All scripts are idempotent: they skip what is already deployed or configured, and simulate
without `--broadcast`.

## Known gaps

- Not audited (neither Lowki's contracts nor Fluidkey's, whose README says "not audited, not
  deployed"). ENSIP-30 is a draft; its record format moved from a `st:eth:` text record to a
  `data()` record with a flag byte, and Lowki writes both.
- The registry admin can still reserve unregistered labels and replace the registrar; it cannot
  touch registered names. Expired names return to the pool (re-registration renews the resource, so
  old roles and old StealthResolver state do not carry over).
- One parent-level `StealthResolver` config is set (gateway URL, no chains); unregistered
  siblings resolve to nothing rather than to wildcard stealth addresses.
- `LowkiOwnerAccount` delegation is permanent until the owner signs another 7702 authorisation.
