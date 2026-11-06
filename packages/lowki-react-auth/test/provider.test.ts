import { createPublicClient, createWalletClient, custom, hashTypedData, parseEther, recoverTypedDataAddress, verifyMessage, type Hex } from 'viem';
import { privateKeyToAccount } from 'viem/accounts';
import { describe, expect, it, vi } from 'vitest';
import { monadTestnet, sepolia } from '../src/chains';
import { RpcErrorCode } from '../src/errors';
import { LowkiEthereumProvider, type ProviderBackend } from '../src/provider';
import type { WireTxRequest } from '../src/protocol';

const KEY = '0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d';
const acct = privateKeyToAccount(KEY);

/** A stand-in for the wallet frame that signs with a real key, so signatures can be verified. */
function keyBackend(opts: { signedIn?: boolean } = {}) {
  let signedIn = opts.signedIn ?? true;
  const calls = { messages: [] as string[], typed: [] as any[], txs: [] as WireTxRequest[], logins: 0 };
  const b: ProviderBackend = {
    address: () => (signedIn ? acct.address : null),
    signMessage: async (text) => {
      calls.messages.push(text);
      return acct.signMessage({ message: text });
    },
    signTypedData: async (t: any) => {
      calls.typed.push(t);
      return acct.signTypedData(t);
    },
    sendTransaction: async (tx) => {
      calls.txs.push(tx);
      return `0x${'11'.repeat(32)}` as Hex;
    },
    login: async () => {
      calls.logins++;
      signedIn = true;
    },
  };
  return { b, calls };
}

const make = (b: ProviderBackend, fetchImpl?: typeof fetch) => new LowkiEthereumProvider(b, { chains: [sepolia, monadTestnet], fetch: fetchImpl });

describe('accounts and chains', () => {
  it('eth_accounts is empty when signed out, the app account when signed in', async () => {
    expect(await make(keyBackend({ signedIn: false }).b).request({ method: 'eth_accounts' })).toEqual([]);
    expect(await make(keyBackend().b).request({ method: 'eth_accounts' })).toEqual([acct.address]);
  });

  it('eth_requestAccounts signs in first', async () => {
    const { b, calls } = keyBackend({ signedIn: false });
    expect(await make(b).request({ method: 'eth_requestAccounts' })).toEqual([acct.address]);
    expect(calls.logins).toBe(1);
  });

  it('reports and switches chains, emitting chainChanged', async () => {
    const p = make(keyBackend().b);
    expect(await p.request({ method: 'eth_chainId' })).toBe('0xaa36a7');
    expect(await p.request({ method: 'net_version' })).toBe('11155111');
    const seen = vi.fn();
    p.on('chainChanged', seen);
    await p.request({ method: 'wallet_switchEthereumChain', params: [{ chainId: '0x279f' }] });
    expect(await p.request({ method: 'eth_chainId' })).toBe('0x279f');
    expect(seen).toHaveBeenCalledWith('0x279f');
    p.removeListener('chainChanged', seen);
    await p.request({ method: 'wallet_switchEthereumChain', params: [{ chainId: '0xaa36a7' }] });
    expect(seen).toHaveBeenCalledTimes(1);
  });

  it('refuses unknown chains with 4902', async () => {
    const p = make(keyBackend().b);
    await expect(p.request({ method: 'wallet_switchEthereumChain', params: [{ chainId: '0x1' }] })).rejects.toMatchObject({ code: RpcErrorCode.UNRECOGNIZED_CHAIN });
  });
});

describe('signing', () => {
  it('personal_sign decodes hex to UTF-8 text and returns a signature for the account', async () => {
    const { b, calls } = keyBackend();
    const p = make(b);
    const hex = `0x${Buffer.from('Swap 1 ETH for USDC', 'utf8').toString('hex')}`;
    const sig = await p.request({ method: 'personal_sign', params: [hex, acct.address] });
    expect(calls.messages).toEqual(['Swap 1 ETH for USDC']);
    expect(await verifyMessage({ address: acct.address, message: 'Swap 1 ETH for USDC', signature: sig })).toBe(true);
  });

  it('personal_sign accepts swapped params and plain text', async () => {
    const { b, calls } = keyBackend();
    await make(b).request({ method: 'personal_sign', params: [acct.address, 'hello'] });
    expect(calls.messages).toEqual(['hello']);
  });

  it('personal_sign refuses another address (4100) and non-UTF-8 bytes (-32602)', async () => {
    const p = make(keyBackend().b);
    await expect(p.request({ method: 'personal_sign', params: ['hi', '0x0000000000000000000000000000000000000001'] })).rejects.toMatchObject({ code: RpcErrorCode.UNAUTHORIZED });
    await expect(p.request({ method: 'personal_sign', params: ['0xff00fe', acct.address] })).rejects.toMatchObject({ code: RpcErrorCode.INVALID_PARAMS });
  });

  it('signing when signed out is 4100', async () => {
    const p = make(keyBackend({ signedIn: false }).b);
    await expect(p.request({ method: 'personal_sign', params: ['hi', acct.address] })).rejects.toMatchObject({ code: RpcErrorCode.UNAUTHORIZED });
  });

  const typed = {
    domain: { name: 'Privy Swap', version: '1', chainId: 11155111, verifyingContract: '0x000000000022D473030F116dDEE9F6B43aC78BA3' as Hex },
    types: { Order: [{ name: 'sell', type: 'uint256' }, { name: 'buy', type: 'address' }] },
    primaryType: 'Order' as const,
    message: { sell: 1000000000000000000n, buy: '0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238' as Hex },
  };

  it('eth_signTypedData_v4 takes a JSON string and passes the object to the wallet', async () => {
    const { b, calls } = keyBackend();
    const p = make(b);
    const json = JSON.stringify(typed, (_k, v) => (typeof v === 'bigint' ? v.toString() : v));
    const sig = await p.request({ method: 'eth_signTypedData_v4', params: [acct.address, json] });
    expect(calls.typed[0].primaryType).toBe('Order');
    expect(await recoverTypedDataAddress({ ...typed, signature: sig })).toBe(acct.address);
  });

  it('eth_signTypedData_v4 refuses a domain on another chain', async () => {
    const p = make(keyBackend().b);
    await expect(
      p.request({ method: 'eth_signTypedData_v4', params: [acct.address, JSON.stringify({ ...typed, domain: { ...typed.domain, chainId: 1 }, message: { sell: '1', buy: typed.message.buy } })] }),
    ).rejects.toMatchObject({ code: RpcErrorCode.INVALID_PARAMS });
  });

  it('eth_sendTransaction converts to the wallet tx format (value in decimal wei)', async () => {
    const { b, calls } = keyBackend();
    const hash = await make(b).request({
      method: 'eth_sendTransaction',
      params: [{ from: acct.address, to: '0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238', value: '0xde0b6b3a7640000', data: '0xA9059CBB', gas: '0x5208' }],
    });
    expect(hash).toMatch(/^0x(11){32}$/);
    expect(calls.txs).toEqual([{ to: '0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238', value: '1000000000000000000', data: '0xa9059cbb' }]);
  });

  it('eth_sendTransaction refuses deployments, a foreign from, and another chain', async () => {
    const p = make(keyBackend().b);
    await expect(p.request({ method: 'eth_sendTransaction', params: [{ from: acct.address, data: '0x60' }] })).rejects.toMatchObject({ code: RpcErrorCode.INVALID_PARAMS });
    await expect(p.request({ method: 'eth_sendTransaction', params: [{ from: '0x0000000000000000000000000000000000000001', to: acct.address }] })).rejects.toMatchObject({ code: RpcErrorCode.UNAUTHORIZED });
    await expect(p.request({ method: 'eth_sendTransaction', params: [{ to: acct.address, chainId: '0x1' }] })).rejects.toMatchObject({ code: RpcErrorCode.INVALID_PARAMS });
  });

  it('unsupported signing methods are 4200', async () => {
    const p = make(keyBackend().b);
    for (const method of ['eth_sign', 'eth_signTransaction', 'wallet_sendCalls', 'eth_bogus']) {
      await expect(p.request({ method, params: [] })).rejects.toMatchObject({ code: RpcErrorCode.UNSUPPORTED_METHOD });
    }
  });
});

describe('read calls', () => {
  it('forwards allow-listed reads to the current chain RPC, without credentials', async () => {
    const f = vi.fn(async (_url: any, init: any) => new Response(JSON.stringify({ jsonrpc: '2.0', id: JSON.parse(init.body).id, result: '0x10' })));
    const p = make(keyBackend().b, f as any);
    expect(await p.request({ method: 'eth_getBalance', params: [acct.address, 'latest'] })).toBe('0x10');
    const [url, init] = f.mock.calls[0]!;
    expect(url).toBe(sepolia.rpcUrls.default.http[0]);
    expect(init.credentials).toBe('omit');
    expect(JSON.parse(init.body)).toMatchObject({ jsonrpc: '2.0', method: 'eth_getBalance', params: [acct.address, 'latest'] });
    await p.request({ method: 'wallet_switchEthereumChain', params: [{ chainId: monadTestnet.id }] });
    await p.request({ method: 'eth_blockNumber' });
    expect(f.mock.calls[1]![0]).toBe(monadTestnet.rpcUrls.default.http[0]);
  });

  it('uses rpcUrls overrides and passes RPC errors through', async () => {
    const f = vi.fn(async () => new Response(JSON.stringify({ jsonrpc: '2.0', id: 1, error: { code: 3, message: 'execution reverted', data: '0x08c379a0' } })));
    const p = new LowkiEthereumProvider(keyBackend().b, { chains: [sepolia], rpcUrls: { [sepolia.id]: 'https://rpc.private.example' }, fetch: f as any });
    await expect(p.request({ method: 'eth_call', params: [{ to: acct.address }, 'latest'] })).rejects.toMatchObject({ code: 3, message: 'execution reverted', data: '0x08c379a0' });
    expect((f.mock.calls[0] as any[])[0]).toBe('https://rpc.private.example');
  });
});

describe('viem on top of the provider', () => {
  it('walletClient.signMessage / signTypedData / sendTransaction work and verify', async () => {
    const { b, calls } = keyBackend();
    const f = vi.fn(async (_u: any, init: any) => {
      const req = JSON.parse(init.body);
      const result = req.method === 'eth_getTransactionCount' ? '0x0' : req.method === 'eth_estimateGas' ? '0x5208' : '0x1';
      return new Response(JSON.stringify({ jsonrpc: '2.0', id: req.id, result }));
    });
    const provider = make(b, f as any);
    const [address] = (await provider.request({ method: 'eth_requestAccounts' })) as Hex[];
    const wallet = createWalletClient({ account: address!, chain: sepolia as any, transport: custom(provider) });

    const sig = await wallet.signMessage({ account: address!, message: 'gm from viem' });
    expect(await verifyMessage({ address: acct.address, message: 'gm from viem', signature: sig })).toBe(true);
    expect(calls.messages).toEqual(['gm from viem']);

    const tsig = await wallet.signTypedData({
      account: address!,
      domain: { name: 'Privy Swap', version: '1', chainId: sepolia.id },
      types: { Order: [{ name: 'sell', type: 'uint256' }] },
      primaryType: 'Order',
      message: { sell: 5n },
    });
    const digest = hashTypedData({ domain: { name: 'Privy Swap', version: '1', chainId: sepolia.id }, types: { Order: [{ name: 'sell', type: 'uint256' }] }, primaryType: 'Order', message: { sell: 5n } });
    expect(digest).toMatch(/^0x/);
    expect(
      await recoverTypedDataAddress({
        domain: { name: 'Privy Swap', version: '1', chainId: sepolia.id },
        types: { Order: [{ name: 'sell', type: 'uint256' }] },
        primaryType: 'Order',
        message: { sell: 5n },
        signature: tsig,
      }),
    ).toBe(acct.address);

    const hash = await wallet.sendTransaction({ account: address!, to: '0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238', value: parseEther('0.01'), chain: sepolia as any });
    expect(hash).toMatch(/^0x(11){32}$/);
    expect(calls.txs[0]).toMatchObject({ to: '0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238', value: '10000000000000000' });

    const pub = createPublicClient({ chain: sepolia as any, transport: custom(provider) });
    expect(await pub.getBlockNumber()).toBe(1n);
  });
});
