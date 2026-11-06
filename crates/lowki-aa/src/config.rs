//! Protocol constants from Kohaku `packages/privacy-pools/src/{config.ts,paymaster/utils.ts}`.

use alloy_primitives::{Address, address};

pub use crate::userop::ENTRY_POINT_V08;

/// Native-asset sentinel (`E_ADDRESS`).
pub const E_ADDRESS: Address = address!("0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee");

/// EntryPoint v0.8 canonical `Simple7702Account` implementation — the 7702 delegate of
/// the ephemeral withdrawal sender (Kohaku `SIMPLE_7702_IMPLEMENTATION`).
pub const SIMPLE_7702_IMPLEMENTATION: Address =
    address!("0xe6Cae83BdE06E4c305530e199D7217f42808555B");

/// Privacy Pools v1 (0xbow) Entrypoint, mainnet.
pub const PP_ENTRYPOINT_MAINNET: Address = address!("0x6818809EefCe719E480a7526D76bD3e561526b46");
/// Privacy Pools v1 (0xbow) Entrypoint, Sepolia.
pub const PP_ENTRYPOINT_SEPOLIA: Address = address!("0x34A2068192b1297f2a7f85D7D8CdE66F8F0921cB");

/// A chain's paymaster wiring (Kohaku `IPaymasterConfig`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaymasterConfig {
    pub chain_id: u64,
    pub bundler_url: &'static str,
    pub entry_point: Address,
    pub paymaster: Address,
    /// `(pool, adapter)` pairs (`poolsAccountsMap`).
    pub pool_adapters: &'static [(Address, Address)],
}

impl PaymasterConfig {
    /// The PrivacyPoolsFeeAdapter for `pool`, if configured.
    pub fn adapter_for_pool(&self, pool: Address) -> Option<Address> {
        self.pool_adapters
            .iter()
            .find(|(p, _)| *p == pool)
            .map(|(_, a)| *a)
    }
}

/// Mainnet (the only chain with a deployed paymaster at the pinned commit).
pub const PAYMASTER_MAINNET: PaymasterConfig = PaymasterConfig {
    chain_id: 1,
    bundler_url: "https://public.pimlico.io/v2/1/rpc",
    entry_point: ENTRY_POINT_V08,
    paymaster: address!("0xe06CB96C57D2442f8F60F5017354BC08F7e91308"),
    pool_adapters: &[
        // ETH pool -> privacypools_simple_eth
        (
            address!("0xf241d57c6debae225c0f2e6ea1529373c9a9c9fb"),
            address!("0x521F578Ac6E894939364Dc27c7e6978Ecf7b7926"),
        ),
        // USDT pool -> privacypools_complex_usdt_100
        (
            address!("0xe859c0bd25f260baee534fb52e307d3b64d24572"),
            address!("0x3B33c5cb645690E6b7Df599F58CADA8fA189C36F"),
        ),
        // USDC pool -> privacypools_complex_usdc_100
        (
            address!("0xb419c2867ab3cbc78921660cb95150d95a94ce86"),
            address!("0x841E8F12FDbB2c55c9E054B4c6d07c2743E09cFC"),
        ),
    ],
};

/// Sepolia: placeholder in Kohaku (paymaster `0x0`, no adapters) — not usable as-is.
pub const PAYMASTER_SEPOLIA_PLACEHOLDER: PaymasterConfig = PaymasterConfig {
    chain_id: 11155111,
    bundler_url: "https://public.pimlico.io/v2/11155111/rpc",
    entry_point: ENTRY_POINT_V08,
    paymaster: Address::ZERO,
    pool_adapters: &[],
};
