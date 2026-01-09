//! Tiered position tracker for efficient position management.
//!
//! Positions are classified into tiers based on health factor and trigger distance,
//! with different update frequencies and capabilities per tier.

use alloy::primitives::Address;
use arrayvec::ArrayVec;
use dashmap::{DashMap, DashSet};
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Instant;

use crate::position::{PositionTier, TrackedPosition};
use crate::pre_staging::StagedLiquidation;
use crate::trigger_index::TriggerIndex;
use liquidator_chain::OraclePrice;

/// Maximum number of critical positions to track (cache-friendly ArrayVec).
const MAX_CRITICAL_POSITIONS: usize = 64;

/// Tiered position tracker with efficient data structures per tier.
pub struct TieredPositionTracker {
    /// Critical tier: ArrayVec for zero-hash, cache-friendly access
    critical: RwLock<ArrayVec<Arc<TrackedPosition>, MAX_CRITICAL_POSITIONS>>,

    /// Hot tier: DashMap for concurrent access
    hot: DashMap<Address, Arc<TrackedPosition>>,

    /// Warm tier: DashMap for concurrent access
    warm: DashMap<Address, Arc<TrackedPosition>>,

    /// Cold tier: DashMap for concurrent access
    cold: DashMap<Address, Arc<TrackedPosition>>,

    /// Reverse index: collateral asset → users holding it
    collateral_holders: DashMap<Address, DashSet<Address>>,

    /// Reverse index: debt asset → users owing it
    debt_holders: DashMap<Address, DashSet<Address>>,

    /// Trigger index for instant liquidation detection
    trigger_index: TriggerIndex,

    /// Pre-staged transactions for critical positions
    staged_txs: DashMap<Address, StagedLiquidation>,

    /// Price cache (8 decimals)
    prices: DashMap<Address, OraclePrice>,
}

impl TieredPositionTracker {
    /// Create a new tiered position tracker.
    pub fn new() -> Self {
        Self {
            critical: RwLock::new(ArrayVec::new()),
            hot: DashMap::new(),
            warm: DashMap::new(),
            cold: DashMap::new(),
            collateral_holders: DashMap::new(),
            debt_holders: DashMap::new(),
            trigger_index: TriggerIndex::new(),
            staged_txs: DashMap::new(),
            prices: DashMap::new(),
        }
    }

    /// Insert or update a position.
    pub fn upsert(&self, position: TrackedPosition) {
        let user = position.user;
        let tier = position.tier;
        let position = Arc::new(position);

        // Remove from current tier if exists
        self.remove(&user);

        // Update reverse indices
        self.update_reverse_indices(&position);

        // Insert into appropriate tier
        match tier {
            PositionTier::Critical => {
                let mut critical = self.critical.write();
                if critical.len() < MAX_CRITICAL_POSITIONS {
                    critical.push(position.clone());
                } else {
                    // Overflow to hot tier
                    self.hot.insert(user, position.clone());
                }
            }
            PositionTier::Hot => {
                self.hot.insert(user, position.clone());
            }
            PositionTier::Warm => {
                self.warm.insert(user, position.clone());
            }
            PositionTier::Cold => {
                self.cold.insert(user, position.clone());
            }
        }

        // Update trigger index
        self.trigger_index.update_position(&position);
    }

    /// Remove a position by user address.
    pub fn remove(&self, user: &Address) {
        // Remove from critical
        {
            let mut critical = self.critical.write();
            critical.retain(|p| &p.user != user);
        }

        // Remove from other tiers
        self.hot.remove(user);
        self.warm.remove(user);
        self.cold.remove(user);

        // Remove from reverse indices
        for mut holders in self.collateral_holders.iter_mut() {
            holders.value_mut().remove(user);
        }
        for mut holders in self.debt_holders.iter_mut() {
            holders.value_mut().remove(user);
        }

        // Remove from trigger index
        self.trigger_index.remove_user(user);

        // Remove staged transaction
        self.staged_txs.remove(user);
    }

    /// Get a position by user address.
    pub fn get(&self, user: &Address) -> Option<Arc<TrackedPosition>> {
        // Check critical first (most likely to be accessed)
        {
            let critical = self.critical.read();
            if let Some(pos) = critical.iter().find(|p| &p.user == user) {
                return Some(pos.clone());
            }
        }

        // Check other tiers
        if let Some(pos) = self.hot.get(user) {
            return Some(pos.clone());
        }
        if let Some(pos) = self.warm.get(user) {
            return Some(pos.clone());
        }
        if let Some(pos) = self.cold.get(user) {
            return Some(pos.clone());
        }

        None
    }

    /// Get current tier of a position.
    pub fn get_tier(&self, user: &Address) -> Option<PositionTier> {
        // Check critical
        {
            let critical = self.critical.read();
            if critical.iter().any(|p| &p.user == user) {
                return Some(PositionTier::Critical);
            }
        }

        if self.hot.contains_key(user) {
            return Some(PositionTier::Hot);
        }
        if self.warm.contains_key(user) {
            return Some(PositionTier::Warm);
        }
        if self.cold.contains_key(user) {
            return Some(PositionTier::Cold);
        }

        None
    }

    /// Re-tier a position based on updated health factor.
    pub fn re_tier(&self, user: &Address, new_hf: f64, new_trigger_distance: f64) {
        let new_tier = PositionTier::classify(new_hf, new_trigger_distance);
        let current_tier = self.get_tier(user);

        if current_tier == Some(new_tier) {
            return;
        }

        // Get and update position
        if let Some(old_pos) = self.get(user) {
            let mut new_pos = (*old_pos).clone();
            new_pos.health_factor = new_hf;
            new_pos.min_trigger_distance_pct = new_trigger_distance;
            new_pos.tier = new_tier;
            new_pos.last_updated = Instant::now();

            self.upsert(new_pos);
        }
    }

    /// Get all critical positions.
    pub fn critical_positions(&self) -> Vec<Arc<TrackedPosition>> {
        self.critical.read().to_vec()
    }

    /// Get all hot positions.
    pub fn hot_positions(&self) -> Vec<Arc<TrackedPosition>> {
        self.hot.iter().map(|e| e.value().clone()).collect()
    }

    /// Get all warm positions.
    pub fn warm_positions(&self) -> Vec<Arc<TrackedPosition>> {
        self.warm.iter().map(|e| e.value().clone()).collect()
    }

    /// Get all cold positions.
    pub fn cold_positions(&self) -> Vec<Arc<TrackedPosition>> {
        self.cold.iter().map(|e| e.value().clone()).collect()
    }

    /// Get all positions across all tiers.
    pub fn all_positions(&self) -> Vec<Arc<TrackedPosition>> {
        let mut all = Vec::new();
        all.extend(self.critical_positions());
        all.extend(self.hot_positions());
        all.extend(self.warm_positions());
        all.extend(self.cold_positions());
        all
    }

    /// Get users holding a specific collateral asset.
    pub fn users_with_collateral(&self, asset: &Address) -> Vec<Address> {
        self.collateral_holders
            .get(asset)
            .map(|set| set.iter().map(|a| *a).collect())
            .unwrap_or_default()
    }

    /// Get users owing a specific debt asset.
    pub fn users_with_debt(&self, asset: &Address) -> Vec<Address> {
        self.debt_holders
            .get(asset)
            .map(|set| set.iter().map(|a| *a).collect())
            .unwrap_or_default()
    }

    /// Get users affected by a price change (either as collateral or debt).
    pub fn users_affected_by_asset(&self, asset: &Address) -> Vec<Address> {
        let users: DashSet<Address> = DashSet::new();

        if let Some(collateral_users) = self.collateral_holders.get(asset) {
            for user in collateral_users.iter() {
                users.insert(*user);
            }
        }

        if let Some(debt_users) = self.debt_holders.get(asset) {
            for user in debt_users.iter() {
                users.insert(*user);
            }
        }

        users.iter().map(|u| *u).collect()
    }

    /// Update price cache.
    pub fn update_price(&self, asset: Address, price: OraclePrice) {
        self.prices.insert(asset, price);
    }

    /// Get cached price.
    pub fn get_price(&self, asset: &Address) -> Option<OraclePrice> {
        self.prices.get(asset).map(|p| p.clone())
    }

    /// Get prices map reference.
    pub fn prices(&self) -> &DashMap<Address, OraclePrice> {
        &self.prices
    }

    /// Get trigger index reference.
    pub fn trigger_index(&self) -> &TriggerIndex {
        &self.trigger_index
    }

    /// Stage a liquidation transaction.
    pub fn stage_tx(&self, user: Address, staged: StagedLiquidation) {
        self.staged_txs.insert(user, staged);
    }

    /// Get staged transaction.
    pub fn get_staged_tx(&self, user: &Address) -> Option<StagedLiquidation> {
        self.staged_txs.get(user).map(|s| s.clone())
    }

    /// Remove staged transaction.
    pub fn remove_staged_tx(&self, user: &Address) {
        self.staged_txs.remove(user);
    }

    /// Invalidate staged transaction.
    pub fn invalidate_staged(&self, user: &Address) {
        self.staged_txs.remove(user);
    }

    /// Get statistics about tracked positions.
    pub fn stats(&self) -> TrackerStats {
        TrackerStats {
            critical_count: self.critical.read().len(),
            hot_count: self.hot.len(),
            warm_count: self.warm.len(),
            cold_count: self.cold.len(),
            staged_count: self.staged_txs.len(),
            trigger_count: self.trigger_index.len(),
            price_count: self.prices.len(),
        }
    }

    /// Rebuild the trigger index from all positions.
    pub fn rebuild_trigger_index(&self) {
        let positions = self.all_positions();
        self.trigger_index.rebuild(&positions);
    }

    // Private helpers

    fn update_reverse_indices(&self, position: &TrackedPosition) {
        // Update collateral holders
        for (asset, _) in &position.collaterals {
            self.collateral_holders
                .entry(*asset)
                .or_default()
                .insert(position.user);
        }

        // Update debt holders
        for (asset, _) in &position.debts {
            self.debt_holders
                .entry(*asset)
                .or_default()
                .insert(position.user);
        }
    }
}

impl Default for TieredPositionTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about tracked positions.
#[derive(Debug, Clone)]
pub struct TrackerStats {
    pub critical_count: usize,
    pub hot_count: usize,
    pub warm_count: usize,
    pub cold_count: usize,
    pub staged_count: usize,
    pub trigger_count: usize,
    pub price_count: usize,
}

impl TrackerStats {
    pub fn total_positions(&self) -> usize {
        self.critical_count + self.hot_count + self.warm_count + self.cold_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_management() {
        let tracker = TieredPositionTracker::new();

        // Create critical position
        let mut pos = TrackedPosition::new(Address::repeat_byte(1));
        pos.health_factor = 1.01;
        pos.tier = PositionTier::Critical;

        tracker.upsert(pos);

        assert_eq!(tracker.get_tier(&Address::repeat_byte(1)), Some(PositionTier::Critical));
        assert_eq!(tracker.stats().critical_count, 1);
    }

    #[test]
    fn test_re_tiering() {
        let tracker = TieredPositionTracker::new();

        // Create hot position
        let mut pos = TrackedPosition::new(Address::repeat_byte(1));
        pos.health_factor = 1.05;
        pos.tier = PositionTier::Hot;

        tracker.upsert(pos);
        assert_eq!(tracker.get_tier(&Address::repeat_byte(1)), Some(PositionTier::Hot));

        // Re-tier to critical
        tracker.re_tier(&Address::repeat_byte(1), 1.01, 0.5);
        assert_eq!(tracker.get_tier(&Address::repeat_byte(1)), Some(PositionTier::Critical));
    }

    #[test]
    fn test_reverse_indices() {
        let tracker = TieredPositionTracker::new();

        let asset = Address::repeat_byte(0xAA);
        let user = Address::repeat_byte(1);

        let mut pos = TrackedPosition::new(user);
        pos.collaterals.push((
            asset,
            crate::position::CollateralData {
                asset,
                amount: alloy::primitives::U256::from(1000u64),
                price: alloy::primitives::U256::from(100_000_000u64),
                decimals: 6,
                value_usd: 1000.0,
                liquidation_threshold: 8000,
                enabled: true,
            },
        ));

        tracker.upsert(pos);

        let holders = tracker.users_with_collateral(&asset);
        assert!(holders.contains(&user));
    }
}
