//! Legacy gas pricing strategy (pre-EIP-1559).
//!
//! This strategy is used for chains that don't support EIP-1559,
//! such as HyperLiquid and some other EVM chains.

use super::{GasParams, GasStrategy};
use alloy::rpc::types::TransactionRequest;
use alloy::network::TransactionBuilder;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};

/// Legacy gas pricing strategy.
///
/// Uses a single `gas_price` field for transaction pricing.
/// Caches the gas price to reduce RPC calls.
#[derive(Debug)]
pub struct LegacyGasStrategy {
    /// Default gas price in wei.
    default_gas_price: u128,
    /// Maximum gas price in wei.
    max_gas_price: u128,
    /// Cached gas price (atomic for thread-safety).
    cached_gas_price: AtomicU64,
}

impl LegacyGasStrategy {
    /// Create a new Legacy gas strategy.
    ///
    /// # Arguments
    /// * `default_gas_price` - Default gas price in wei
    /// * `max_gas_price` - Maximum allowed gas price in wei
    pub fn new(default_gas_price: u128, max_gas_price: u128) -> Self {
        Self {
            default_gas_price,
            max_gas_price,
            cached_gas_price: AtomicU64::new(default_gas_price as u64),
        }
    }

    /// Get the cached gas price.
    pub fn cached_gas_price(&self) -> u128 {
        self.cached_gas_price.load(Ordering::Relaxed) as u128
    }

    /// Update the cached gas price.
    pub fn update_cache(&self, gas_price: u128) {
        let capped = gas_price.min(self.max_gas_price);
        self.cached_gas_price.store(capped as u64, Ordering::Relaxed);
    }
}

#[async_trait]
impl GasStrategy for LegacyGasStrategy {
    async fn fetch_params(&self, rpc_url: &str) -> Result<GasParams> {
        use alloy::providers::{Provider, ProviderBuilder};

        let provider = ProviderBuilder::new().on_http(rpc_url.parse()?);
        let gas_price = provider
            .get_gas_price()
            .await
            .unwrap_or(self.default_gas_price);

        // Cap at max and update cache
        let capped_price = gas_price.min(self.max_gas_price);
        self.update_cache(capped_price);

        Ok(GasParams::Legacy {
            gas_price: capped_price,
        })
    }

    fn apply_gas(&self, tx: &mut TransactionRequest, params: &GasParams) {
        match params {
            GasParams::Legacy { gas_price } => {
                tx.set_gas_price(*gas_price);
            }
            GasParams::Eip1559 { max_fee_per_gas, .. } => {
                // Fallback: use max_fee as legacy gas price
                tx.set_gas_price(*max_fee_per_gas);
            }
        }
    }

    fn strategy_name(&self) -> &'static str {
        "Legacy"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::Address;

    #[test]
    fn test_legacy_strategy_creation() {
        let strategy = LegacyGasStrategy::new(1_000_000_000, 10_000_000_000);
        assert_eq!(strategy.default_gas_price, 1_000_000_000);
        assert_eq!(strategy.max_gas_price, 10_000_000_000);
        assert_eq!(strategy.cached_gas_price(), 1_000_000_000);
    }

    #[test]
    fn test_legacy_cache_update() {
        let strategy = LegacyGasStrategy::new(1_000_000_000, 10_000_000_000);

        // Normal update
        strategy.update_cache(5_000_000_000);
        assert_eq!(strategy.cached_gas_price(), 5_000_000_000);

        // Update above max should be capped
        strategy.update_cache(20_000_000_000);
        assert_eq!(strategy.cached_gas_price(), 10_000_000_000);
    }

    #[test]
    fn test_legacy_apply_gas() {
        let strategy = LegacyGasStrategy::new(1_000_000_000, 10_000_000_000);
        let mut tx = TransactionRequest::default().with_to(Address::ZERO);

        let params = GasParams::Legacy {
            gas_price: 5_000_000_000,
        };

        strategy.apply_gas(&mut tx, &params);
        assert_eq!(tx.gas_price(), Some(5_000_000_000));
    }
}
