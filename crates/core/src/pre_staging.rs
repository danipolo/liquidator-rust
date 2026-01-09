//! Transaction pre-staging pipeline for critical positions.
//!
//! Pre-builds liquidation transactions for critical positions to minimize
//! latency when a liquidation opportunity is detected.

use alloy::primitives::{Address, Bytes, U256};
use dashmap::DashMap;
use smallvec::SmallVec;
use std::time::{Duration, Instant};

use crate::config::config;
use crate::position::TrackedPosition;
use liquidator_api::SwapRoute;

/// Check if price deviation exceeds threshold using native U256 arithmetic.
///
/// OPTIMIZATION: Avoids String parsing which is ~100x slower.
/// Uses basis points (bps) for precision: 50 bps = 0.5%
#[inline]
pub fn price_deviation_exceeds_bps(old_price: U256, new_price: U256, threshold_bps: u64) -> bool {
    if old_price.is_zero() {
        return true; // Consider zero prices as stale
    }

    // Calculate absolute difference
    let diff = if new_price > old_price {
        new_price - old_price
    } else {
        old_price - new_price
    };

    // deviation_bps = (diff * 10000) / old_price
    // Returns true if deviation > threshold
    let deviation_bps = (diff * U256::from(10000)) / old_price;
    deviation_bps > U256::from(threshold_bps)
}

/// Pre-staged liquidation transaction.
#[derive(Debug, Clone)]
pub struct StagedLiquidation {
    /// Target user to liquidate
    pub user: Address,

    /// Collateral asset to seize
    pub collateral_asset: Address,

    /// Debt asset to repay
    pub debt_asset: Address,

    /// Amount of debt to cover
    pub debt_to_cover: U256,

    /// Expected collateral to receive
    pub expected_collateral: U256,

    /// Pre-computed swap route
    pub swap_route: SwapRoute,

    /// When this was staged
    pub staged_at: Instant,

    /// Price snapshot when staged
    pub price_snapshot: SmallVec<[(Address, U256); 4]>,

    /// Valid until (staged_at + TTL)
    pub valid_until: Instant,

    /// Hash of position state for invalidation
    pub position_hash: u64,

    // ============ OPTIMIZATIONS ============

    /// Pre-encoded calldata for instant submission (no encoding at execution time).
    /// This saves ~5ms per liquidation.
    pub encoded_calldata: Option<Bytes>,

    /// Pre-computed minimum amount out (with slippage applied).
    pub min_amount_out: U256,

    /// Pre-estimated gas limit for this specific liquidation.
    pub estimated_gas: u64,
}

impl StagedLiquidation {
    /// Check if this staged transaction is still valid.
    pub fn is_valid(&self) -> bool {
        Instant::now() < self.valid_until
    }

    /// Check if this staged transaction is stale based on price deviation.
    ///
    /// OPTIMIZATION: Uses native U256 arithmetic instead of String parsing.
    /// This is ~100x faster in the hot path.
    pub fn is_price_stale(&self, current_prices: &[(Address, U256)], threshold_pct: f64) -> bool {
        // Convert threshold to basis points for U256 math (e.g., 0.5% = 50 bps)
        let threshold_bps = (threshold_pct * 100.0) as u64;

        for (asset, staged_price) in &self.price_snapshot {
            if let Some((_, current_price)) = current_prices.iter().find(|(a, _)| a == asset) {
                if price_deviation_exceeds_bps(*staged_price, *current_price, threshold_bps) {
                    return true;
                }
            }
        }
        false
    }

    /// Check if position has changed.
    pub fn is_position_changed(&self, current_hash: u64) -> bool {
        self.position_hash != current_hash
    }

    /// Age of this staged transaction.
    pub fn age(&self) -> Duration {
        self.staged_at.elapsed()
    }

    /// Time remaining until expiry.
    pub fn time_remaining(&self) -> Duration {
        self.valid_until.saturating_duration_since(Instant::now())
    }

    /// Check if this staged transaction has pre-encoded calldata.
    ///
    /// If true, execution can skip calldata encoding (~5ms savings).
    #[inline]
    pub fn has_precomputed_calldata(&self) -> bool {
        self.encoded_calldata.is_some()
    }

    /// Get the pre-encoded calldata if available.
    #[inline]
    pub fn get_calldata(&self) -> Option<&Bytes> {
        self.encoded_calldata.as_ref()
    }

    /// Check if this staged transaction is ready for instant execution.
    ///
    /// Returns true if:
    /// - The staged tx is still valid (not expired)
    /// - Has pre-encoded calldata
    /// - Has estimated gas > 0
    #[inline]
    pub fn is_ready_for_instant_execution(&self) -> bool {
        self.is_valid() && self.has_precomputed_calldata() && self.estimated_gas > 0
    }
}

/// Configuration for pre-staging.
/// Uses values from global BotConfig by default.
#[derive(Debug, Clone)]
pub struct PreStagingConfig {
    /// Health factor threshold to start staging
    pub staging_hf_threshold: f64,
    /// TTL for staged transactions
    pub staged_tx_ttl: Duration,
    /// Price deviation threshold for invalidation (percentage)
    pub price_deviation_threshold: f64,
    /// Minimum debt USD value to stage
    pub min_debt_usd_to_stage: f64,
}

impl Default for PreStagingConfig {
    fn default() -> Self {
        let cfg = &config().pre_staging;
        Self {
            staging_hf_threshold: cfg.staging_hf_threshold,
            staged_tx_ttl: cfg.staged_tx_ttl(),
            price_deviation_threshold: cfg.price_deviation_threshold_pct,
            min_debt_usd_to_stage: cfg.min_debt_usd_to_stage,
        }
    }
}

/// Pre-staging pipeline for managing staged transactions.
pub struct PreStager {
    /// Staged transactions by user
    staged: DashMap<Address, StagedLiquidation>,

    /// Cached swap routes by (tokenIn, tokenOut)
    swap_routes: DashMap<(Address, Address), SwapRoute>,

    /// Configuration
    config: PreStagingConfig,
}

impl PreStager {
    /// Create a new pre-stager with default config.
    pub fn new() -> Self {
        Self {
            staged: DashMap::new(),
            swap_routes: DashMap::new(),
            config: PreStagingConfig::default(),
        }
    }

    /// Create a new pre-stager with custom config.
    pub fn with_config(config: PreStagingConfig) -> Self {
        Self {
            staged: DashMap::new(),
            swap_routes: DashMap::new(),
            config,
        }
    }

    /// Check if a position should be staged based on health factor.
    pub fn should_stage(&self, position: &TrackedPosition) -> bool {
        position.health_factor <= self.config.staging_hf_threshold
            && !position.is_bad_debt()
            && position.total_debt_usd() >= self.config.min_debt_usd_to_stage
    }

    /// Stage a liquidation for a position.
    pub fn stage(
        &self,
        position: &TrackedPosition,
        swap_route: SwapRoute,
        debt_to_cover: U256,
        expected_collateral: U256,
        price_snapshot: SmallVec<[(Address, U256); 4]>,
    ) -> Option<StagedLiquidation> {
        let (collateral_asset, _) = position.largest_collateral()?;
        let (debt_asset, _) = position.largest_debt()?;

        let staged = StagedLiquidation {
            user: position.user,
            collateral_asset: *collateral_asset,
            debt_asset: *debt_asset,
            debt_to_cover,
            expected_collateral,
            swap_route: swap_route.clone(),
            staged_at: Instant::now(),
            price_snapshot,
            valid_until: Instant::now() + self.config.staged_tx_ttl,
            position_hash: position.compute_state_hash(),
            // OPTIMIZATION fields - to be filled by caller if needed
            encoded_calldata: None,
            min_amount_out: U256::ZERO,
            estimated_gas: 0,
        };

        self.staged.insert(position.user, staged.clone());

        // Cache the swap route
        self.swap_routes
            .insert((*collateral_asset, *debt_asset), swap_route);

        Some(staged)
    }

    /// Stage a liquidation with pre-encoded calldata for instant execution.
    ///
    /// OPTIMIZATION: Pre-encodes calldata during staging to eliminate
    /// encoding overhead at execution time (~5ms savings).
    pub fn stage_with_calldata(
        &self,
        position: &TrackedPosition,
        swap_route: SwapRoute,
        debt_to_cover: U256,
        expected_collateral: U256,
        price_snapshot: SmallVec<[(Address, U256); 4]>,
        encoded_calldata: Bytes,
        min_amount_out: U256,
        estimated_gas: u64,
    ) -> Option<StagedLiquidation> {
        let (collateral_asset, _) = position.largest_collateral()?;
        let (debt_asset, _) = position.largest_debt()?;

        let staged = StagedLiquidation {
            user: position.user,
            collateral_asset: *collateral_asset,
            debt_asset: *debt_asset,
            debt_to_cover,
            expected_collateral,
            swap_route: swap_route.clone(),
            staged_at: Instant::now(),
            price_snapshot,
            valid_until: Instant::now() + self.config.staged_tx_ttl,
            position_hash: position.compute_state_hash(),
            // Pre-computed optimization fields
            encoded_calldata: Some(encoded_calldata),
            min_amount_out,
            estimated_gas,
        };

        self.staged.insert(position.user, staged.clone());

        // Cache the swap route
        self.swap_routes
            .insert((*collateral_asset, *debt_asset), swap_route);

        Some(staged)
    }

    /// Update the pre-encoded calldata for a staged transaction.
    pub fn update_calldata(&self, user: &Address, calldata: Bytes, min_amount_out: U256, gas: u64) {
        if let Some(mut staged) = self.staged.get_mut(user) {
            staged.encoded_calldata = Some(calldata);
            staged.min_amount_out = min_amount_out;
            staged.estimated_gas = gas;
        }
    }

    /// Get a valid staged transaction for a user.
    pub fn get_valid_staged(&self, user: &Address) -> Option<StagedLiquidation> {
        self.staged.get(user).and_then(|s| {
            if s.is_valid() {
                Some(s.clone())
            } else {
                None
            }
        })
    }

    /// Get staged transaction regardless of validity.
    pub fn get_staged(&self, user: &Address) -> Option<StagedLiquidation> {
        self.staged.get(user).map(|s| s.clone())
    }

    /// Check if a position has a valid staged transaction.
    pub fn has_valid_staged(&self, user: &Address) -> bool {
        self.staged.get(user).is_some_and(|s| s.is_valid())
    }

    /// Invalidate a staged transaction.
    pub fn invalidate(&self, user: &Address) {
        self.staged.remove(user);
    }

    /// Invalidate all staged transactions for positions affected by an asset.
    pub fn invalidate_by_asset(&self, asset: &Address, affected_users: &[Address]) {
        for user in affected_users {
            if let Some(staged) = self.staged.get(user) {
                if staged.collateral_asset == *asset || staged.debt_asset == *asset {
                    drop(staged);
                    self.staged.remove(user);
                }
            }
        }
    }

    /// Get cached swap route.
    pub fn get_swap_route(&self, token_in: &Address, token_out: &Address) -> Option<SwapRoute> {
        self.swap_routes.get(&(*token_in, *token_out)).map(|r| r.clone())
    }

    /// Cache a swap route.
    pub fn cache_swap_route(&self, token_in: Address, token_out: Address, route: SwapRoute) {
        self.swap_routes.insert((token_in, token_out), route);
    }

    /// Clean up expired staged transactions.
    pub fn cleanup_expired(&self) -> usize {
        let mut removed = 0;
        self.staged.retain(|_, staged| {
            if staged.is_valid() {
                true
            } else {
                removed += 1;
                false
            }
        });
        removed
    }

    /// Validate staged transaction against current state.
    pub fn validate_staged(
        &self,
        user: &Address,
        current_position: &TrackedPosition,
        current_prices: &[(Address, U256)],
    ) -> StagedValidationResult {
        let Some(staged) = self.staged.get(user) else {
            return StagedValidationResult::NotStaged;
        };

        if !staged.is_valid() {
            return StagedValidationResult::Expired;
        }

        if staged.is_position_changed(current_position.compute_state_hash()) {
            return StagedValidationResult::PositionChanged;
        }

        if staged.is_price_stale(current_prices, self.config.price_deviation_threshold) {
            return StagedValidationResult::PriceStale;
        }

        StagedValidationResult::Valid(staged.clone())
    }

    /// Get statistics about staged transactions.
    pub fn stats(&self) -> PreStagingStats {
        let total = self.staged.len();
        let valid = self.staged.iter().filter(|s| s.is_valid()).count();
        let swap_routes_cached = self.swap_routes.len();

        PreStagingStats {
            total_staged: total,
            valid_staged: valid,
            expired_staged: total - valid,
            swap_routes_cached,
        }
    }

    /// Get configuration.
    pub fn config(&self) -> &PreStagingConfig {
        &self.config
    }
}

impl Default for PreStager {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of validating a staged transaction.
#[derive(Debug, Clone)]
pub enum StagedValidationResult {
    /// Valid and ready to use
    Valid(StagedLiquidation),
    /// No staged transaction exists
    NotStaged,
    /// Staged transaction has expired
    Expired,
    /// Position state has changed
    PositionChanged,
    /// Prices have deviated too much
    PriceStale,
}

impl StagedValidationResult {
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid(_))
    }

    pub fn into_staged(self) -> Option<StagedLiquidation> {
        match self {
            Self::Valid(staged) => Some(staged),
            _ => None,
        }
    }
}

/// Statistics about pre-staging.
#[derive(Debug, Clone)]
pub struct PreStagingStats {
    pub total_staged: usize,
    pub valid_staged: usize,
    pub expired_staged: usize,
    pub swap_routes_cached: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_staged_validity() {
        let staged = StagedLiquidation {
            user: Address::ZERO,
            collateral_asset: Address::repeat_byte(1),
            debt_asset: Address::repeat_byte(2),
            debt_to_cover: U256::from(1000u64),
            expected_collateral: U256::from(1100u64),
            swap_route: SwapRoute::default(),
            staged_at: Instant::now(),
            price_snapshot: SmallVec::new(),
            valid_until: Instant::now() + Duration::from_secs(15),
            position_hash: 12345,
            encoded_calldata: None,
            min_amount_out: U256::ZERO,
            estimated_gas: 0,
        };

        assert!(staged.is_valid());
        assert!(!staged.is_position_changed(12345));
        assert!(staged.is_position_changed(99999));
        assert!(!staged.has_precomputed_calldata());
        assert!(!staged.is_ready_for_instant_execution());
    }

    #[test]
    fn test_price_deviation_bps() {
        // 0.5% deviation = 50 bps
        let old = U256::from(1000u64);
        let new_up = U256::from(1006u64);  // 0.6% up
        let new_down = U256::from(994u64); // 0.6% down
        let same = U256::from(1004u64);    // 0.4% up

        assert!(price_deviation_exceeds_bps(old, new_up, 50));
        assert!(price_deviation_exceeds_bps(old, new_down, 50));
        assert!(!price_deviation_exceeds_bps(old, same, 50));
        assert!(!price_deviation_exceeds_bps(old, old, 50));
    }

    #[test]
    fn test_precomputed_calldata() {
        let staged = StagedLiquidation {
            user: Address::ZERO,
            collateral_asset: Address::repeat_byte(1),
            debt_asset: Address::repeat_byte(2),
            debt_to_cover: U256::from(1000u64),
            expected_collateral: U256::from(1100u64),
            swap_route: SwapRoute::default(),
            staged_at: Instant::now(),
            price_snapshot: SmallVec::new(),
            valid_until: Instant::now() + Duration::from_secs(15),
            position_hash: 12345,
            encoded_calldata: Some(Bytes::from(vec![0x01, 0x02, 0x03])),
            min_amount_out: U256::from(900u64),
            estimated_gas: 1_600_000,
        };

        assert!(staged.has_precomputed_calldata());
        assert!(staged.is_ready_for_instant_execution());
        assert_eq!(staged.get_calldata().unwrap().len(), 3);
    }

    #[test]
    fn test_pre_stager() {
        let stager = PreStager::new();

        let mut pos = TrackedPosition::new(Address::ZERO);
        pos.health_factor = 1.03;
        pos.collaterals.push((
            Address::repeat_byte(1),
            crate::position::CollateralData {
                asset: Address::repeat_byte(1),
                amount: U256::from(1000u64),
                price: U256::from(100_000_000u64),
                decimals: 6,
                value_usd: 1000.0,
                liquidation_threshold: 8000,
                enabled: true,
            },
        ));
        pos.debts.push((
            Address::repeat_byte(2),
            crate::position::DebtData {
                asset: Address::repeat_byte(2),
                amount: U256::from(500u64),
                price: U256::from(100_000_000u64),
                decimals: 6,
                value_usd: 500.0,
            },
        ));

        assert!(stager.should_stage(&pos));

        let staged = stager.stage(
            &pos,
            SwapRoute::default(),
            U256::from(250u64),
            U256::from(275u64),
            SmallVec::new(),
        );

        assert!(staged.is_some());
        assert!(stager.has_valid_staged(&Address::ZERO));
    }
}
