/**
 * EIP-1193 provider for the Lowki embedded wallet.
 *
 * Signing methods go to the wallet (the user approves them there); read-only
 * methods are forwarded to the current chain's RPC. Nothing here holds a key.
 */
import { caip2, parseChainId, toHexChainId } from './chains';
import { LowkiError, PrivyErrorCode, ProviderRpcError, RpcErrorCode, toProviderError } from './errors';
import { isAddress, jsonSafe, personalSignMessageToText, sameAddress, toWireTx } from './encoding';
import type { WireTxRequest } from './protocol';
import type { Chain, EIP1193OnEventHandler, EIP1193Provider, Hex } from './types';

/** What the provider needs from the wallet session. */
export interface ProviderBackend {
  address(): string | null;
  signMessage(text: string): Promise<Hex>;
  signTypedData(typedData: unknown): Promise<Hex>;
  sendTransaction(tx: WireTxRequest): Promise<Hex>;
  /** Called for `eth_requestAccounts` when signed out (opens sign-in). */
  login?(): Promise<void>;
}

/** Read-only JSON-RPC methods forwarded to the chain's RPC. Everything else is refused. */
export const READ_METHODS = new Set([
  'eth_blockNumber',
  'eth_call',
  'eth_estimateGas',
  'eth_feeHistory',
  'eth_gasPrice',
  'eth_getBalance',
  'eth_getBlockByHash',
  'eth_getBlockByNumber',
  'eth_getBlockTransactionCountByHash',
  'eth_getBlockTransactionCountByNumber',
  'eth_getCode',
  'eth_getFilterChanges',
  'eth_getFilterLogs',
  'eth_getLogs',
  'eth_getProof',
  'eth_getStorageAt',
  'eth_getTransactionByBlockHashAndIndex',
  'eth_getTransactionByBlockNumberAndIndex',
  'eth_getTransactionByHash',
  'eth_getTransactionCount',
  'eth_getTransactionReceipt',
  'eth_getUncleByBlockHashAndIndex',
  'eth_getUncleCountByBlockHash',
  'eth_maxPriorityFeePerGas',
  'eth_newBlockFilter',
  'eth_newFilter',
  'eth_uninstallFilter',
  'eth_syncing',
  'eth_blobBaseFee',
  'eth_createAccessList',
  'eth_simulateV1',
  'net_listening',
  'web3_clientVersion',
]);

/** Signing methods the Lowki wallet can't do (yet). */
const UNSUPPORTED = new Set([
  'eth_sign',
  'eth_signTransaction',
  'eth_sendRawTransaction',
  'eth_signTypedData',
  'eth_signTypedData_v1',
  'eth_signTypedData_v3',
  'wallet_sendCalls',
  'wallet_getCallsStatus',
  'wallet_showCallsStatus',
  'wallet_watchAsset',
  'wallet_requestPermissions',
  'wallet_revokePermissions',
  'eth_coinbase',
  'eth_subscribe',
  'eth_unsubscribe',
]);

export interface ProviderOptions {
  chains: Chain[];
  rpcUrls?: Record<number, string>;
  fetch?: typeof fetch;
  initialChainId?: number;
}

type Params = readonly unknown[] | Record<string, unknown> | undefined;

export class LowkiEthereumProvider implements EIP1193Provider {
  readonly isLowki = true;
  rpcTimeoutDuration = 120_000;
  private chainIdValue: number;
  private readonly listeners = new Map<string, Set<EIP1193OnEventHandler>>();
  private rpcSeq = 0;
  private readonly b: ProviderBackend;
  private readonly o: ProviderOptions;

  constructor(backend: ProviderBackend, opts: ProviderOptions) {
    if (!opts.chains.length) throw new LowkiError(PrivyErrorCode.INVALID_DATA, 'Lowki needs at least one chain');
    this.b = backend;
    this.o = opts;
    this.chainIdValue = opts.initialChainId && opts.chains.some((c) => c.id === opts.initialChainId) ? opts.initialChainId : opts.chains[0]!.id;
  }

  get chainId(): number {
    return this.chainIdValue;
  }

  get chain(): Chain {
    return this.o.chains.find((c) => c.id === this.chainIdValue)!;
  }

  get chains(): readonly Chain[] {
    return this.o.chains;
  }

  // ------------------------------------------------------------ EIP-1193

  async request(args: { method: string; params?: Params }): Promise<any> { // eslint-disable-line @typescript-eslint/no-explicit-any
    try {
      return await this.handle(args.method, args.params);
    } catch (e) {
      throw toProviderError(e);
    }
  }

  on(event: string, listener: EIP1193OnEventHandler): this {
    let set = this.listeners.get(event);
    if (!set) this.listeners.set(event, (set = new Set()));
    set.add(listener);
    return this;
  }

  removeListener(event: string | symbol, listener: EIP1193OnEventHandler): this {
    this.listeners.get(String(event))?.delete(listener);
    return this;
  }

  emit(event: string, ...args: unknown[]): void {
    for (const l of [...(this.listeners.get(event) ?? [])]) {
      try {
        l(...args);
      } catch (e) {
        console.error('[lowki] provider listener threw', e);
      }
    }
  }

  // ------------------------------------------------------------ helpers for the SDK

  /** Switches the current chain. Throws 4902 for a chain not in the config. */
  switchChain(target: unknown): void {
    const id = parseChainId(target);
    if (id === null || !this.o.chains.some((c) => c.id === id)) {
      throw new LowkiError(PrivyErrorCode.UNSUPPORTED_CHAIN_ID, `Chain ${String(target)} is not configured. Add it to config.supportedChains or config.lowki.chains.`, {
        rpcCode: RpcErrorCode.UNRECOGNIZED_CHAIN,
      });
    }
    if (id === this.chainIdValue) return;
    this.chainIdValue = id;
    this.emit('chainChanged', toHexChainId(id));
  }

  get caip2(): string {
    return caip2(this.chainIdValue);
  }

  // ------------------------------------------------------------ dispatch

  private requireAccount(from?: unknown): string {
    const a = this.b.address();
    if (!a) throw new LowkiError(PrivyErrorCode.MUST_BE_AUTHENTICATED, 'Not signed in to Lowki', { rpcCode: RpcErrorCode.UNAUTHORIZED });
    if (from !== undefined && from !== null && !(typeof from === 'string' && sameAddress(from, a))) {
      throw new LowkiError(PrivyErrorCode.EMBEDDED_WALLET_NOT_FOUND, `Address ${String(from)} is not this app's Lowki account`, { rpcCode: RpcErrorCode.UNAUTHORIZED });
    }
    return a;
  }

  private async handle(method: string, params: Params): Promise<unknown> {
    const p = (Array.isArray(params) ? params : params === undefined ? [] : [params]) as unknown[];
    switch (method) {
      case 'eth_accounts': {
        const a = this.b.address();
        return a ? [a] : [];
      }
      case 'eth_requestAccounts': {
        if (!this.b.address() && this.b.login) await this.b.login();
        return [this.requireAccount()];
      }
      case 'eth_chainId':
        return toHexChainId(this.chainIdValue);
      case 'net_version':
        return String(this.chainIdValue);

      case 'personal_sign': {
        // [message, address]; some libraries send [address, message].
        let [message, address] = p;
        if (isAddress(message) && !isAddress(address) && this.b.address() && sameAddress(message, this.b.address()!)) {
          [message, address] = [address, message];
        }
        if (typeof message !== 'string') throw new ProviderRpcError(RpcErrorCode.INVALID_PARAMS, 'personal_sign: message must be a string');
        this.requireAccount(address);
        return this.b.signMessage(personalSignMessageToText(message));
      }

      case 'eth_signTypedData_v4': {
        const [address, data] = p;
        this.requireAccount(address);
        let typed: unknown = data;
        if (typeof data === 'string') {
          try {
            typed = JSON.parse(data);
          } catch {
            throw new ProviderRpcError(RpcErrorCode.INVALID_PARAMS, 'eth_signTypedData_v4: typed data is not JSON');
          }
        }
        const t = typed as { types?: unknown; primaryType?: unknown; domain?: unknown; message?: unknown } | null;
        if (!t || typeof t !== 'object' || typeof t.types !== 'object' || typeof t.primaryType !== 'string' || typeof t.message !== 'object') {
          throw new ProviderRpcError(RpcErrorCode.INVALID_PARAMS, 'eth_signTypedData_v4: expected { domain, types, primaryType, message }');
        }
        const domainChain = (t.domain as { chainId?: unknown } | undefined)?.chainId;
        if (domainChain !== undefined && parseChainId(domainChain) !== this.chainIdValue) {
          throw new ProviderRpcError(RpcErrorCode.INVALID_PARAMS, `eth_signTypedData_v4: domain.chainId ${String(domainChain)} is not the current chain ${this.chainIdValue}`);
        }
        return this.b.signTypedData(jsonSafe(typed));
      }

      case 'eth_sendTransaction': {
        const tx = p[0] as (Record<string, unknown> & { from?: string; chainId?: unknown }) | undefined;
        if (!tx || typeof tx !== 'object') throw new ProviderRpcError(RpcErrorCode.INVALID_PARAMS, 'eth_sendTransaction: missing transaction');
        this.requireAccount(tx.from);
        if (tx.chainId !== undefined && parseChainId(tx.chainId) !== this.chainIdValue) {
          throw new ProviderRpcError(RpcErrorCode.INVALID_PARAMS, `eth_sendTransaction: chainId ${String(tx.chainId)} is not the current chain ${this.chainIdValue}`);
        }
        return this.b.sendTransaction(toWireTx(tx as never));
      }

      case 'wallet_switchEthereumChain': {
        const arg = p[0] as { chainId?: unknown } | undefined;
        this.switchChain(arg?.chainId);
        return null;
      }
      case 'wallet_addEthereumChain': {
        const arg = p[0] as { chainId?: unknown } | undefined;
        const id = parseChainId(arg?.chainId);
        if (id !== null && this.o.chains.some((c) => c.id === id)) return null;
        throw new ProviderRpcError(RpcErrorCode.UNSUPPORTED_METHOD, 'wallet_addEthereumChain: Lowki only uses chains from the app config');
      }
      case 'wallet_getPermissions':
        return this.b.address() ? [{ parentCapability: 'eth_accounts' }] : [];
    }

    if (UNSUPPORTED.has(method)) {
      throw new ProviderRpcError(RpcErrorCode.UNSUPPORTED_METHOD, `${method} is not supported by the Lowki wallet`);
    }
    if (READ_METHODS.has(method)) return this.rpc(method, p);
    throw new ProviderRpcError(RpcErrorCode.UNSUPPORTED_METHOD, `Unsupported method: ${method}`);
  }

  private rpcUrl(): string {
    const url = this.o.rpcUrls?.[this.chainIdValue] ?? this.chain.rpcUrls.default.http[0];
    if (!url) throw new ProviderRpcError(RpcErrorCode.CHAIN_DISCONNECTED, `No RPC URL for chain ${this.chainIdValue}`);
    return url;
  }

  private async rpc(method: string, params: unknown[]): Promise<unknown> {
    const f = this.o.fetch ?? globalThis.fetch.bind(globalThis);
    const ctl = typeof AbortController === 'undefined' ? null : new AbortController();
    const timer = ctl ? setTimeout(() => ctl.abort(), this.rpcTimeoutDuration) : null;
    let res: Response;
    try {
      res = await f(this.rpcUrl(), {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ jsonrpc: '2.0', id: ++this.rpcSeq, method, params }),
        signal: ctl?.signal,
        credentials: 'omit',
      });
    } catch (e) {
      throw new ProviderRpcError(RpcErrorCode.CHAIN_DISCONNECTED, `RPC request failed: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      if (timer) clearTimeout(timer);
    }
    let body: { result?: unknown; error?: { code?: number; message?: string; data?: unknown } };
    try {
      body = (await res.json()) as typeof body;
    } catch {
      throw new ProviderRpcError(RpcErrorCode.INTERNAL, `RPC returned HTTP ${res.status} without JSON`);
    }
    if (body.error) throw new ProviderRpcError(body.error.code ?? RpcErrorCode.INTERNAL, body.error.message ?? 'RPC error', body.error.data);
    if (!res.ok) throw new ProviderRpcError(RpcErrorCode.INTERNAL, `RPC returned HTTP ${res.status}`);
    return body.result ?? null;
  }
}
