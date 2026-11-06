//! Lowki side of the benchmarks. The same `cases()` drive the native binary (`src/bin/native.rs`)
//! and the wasm32 module (`src/wasm.rs`, timed by the same JS harness as the TS side).
//! Case ids and `check()` outputs match `js/ts-ops.ts` so results can be joined and compared.
#![allow(clippy::type_complexity)]

#[cfg(feature = "aa")]
pub mod aa;
#[cfg(not(feature = "size-only"))]
pub mod fieldbench;
#[cfg(not(feature = "size-only"))]
pub mod cases;
#[cfg(all(feature = "g16", not(feature = "size-only")))]
pub mod g16;
#[cfg(all(target_arch = "wasm32", not(feature = "size-only")))]
pub mod wasm;
#[cfg(all(target_arch = "wasm32", feature = "size-only"))]
pub mod size;
