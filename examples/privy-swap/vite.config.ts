import { resolve } from 'node:path';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// The runtime build swaps the import, exactly like an app that changed one line.
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: { '@privy-io/react-auth': '@lowki/react-auth' },
    // @lowki/react-auth is a symlinked local package: use one React.
    dedupe: ['react', 'react-dom'],
  },
  build: {
    rollupOptions: {
      input: {
        app: resolve(import.meta.dirname, 'index.html'),
        wallet: resolve(import.meta.dirname, 'e2e/reference-wallet/index.html'),
      },
    },
  },
});
