import { defineConfig } from 'tsup';

export default defineConfig({
  entry: { index: 'src/index.ts', wagmi: 'src/wagmi.ts' },
  format: ['esm'],
  dts: true,
  sourcemap: true,
  clean: true,
  target: 'es2022',
  treeshake: true,
  external: ['react', 'react/jsx-runtime', '@wagmi/core'],
});
