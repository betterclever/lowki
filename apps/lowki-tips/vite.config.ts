import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { defineConfig, type Plugin } from 'vite';
import react from '@vitejs/plugin-react';

// The one config: config/<target>.json, target from VITE_TIPS_TARGET (default "fork").
const target = process.env.VITE_TIPS_TARGET ?? 'fork';
const cfgPath = resolve(import.meta.dirname, `config/${target}.json`);
const cfg = existsSync(cfgPath) ? JSON.parse(readFileSync(cfgPath, 'utf8')) : null;
const origin = (u: unknown) => {
  try { return typeof u === 'string' ? new URL(u).origin : null; } catch { return null; }
};

// CROPS: no third-party anything. The CSP allows only this origin, the configured RPC and
// gateway, and the Lowki wallet frame. Built pages only (the dev server needs its HMR socket).
function csp(): Plugin {
  return {
    name: 'tips-csp',
    apply: 'build',
    transformIndexHtml(html) {
      const connect = ["'self'", origin(cfg?.chain?.rpcUrl), origin(cfg?.ens?.gatewayUrl), origin(cfg?.ens?.recordsUrl)];
      const frame = [origin(cfg?.lowki?.walletUrl)];
      const policy = [
        "default-src 'self'",
        "script-src 'self'",
        "style-src 'self' 'unsafe-inline'",
        "font-src 'self'",
        "img-src 'self' data:",
        `connect-src ${[...new Set(connect.filter(Boolean))].join(' ')}`,
        `frame-src ${frame.filter(Boolean).join(' ') || "'none'"}`,
        "object-src 'none'",
        "base-uri 'none'",
        "form-action 'none'",
      ].join('; ');
      return html.replace('<!--CSP-->', `<meta http-equiv="Content-Security-Policy" content="${policy}" />`);
    },
  };
}

const port = Number(process.env.TIPS_PORT ?? 8823);

export default defineConfig({
  plugins: [react(), csp()],
  resolve: {
    // @lowki/react-auth is a symlinked local package: use one React.
    dedupe: ['react', 'react-dom'],
  },
  // Lowki's design tokens, fonts and logo are read from the repo (design/, apps/wallet-ui/assets).
  server: { port, strictPort: true, host: 'localhost', fs: { allow: [resolve(import.meta.dirname, '../..')] } },
  preview: { port, strictPort: true, host: 'localhost' },
  build: {
    rollupOptions: {
      input: {
        app: resolve(import.meta.dirname, 'index.html'),
        creator: resolve(import.meta.dirname, 'creator.html'),
      },
    },
  },
});
