// Minimal browser stand-in for node:assert (maci-crypto requires it). Deliberately tiny so the
// JS payload numbers are not inflated by a full polyfill.
function assert(c, m) { if (!c) throw new Error(m || "Assertion failed"); }
assert.ok = assert;
assert.strict = assert;
assert.equal = (a, b, m) => assert(a == b, m);
assert.strictEqual = (a, b, m) => assert(a === b, m);
module.exports = assert;
