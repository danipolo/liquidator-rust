//! Oracle price monitoring and caching.

use alloy::primitives::{Address, U256};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info, instrument};

use crate::event_listener::{OracleType, OracleUpdate};
use crate::provider::ProviderManager;

/// Cached oracle price.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OraclePrice {
    /// Price value (8 decimals standard)
    pub price: U256,
    /// Timestamp of the update
    pub updated_at: u64,
    /// Block number of the update
    pub block_number: u64,
    /// Oracle type
    pub oracle_type: OracleType,
}

impl OraclePrice {
    /// Create a new oracle price.
    pub fn new(price: U256, updated_at: u64, block_number: u64, oracle_type: OracleType) -> Self {
        Self {
            price,
            updated_at,
            block_number,
            oracle_type,
        }
    }

    /// Get price as f64.
    pub fn price_f64(&self) -> f64 {
        self.price.to_string().parse::<f64>().unwrap_or(0.0) / 1e8
    }

    /// Check if price is stale based on threshold.
    pub fn is_stale(&self, threshold_secs: u64, current_time: u64) -> bool {
        current_time.saturating_sub(self.updated_at) > threshold_secs
    }
}

/// Oracle price monitor with caching.
pub struct OracleMonitor {
    /// Price cache by asset address
    prices: DashMap<Address, OraclePrice>,
    /// Oracle to asset mapping
    oracle_to_asset: DashMap<Address, Address>,
    /// Asset to oracle mapping
    asset_to_oracle: DashMap<Address, Address>,
    /// Provider for on-chain queries
    provider: Arc<ProviderManager>,
}

impl OracleMonitor {
    /// Create a new oracle monitor.
    pub fn new(provider: Arc<ProviderManager>) -> Self {
        Self {
            prices: DashMap::new(),
            oracle_to_asset: DashMap::new(),
            asset_to_oracle: DashMap::new(),
            provider,
        }
    }

    /// Register an oracle-asset mapping.
    pub fn register_oracle(&self, oracle: Address, asset: Address) {
        self.oracle_to_asset.insert(oracle, asset);
        self.asset_to_oracle.insert(asset, oracle);
    }

    /// Update price from an oracle event.
    #[instrument(skip(self), fields(asset = %update.asset, price = %update.price))]
    pub fn update_price(&self, update: OracleUpdate) {
        let price = OraclePrice {
            price: update.price,
            updated_at: update.timestamp,
            block_number: update.block_number,
            oracle_type: update.oracle_type,
        };

        self.prices.insert(update.asset, price);

        debug!(
            asset = %update.asset,
            price = %update.price,
            block = update.block_number,
            "Updated price cache"
        );
    }

    /// Get cached price for an asset.
    pub fn get_price(&self, asset: &Address) -> Option<OraclePrice> {
        self.prices.get(asset).map(|p| p.clone())
    }

    /// Get all cached prices.
    pub fn all_prices(&self) -> Vec<(Address, OraclePrice)> {
        self.prices.iter().map(|e| (*e.key(), e.value().clone())).collect()
    }

    /// Get price for asset or return default.
    pub fn get_price_or_default(&self, asset: &Address) -> OraclePrice {
        self.prices
            .get(asset)
            .map(|p| p.clone())
            .unwrap_or_else(|| OraclePrice {
                price: U256::ZERO,
                updated_at: 0,
                block_number: 0,
                oracle_type: OracleType::Standard,
            })
    }

    /// Check if we have a price for an asset.
    pub fn has_price(&self, asset: &Address) -> bool {
        self.prices.contains_key(asset)
    }

    /// Get number of cached prices.
    pub fn price_count(&self) -> usize {
        self.prices.len()
    }

    /// Refresh all prices from on-chain.
    #[instrument(skip(self))]
    pub async fn refresh_all_prices(&self) -> anyhow::Result<()> {
        info!(count = self.asset_to_oracle.len(), "Refreshing all oracle prices");

        // In a real implementation, this would batch query all oracles
        // For now, prices are updated via WebSocket events

        Ok(())
    }

    /// Get stale prices (older than threshold).
    pub fn get_stale_prices(&self, threshold_secs: u64, current_time: u64) -> Vec<Address> {
        self.prices
            .iter()
            .filter(|e| e.value().is_stale(threshold_secs, current_time))
            .map(|e| *e.key())
            .collect()
    }

    /// Calculate price change percentage.
    pub fn price_change_pct(&self, asset: &Address, new_price: U256) -> Option<f64> {
        let old_price = self.get_price(asset)?;

        let old_f64 = old_price.price.to_string().parse::<f64>().ok()?;
        let new_f64 = new_price.to_string().parse::<f64>().ok()?;

        if old_f64 == 0.0 {
            return None;
        }

        Some(((new_f64 - old_f64) / old_f64) * 100.0)
    }

    /// Get prices map reference.
    pub fn prices(&self) -> &DashMap<Address, OraclePrice> {
        &self.prices
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oracle_price() {
        let price = OraclePrice::new(
            U256::from(200_000_000_000u64), // $2000.00
            1700000000,
            100,
            OracleType::Standard,
        );

        assert!((price.price_f64() - 2000.0).abs() < 0.01);
    }

    #[test]
    fn test_staleness_check() {
        let price = OraclePrice::new(
            U256::from(100_000_000u64),
            1700000000,
            100,
            OracleType::Standard,
        );

        // 1 hour threshold
        let threshold = 3600;
        let current = 1700000000 + 3601; // Just past threshold

        assert!(price.is_stale(threshold, current));

        let current = 1700000000 + 3599; // Just before threshold
        assert!(!price.is_stale(threshold, current));
    }

}
