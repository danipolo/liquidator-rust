//! EIP-1559 gas pricing strategy.
//!
//! This strategy is used for Ethereum mainnet and most L2s that support
//! EIP-1559 transaction types with base fee and priority fee.

use super::{GasParams, GasStrategy};
use alloy::rpc::types::TransactionRequest;
use alloy::network::TransactionBuilder;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};

/// EIP-1559 gas pricing strategy.
///
/// Uses `max_fee_per_gas` and `max_priority_fee_per_gas` for transaction pricing.
/// Calculates max_fee based on base fee with a configurable multiplier.
#[derive(Debug)]
pub struct Eip1559GasStrategy {
    /// Default priority fee (tip) in wei.
    default_priority_fee: u128,
    /// Multiplier for max_fee relative to base_fee (e.g., 1.5 = 50% buffer).
    max_fee_multiplier: f64,
    /// Maximum allowed max_fee_per_gas in wei.
    max_fee_cap: u128,
    /// Cached base fee (atomic for thread-safety).
    cached_base_fee: AtomicU64,
    /// Cached priority fee (atomic for thread-safety).
    cached_priority_fee: AtomicU64,
}

impl Eip1559GasStrategy {
    /// Create a new EIP-1559 gas strategy.
    ///
    /// # Arguments
    /// * `default_priority_fee` - Default priority fee (tip) in wei
    /// * `max_fee_multiplier` - Multiplier for max_fee (e.g., 2.0 means max_fee = 2 * base_fee + priority)
    pub fn new(default_priority_fee: u128, max_fee_multiplier: f64) -> Self {
        Self {
            default_priority_fee,
            max_fee_multiplier,
            max_fee_cap: 500_000_000_000, // 500 gwei default cap
            cached_base_fee: AtomicU64::new(30_000_000_000), // 30 gwei default
            cached_priority_fee: AtomicU64::new(default_priority_fee as u64),
        }
    }

    /// Create with a custom max fee cap.
    pub fn with_max_fee_cap(mut self, cap: u128) -> Self {
        self.max_fee_cap = cap;
        self
    }

    /// Get the cached base fee.
    pub fn cached_base_fee(&self) -> u128 {
        self.cached_base_fee.load(Ordering::Relaxed) as u128
    }

    /// Get the cached priority fee.
    pub fn cached_priority_fee(&self) -> u128 {
        self.cached_priority_fee.load(Ordering::Relaxed) as u128
    }

    /// Update cached values.
    pub fn update_cache(&self, base_fee: u128, priority_fee: u128) {
        self.cached_base_fee.store(base_fee as u64, Ordering::Relaxed);
        self.cached_priority_fee.store(priority_fee as u64, Ordering::Relaxed);
    }

    /// Calculate max_fee_per_gas based on base_fee.
    fn calculate_max_fee(&self, base_fee: u128, priority_fee: u128) -> u128 {
        let max_fee = ((base_fee as f64) * self.max_fee_multiplier) as u128 + priority_fee;
        max_fee.min(self.max_fee_cap)
    }
}

#[async_trait]
impl GasStrategy for Eip1559GasStrategy {
    async fn fetch_params(&self, rpc_url: &str) -> Result<GasParams> {
        use alloy::providers::{Provider, ProviderBuilder};

        let provider = ProviderBuilder::new().on_http(rpc_url.parse()?);

        // Get the latest block to extract base fee
        let block = provider
            .get_block_by_number(alloy::eips::BlockNumberOrTag::Latest)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Failed to get latest block"))?;

        let base_fee = block
            .header
            .base_fee_per_gas
            .map(|b| b as u128)
            .unwrap_or(30_000_000_000); // 30 gwei fallback

        // Try to get suggested priority fee, fall back to default
        let priority_fee = provider
            .get_max_priority_fee_per_gas()
            .await
            .unwrap_or(self.default_priority_fee);

        // Update cache
        self.update_cache(base_fee, priority_fee);

        let max_fee_per_gas = self.calculate_max_fee(base_fee, priority_fee);

        Ok(GasParams::Eip1559 {
            max_fee_per_gas,
            max_priority_fee_per_gas: priority_fee,
            base_fee,
        })
    }

    fn apply_gas(&self, tx: &mut TransactionRequest, params: &GasParams) {
        match params {
            GasParams::Eip1559 {
                max_fee_per_gas,
                max_priority_fee_per_gas,
                ..
            } => {
                tx.set_max_fee_per_gas(*max_fee_per_gas);
                tx.set_max_priority_fee_per_gas(*max_priority_fee_per_gas);
            }
            GasParams::Legacy { gas_price } => {
                // Fallback: treat gas_price as both max_fee and priority_fee
                tx.set_max_fee_per_gas(*gas_price);
                tx.set_max_priority_fee_per_gas(self.default_priority_fee.min(*gas_price));
            }
        }
    }

    fn strategy_name(&self) -> &'static str {
        "EIP-1559"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::Address;

    #[test]
    fn test_eip1559_strategy_creation() {
        let strategy = Eip1559GasStrategy::new(2_000_000_000, 1.5);
        assert_eq!(strategy.default_priority_fee, 2_000_000_000);
        assert_eq!(strategy.max_fee_multiplier, 1.5);
    }

    #[test]
    fn test_eip1559_max_fee_calculation() {
        let strategy = Eip1559GasStrategy::new(2_000_000_000, 2.0);

        // base_fee = 30 gwei, priority = 2 gwei
        // max_fee = 30 * 2.0 + 2 = 62 gwei
        let max_fee = strategy.calculate_max_fee(30_000_000_000, 2_000_000_000);
        assert_eq!(max_fee, 62_000_000_000);
    }

    #[test]
    fn test_eip1559_max_fee_cap() {
        let strategy = Eip1559GasStrategy::new(2_000_000_000, 10.0)
            .with_max_fee_cap(100_000_000_000); // 100 gwei cap

        // base_fee = 30 gwei, priority = 2 gwei
        // max_fee = 30 * 10.0 + 2 = 302 gwei, but capped at 100 gwei
        let max_fee = strategy.calculate_max_fee(30_000_000_000, 2_000_000_000);
        assert_eq!(max_fee, 100_000_000_000);
    }

    #[test]
    fn test_eip1559_apply_gas() {
        let strategy = Eip1559GasStrategy::new(2_000_000_000, 1.5);
        let mut tx = TransactionRequest::default().with_to(Address::ZERO);

        let params = GasParams::Eip1559 {
            max_fee_per_gas: 50_000_000_000,
            max_priority_fee_per_gas: 2_000_000_000,
            base_fee: 30_000_000_000,
        };

        strategy.apply_gas(&mut tx, &params);
        // TransactionRequest uses builder pattern - check internal fields
        // After apply_gas, the tx should have the gas params set
        assert!(tx.max_fee_per_gas.is_some());
        assert!(tx.max_priority_fee_per_gas.is_some());
        assert_eq!(tx.max_fee_per_gas.unwrap(), 50_000_000_000);
        assert_eq!(tx.max_priority_fee_per_gas.unwrap(), 2_000_000_000);
    }

    #[test]
    fn test_eip1559_cache_update() {
        let strategy = Eip1559GasStrategy::new(2_000_000_000, 1.5);

        strategy.update_cache(40_000_000_000, 3_000_000_000);
        assert_eq!(strategy.cached_base_fee(), 40_000_000_000);
        assert_eq!(strategy.cached_priority_fee(), 3_000_000_000);
    }
}
