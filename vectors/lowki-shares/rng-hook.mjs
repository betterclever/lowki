// Redirects the package's self-import `shamir-secret-sharing/csprng` to ./replay-csprng.mjs,
// so gen.ts controls every random byte split() consumes. The library itself is unmodified.
import { registerHooks } from 'node:module';
const replay = new URL('./replay-csprng.mjs', import.meta.url).href;
registerHooks({
  resolve(specifier, context, next) {
    if (specifier === 'shamir-secret-sharing/csprng') return { url: replay, shortCircuit: true };
    return next(specifier, context);
  },
});
