//! Oracle provider for managing multiple oracles.

use super::{Oracle, OraclePrice, OracleType, PriceData, PriceSource};
use alloy::primitives::{Address, U256};
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};

/// Price cache entry.
#[derive(Debug, Clone)]
pub struct PriceCache {
    /// Cached price data
    pub price: PriceData,
    /// When the cache was last updated
    pub cached_at: u64,
    /// Source oracle address
    pub oracle: Address,
}

impl PriceCache {
    /// Create a new cache entry.
    pub fn new(price: PriceData, oracle: Address) -> Self {
        let cached_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            price,
            cached_at,
            oracle,
        }
    }

    /// Check if cache is stale.
    pub fn is_cache_stale(&self, max_age_secs: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        now.saturating_sub(self.cached_at) > max_age_secs
    }
}

/// Oracle provider manages multiple oracles and provides unified price access.
pub struct OracleProvider {
    /// Oracles by address
    oracles: DashMap<Address, Arc<dyn Oracle>>,
    /// Asset to oracle mapping
    asset_to_oracle: DashMap<Address, Address>,
    /// Price cache by asset
    price_cache: DashMap<Address, PriceCache>,
    /// Cache TTL in seconds
    cache_ttl: u64,
    /// Default heartbeat threshold
    default_heartbeat: Duration,
}

impl std::fmt::Debug for OracleProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OracleProvider")
            .field("oracle_count", &self.oracles.len())
            .field("cache_ttl", &self.cache_ttl)
            .field("default_heartbeat", &self.default_heartbeat)
            .finish()
    }
}

impl Default for OracleProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl OracleProvider {
    /// Create a new oracle provider.
    pub fn new() -> Self {
        Self {
            oracles: DashMap::new(),
            asset_to_oracle: DashMap::new(),
            price_cache: DashMap::new(),
            cache_ttl: 60, // 1 minute default cache
            default_heartbeat: Duration::from_secs(3600), // 1 hour
        }
    }

    /// Set cache TTL.
    pub fn with_cache_ttl(mut self, ttl_secs: u64) -> Self {
        self.cache_ttl = ttl_secs;
        self
    }

    /// Set default heartbeat threshold.
    pub fn with_default_heartbeat(mut self, heartbeat: Duration) -> Self {
        self.default_heartbeat = heartbeat;
        self
    }

    /// Register an oracle.
    pub fn register_oracle(&self, oracle: Arc<dyn Oracle>) {
        let address = oracle.address();
        let asset = oracle.asset();

        debug!(
            oracle = %address,
            asset = %asset,
            oracle_type = ?oracle.oracle_type(),
            "Registering oracle"
        );

        self.asset_to_oracle.insert(asset, address);
        self.oracles.insert(address, oracle);
    }

    /// Get oracle for an asset.
    pub fn get_oracle_for_asset(&self, asset: &Address) -> Option<Arc<dyn Oracle>> {
        let oracle_addr = self.asset_to_oracle.get(asset)?;
        self.oracles.get(&*oracle_addr).map(|o| Arc::clone(&o))
    }

    /// Get oracle by address.
    pub fn get_oracle(&self, address: &Address) -> Option<Arc<dyn Oracle>> {
        self.oracles.get(address).map(|o| Arc::clone(&o))
    }

    /// Get cached price for an asset.
    pub fn get_cached_price(&self, asset: &Address) -> Option<PriceData> {
        let cache = self.price_cache.get(asset)?;

        if cache.is_cache_stale(self.cache_ttl) {
            return None;
        }

        Some(cache.price.clone())
    }

    /// Update price cache.
    pub fn update_cache(&self, asset: Address, price: PriceData, oracle: Address) {
        let cache = PriceCache::new(price, oracle);
        self.price_cache.insert(asset, cache);
    }

    /// Get price for an asset (cached or fresh).
    pub async fn get_price(&self, asset: &Address) -> anyhow::Result<PriceData> {
        // Try cache first
        if let Some(cached) = self.get_cached_price(asset) {
            return Ok(cached);
        }

        // Fetch from oracle
        let oracle = self.get_oracle_for_asset(asset)
            .ok_or_else(|| anyhow::anyhow!("No oracle registered for asset {}", asset))?;

        let price = oracle.get_price().await?;

        // Update cache
        self.update_cache(*asset, price.clone(), oracle.address());

        Ok(price)
    }

    /// Get prices for multiple assets.
    pub async fn get_prices(&self, assets: &[Address]) -> Vec<(Address, Option<PriceData>)> {
        let mut results = Vec::with_capacity(assets.len());

        for asset in assets {
            let price = self.get_price(asset).await.ok();
            results.push((*asset, price));
        }

        results
    }

    /// Get all cached prices.
    pub fn all_cached_prices(&self) -> Vec<(Address, PriceData)> {
        self.price_cache
            .iter()
            .filter(|e| !e.value().is_cache_stale(self.cache_ttl))
            .map(|e| (*e.key(), e.value().price.clone()))
            .collect()
    }

    /// Check if price is stale for an asset.
    pub async fn is_stale(&self, asset: &Address, threshold: Option<Duration>) -> anyhow::Result<bool> {
        let oracle = self.get_oracle_for_asset(asset)
            .ok_or_else(|| anyhow::anyhow!("No oracle for asset {}", asset))?;

        let threshold = threshold
            .or(oracle.heartbeat())
            .unwrap_or(self.default_heartbeat);

        oracle.is_stale(threshold.as_secs()).await
    }

    /// Get stale assets.
    pub fn get_stale_assets(&self, threshold_secs: u64) -> Vec<Address> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        self.price_cache
            .iter()
            .filter(|e| e.value().price.is_stale(threshold_secs, now))
            .map(|e| *e.key())
            .collect()
    }

    /// Handle price update event (from WebSocket).
    pub fn handle_price_update(&self, oracle: Address, asset: Address, price: U256, timestamp: u64, block_number: u64) {
        let oracle_type = self.oracles
            .get(&oracle)
            .map(|o| o.oracle_type())
            .unwrap_or(OracleType::Chainlink);

        let decimals = self.oracles
            .get(&oracle)
            .map(|o| o.decimals())
            .unwrap_or(8);

        let price_data = PriceData::new(price, decimals, timestamp, block_number, oracle_type);
        self.update_cache(asset, price_data, oracle);

        debug!(
            oracle = %oracle,
            asset = %asset,
            price = %price,
            "Updated price from event"
        );
    }

    /// Get price change percentage.
    pub fn price_change_pct(&self, asset: &Address, new_price: U256) -> Option<f64> {
        let cached = self.price_cache.get(asset)?;
        let old_price = cached.price.price;

        let old_f64 = old_price.to_string().parse::<f64>().ok()?;
        let new_f64 = new_price.to_string().parse::<f64>().ok()?;

        if old_f64 == 0.0 {
            return None;
        }

        Some(((new_f64 - old_f64) / old_f64) * 100.0)
    }

    /// Get number of registered oracles.
    pub fn oracle_count(&self) -> usize {
        self.oracles.len()
    }

    /// Get all registered oracle addresses.
    pub fn oracle_addresses(&self) -> Vec<Address> {
        self.oracles.iter().map(|e| *e.key()).collect()
    }

    /// Get all registered asset addresses.
    pub fn asset_addresses(&self) -> Vec<Address> {
        self.asset_to_oracle.iter().map(|e| *e.key()).collect()
    }

    /// Clear the price cache.
    pub fn clear_cache(&self) {
        self.price_cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_price_cache_staleness() {
        let price = PriceData::new(
            U256::from(100_000_000u64),
            8,
            1700000000,
            100,
            OracleType::Chainlink,
        );

        let cache = PriceCache::new(price, Address::ZERO);

        // Just created, should not be stale
        assert!(!cache.is_cache_stale(60));

        // With very short TTL, should be stale
        // (this depends on timing, so we just check logic)
    }

    #[test]
    fn test_oracle_provider_creation() {
        let provider = OracleProvider::new()
            .with_cache_ttl(120)
            .with_default_heartbeat(Duration::from_secs(7200));

        assert_eq!(provider.oracle_count(), 0);
    }
}
