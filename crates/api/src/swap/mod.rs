//! Swap routing abstractions for liquidation.
//!
//! This module provides a trait-based abstraction over swap routing providers,
//! enabling support for multiple DEX aggregators and routing solutions.
//!
//! # Supported Routers
//!
//! - `liqd`: Liqd.ag aggregator (HyperLiquid)
//! - `uniswap_v3`: Uniswap V3 (Plasma, Arbitrum, Base, Optimism, Celo)
//!
//! # Example
//!
//! ```rust,ignore
//! use liquidator_api::swap::{SwapRouter, SwapRouterRegistry, UniswapV3Router};
//!
//! let registry = SwapRouterRegistry::new()
//!     .with_router(Arc::new(LiqdRouter::new()))
//!     .with_router(Arc::new(UniswapV3Router::new("https://rpc.plasma.to", 9745)));
//!
//! let router = registry.get_router_for_chain(9745).unwrap();
//! let route = router.get_route(params).await?;
//! ```

mod liqd;
mod uniswap_v3;

pub use liqd::LiqdRouter;
pub use uniswap_v3::{UniswapV3Router, UniswapV3Addresses, FeeTier};

use alloy::primitives::{Address, Bytes, U256};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

/// Parameters for requesting a swap route.
#[derive(Debug, Clone)]
pub struct SwapParams {
    /// Input token address
    pub token_in: Address,
    /// Output token address
    pub token_out: Address,
    /// Input amount (raw, with decimals)
    pub amount_in: U256,
    /// Decimals of input token
    pub decimals_in: u8,
    /// Whether to allow multi-hop routing
    pub multi_hop: bool,
    /// Slippage tolerance in basis points (e.g., 50 = 0.5%)
    pub slippage_bps: u16,
    /// Optional recipient address (defaults to sender)
    pub recipient: Option<Address>,
}

impl SwapParams {
    /// Create new swap parameters with defaults.
    pub fn new(token_in: Address, token_out: Address, amount_in: U256, decimals_in: u8) -> Self {
        Self {
            token_in,
            token_out,
            amount_in,
            decimals_in,
            multi_hop: true,
            slippage_bps: 50, // 0.5% default
            recipient: None,
        }
    }

    /// Set multi-hop preference.
    pub fn with_multi_hop(mut self, multi_hop: bool) -> Self {
        self.multi_hop = multi_hop;
        self
    }

    /// Set slippage tolerance.
    pub fn with_slippage_bps(mut self, slippage_bps: u16) -> Self {
        self.slippage_bps = slippage_bps;
        self
    }

    /// Set recipient address.
    pub fn with_recipient(mut self, recipient: Address) -> Self {
        self.recipient = Some(recipient);
        self
    }
}

/// Computed swap route from a router.
#[derive(Debug, Clone, Default)]
pub struct SwapRoute {
    /// Input token
    pub token_in: Address,
    /// Output token
    pub token_out: Address,
    /// Input amount
    pub amount_in: U256,
    /// Expected output amount
    pub expected_output: U256,
    /// Minimum output amount (after slippage)
    pub min_output: U256,
    /// Swap hops
    pub hops: Vec<SwapHop>,
    /// All tokens in the path
    pub tokens: Vec<Address>,
    /// Price impact percentage (if available)
    pub price_impact: Option<f64>,
    /// Expected input value in USD
    pub expected_input_usd: Option<f64>,
    /// Expected output value in USD
    pub expected_output_usd: Option<f64>,
    /// Router-specific encoded calldata (if available)
    pub encoded_calldata: Option<Bytes>,
}

impl SwapRoute {
    /// Check if this is a direct swap (single hop).
    pub fn is_direct(&self) -> bool {
        self.hops.len() == 1
    }

    /// Get total number of allocations across all hops.
    pub fn total_allocations(&self) -> usize {
        self.hops.iter().map(|h| h.allocations.len()).sum()
    }

    /// Check if the route is profitable based on USD values.
    pub fn is_profitable(&self) -> bool {
        match (self.expected_input_usd, self.expected_output_usd) {
            (Some(input), Some(output)) => output > input,
            _ => true, // Assume profitable if we can't calculate
        }
    }
}

/// A single hop in the swap route.
#[derive(Debug, Clone)]
pub struct SwapHop {
    /// Allocations in this hop (can be split across multiple pools)
    pub allocations: Vec<SwapAllocation>,
}

/// Single allocation within a hop.
#[derive(Debug, Clone)]
pub struct SwapAllocation {
    /// Input token for this allocation
    pub token_in: Address,
    /// Output token for this allocation
    pub token_out: Address,
    /// Router/DEX index
    pub router_index: u8,
    /// Fee tier (in hundredths of basis points, e.g., 3000 = 0.3%)
    pub fee: u32,
    /// Amount in (scaled to token decimals)
    pub amount_in: U256,
    /// Whether this is a stable pool
    pub stable: bool,
}

/// Trait for swap routing providers.
///
/// Implement this trait to add support for a new DEX aggregator or routing solution.
#[async_trait]
pub trait SwapRouter: Send + Sync + Debug {
    /// Get the router identifier (e.g., "liqd", "1inch").
    fn router_id(&self) -> &str;

    /// Get supported chain IDs.
    fn supported_chains(&self) -> &[u64];

    /// Check if this router supports a specific chain.
    fn supports_chain(&self, chain_id: u64) -> bool {
        self.supported_chains().contains(&chain_id)
    }

    /// Get a swap route for the given parameters.
    async fn get_route(&self, params: SwapParams) -> Result<SwapRoute>;

    /// Get a swap route with caching (if supported).
    /// Implementations should provide caching logic if beneficial.
    async fn get_route_cached(&self, params: SwapParams) -> Result<SwapRoute>;

    /// Encode a swap route into calldata for execution.
    /// Some routers provide pre-encoded calldata, others need encoding.
    fn encode_route(&self, route: &SwapRoute) -> Result<Bytes>;

    /// Create a fallback direct route (for when the API is unavailable).
    fn create_direct_route(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> SwapRoute {
        let allocation = SwapAllocation {
            token_in,
            token_out,
            router_index: 0,
            fee: 3000, // 0.3% default
            amount_in,
            stable: false,
        };

        SwapRoute {
            token_in,
            token_out,
            amount_in,
            expected_output: amount_in, // Assume 1:1 for fallback
            min_output: amount_in * U256::from(995) / U256::from(1000), // 0.5% slippage
            hops: vec![SwapHop {
                allocations: vec![allocation],
            }],
            tokens: vec![token_in, token_out],
            price_impact: None,
            expected_input_usd: None,
            expected_output_usd: None,
            encoded_calldata: None,
        }
    }
}

/// Registry for managing multiple swap routers.
///
/// Allows selecting the appropriate router based on chain ID and
/// provides fallback routing when preferred routers fail.
#[derive(Debug, Default)]
pub struct SwapRouterRegistry {
    /// Routers indexed by chain ID
    routers: HashMap<u64, Vec<Arc<dyn SwapRouter>>>,
    /// Default router for unknown chains
    default_router: Option<Arc<dyn SwapRouter>>,
}

impl SwapRouterRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a router to the registry.
    pub fn with_router(mut self, router: Arc<dyn SwapRouter>) -> Self {
        for chain_id in router.supported_chains() {
            self.routers
                .entry(*chain_id)
                .or_default()
                .push(Arc::clone(&router));
        }
        self
    }

    /// Set the default router for unknown chains.
    pub fn with_default(mut self, router: Arc<dyn SwapRouter>) -> Self {
        self.default_router = Some(router);
        self
    }

    /// Get routers for a specific chain.
    pub fn get_routers_for_chain(&self, chain_id: u64) -> Vec<Arc<dyn SwapRouter>> {
        self.routers
            .get(&chain_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Get the primary router for a chain.
    pub fn get_router_for_chain(&self, chain_id: u64) -> Option<Arc<dyn SwapRouter>> {
        self.routers
            .get(&chain_id)
            .and_then(|r| r.first().cloned())
            .or_else(|| self.default_router.clone())
    }

    /// Get a route, trying multiple routers if needed.
    pub async fn get_route_with_fallback(
        &self,
        chain_id: u64,
        params: SwapParams,
    ) -> Result<SwapRoute> {
        let routers = self.get_routers_for_chain(chain_id);

        if routers.is_empty() {
            if let Some(default) = &self.default_router {
                return default.get_route(params).await;
            }
            anyhow::bail!("No router available for chain {}", chain_id);
        }

        let mut last_error = None;
        for router in routers {
            match router.get_route_cached(params.clone()).await {
                Ok(route) => return Ok(route),
                Err(e) => {
                    tracing::warn!(
                        router = router.router_id(),
                        chain_id = chain_id,
                        error = %e,
                        "Router failed, trying next"
                    );
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("No routers available")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swap_params_builder() {
        let params = SwapParams::new(
            Address::ZERO,
            Address::repeat_byte(1),
            U256::from(1000),
            18,
        )
        .with_multi_hop(false)
        .with_slippage_bps(100);

        assert!(!params.multi_hop);
        assert_eq!(params.slippage_bps, 100);
    }

    #[test]
    fn test_swap_route_helpers() {
        let route = SwapRoute {
            token_in: Address::ZERO,
            token_out: Address::repeat_byte(1),
            amount_in: U256::from(1000),
            expected_output: U256::from(990),
            min_output: U256::from(985),
            hops: vec![SwapHop {
                allocations: vec![SwapAllocation {
                    token_in: Address::ZERO,
                    token_out: Address::repeat_byte(1),
                    router_index: 0,
                    fee: 3000,
                    amount_in: U256::from(1000),
                    stable: false,
                }],
            }],
            tokens: vec![Address::ZERO, Address::repeat_byte(1)],
            price_impact: Some(0.1),
            expected_input_usd: Some(100.0),
            expected_output_usd: Some(99.0),
            encoded_calldata: None,
        };

        assert!(route.is_direct());
        assert_eq!(route.total_allocations(), 1);
        assert!(!route.is_profitable()); // 99 < 100
    }
}
