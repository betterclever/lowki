// Verifies a Lowki access token with `jose`, exactly like an app backend verifies a Privy token.
// Inputs come from env so nothing is printed except the verified claims.
//   LOWKI_TOKEN  the access token
//   LOWKI_ISS    expected issuer (the Lowki server URL)
//   LOWKI_AUD    expected audience (your app ID)
//   LOWKI_JWKS   optional JWKS JSON; otherwise fetched from `${LOWKI_ISS}/.well-known/jwks.json`
import * as jose from 'jose';

const { LOWKI_TOKEN: token, LOWKI_ISS: issuer, LOWKI_AUD: audience, LOWKI_JWKS: jwksJson } = process.env;
const keySet = jwksJson
  ? jose.createLocalJWKSet(JSON.parse(jwksJson))
  : jose.createRemoteJWKSet(new URL(`${issuer}/.well-known/jwks.json`));

const { payload, protectedHeader } = await jose.jwtVerify(token, keySet, {
  issuer,
  audience,
  algorithms: ['ES256'],
});
console.log(JSON.stringify({ alg: protectedHeader.alg, kid: protectedHeader.kid, sub: payload.sub, aud: payload.aud, iss: payload.iss }));
