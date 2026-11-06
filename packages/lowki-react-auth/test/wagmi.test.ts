import { createConfig, connect, getAccount, signMessage, disconnect } from '@wagmi/core';
import { sepolia } from '@wagmi/core/chains';
import { http } from 'viem';
import { describe, expect, it } from 'vitest';
import { LowkiClient } from '../src/client';
import { lowkiConnector } from '../src/wagmi';
import { ADDRESS, APP, FakeHost, USER_ID, WALLET, fakeChannel } from './fakes';

describe('lowkiConnector', () => {
  it('connects through the active Lowki client and signs via the wallet', async () => {
    const host = new FakeHost();
    let signedIn = true;
    const client = new LowkiClient({
      appId: 'app_1',
      appOrigin: APP,
      host,
      config: { lowki: { walletUrl: WALLET } },
      createIframe: (src) => {
        const ch = fakeChannel('iframe', host, src);
        ch.win.onRequest = (m) => {
          if (m.type === 'sign_in' && signedIn) ch.win.reply({ type: 'authenticated', id: m.id, address: ADDRESS, user_id: USER_ID, access_token: 't' });
          if (m.type === 'sign_message') ch.win.reply({ type: 'message_signed', id: m.id, signature: '0x' + '12'.repeat(65) });
          if (m.type === 'sign_out') {
            signedIn = false;
            ch.win.reply({ type: 'signed_out', id: m.id });
          }
        };
        queueMicrotask(() => ch.win.reply({ type: 'ready', app_id: 'app_1' }));
        return ch;
      },
    });
    client.start();
    const config = createConfig({ chains: [sepolia], connectors: [lowkiConnector()], transports: { [sepolia.id]: http() } });
    const res = await connect(config, { connector: config.connectors[0]! });
    expect(res.accounts).toEqual([ADDRESS]);
    expect(res.chainId).toBe(sepolia.id);
    expect(getAccount(config).status).toBe('connected');
    expect(await signMessage(config, { message: 'gm' })).toBe('0x' + '12'.repeat(65));
    await disconnect(config);
    expect(getAccount(config).status).toBe('disconnected');
    client.destroy();
  });
});
