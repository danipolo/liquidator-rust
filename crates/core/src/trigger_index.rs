//! Trigger-based position index for instant liquidation detection.
//!
//! Pre-computes trigger prices for each position: "At what price does this
//! position become liquidatable?"

use alloy::primitives::{Address, U256};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::position::TrackedPosition;
use crate::u256_math;

/// Direction of price movement that triggers liquidation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PriceDirection {
    /// Collateral price dropping triggers liquidation
    Down,
    /// Debt price rising triggers liquidation
    Up,
}

/// Entry in the trigger index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerEntry {
    /// User address
    pub user: Address,
    /// Price at which HF crosses 1.0
    pub trigger_price: U256,
    /// Direction of price movement that triggers
    pub direction: PriceDirection,
    /// Current health factor
    pub current_hf: f64,
}

impl TriggerEntry {
    /// Check if a price movement crosses this trigger.
    pub fn is_triggered(&self, old_price: U256, new_price: U256) -> bool {
        match self.direction {
            PriceDirection::Down => new_price <= self.trigger_price && old_price > self.trigger_price,
            PriceDirection::Up => new_price >= self.trigger_price && old_price < self.trigger_price,
        }
    }

    /// Calculate distance to trigger as percentage.
    /// Uses native U256 arithmetic for performance (2-5x faster than String parsing).
    #[inline]
    pub fn distance_pct(&self, current_price: U256) -> f64 {
        if current_price.is_zero() || self.trigger_price.is_zero() {
            return 100.0;
        }

        // Calculate percentage difference in basis points, then convert to %
        let bps = match self.direction {
            PriceDirection::Down => {
                // Distance = (current - trigger) / current * 100
                if current_price > self.trigger_price {
                    u256_math::pct_diff_bps(current_price, self.trigger_price).abs()
                } else {
                    0
                }
            }
            PriceDirection::Up => {
                // Distance = (trigger - current) / current * 100
                if self.trigger_price > current_price {
                    u256_math::pct_diff_bps(current_price, self.trigger_price).abs()
                } else {
                    0
                }
            }
        };

        // Convert basis points to percentage (100 bps = 1%)
        (bps as f64 / 100.0).max(0.0)
    }
}

/// Index of trigger prices by asset for O(n) lookup on price updates.
pub struct TriggerIndex {
    /// Asset address → sorted vec of trigger entries
    triggers_by_asset: DashMap<Address, Vec<TriggerEntry>>,
}

impl TriggerIndex {
    /// Create a new empty trigger index.
    pub fn new() -> Self {
        Self {
            triggers_by_asset: DashMap::new(),
        }
    }

    /// Get positions that become liquidatable when price moves from old to new.
    pub fn get_liquidatable_at(
        &self,
        asset: Address,
        new_price: U256,
        old_price: U256,
    ) -> Vec<Address> {
        let Some(triggers) = self.triggers_by_asset.get(&asset) else {
            return Vec::new();
        };

        triggers
            .iter()
            .filter(|t| t.is_triggered(old_price, new_price))
            .map(|t| t.user)
            .collect()
    }

    /// Get all users affected by a price change (for re-evaluation).
    pub fn get_affected_users(&self, asset: Address) -> Vec<Address> {
        let Some(triggers) = self.triggers_by_asset.get(&asset) else {
            return Vec::new();
        };

        triggers.iter().map(|t| t.user).collect()
    }

    /// Rebuild the entire index from positions.
    pub fn rebuild(&self, positions: &[Arc<TrackedPosition>]) {
        // Clear existing entries
        self.triggers_by_asset.clear();

        // Rebuild from positions
        for position in positions {
            self.add_position_triggers(position);
        }
    }

    /// Update triggers for a single position.
    pub fn update_position(&self, position: &TrackedPosition) {
        // Remove old entries for this user
        self.remove_user(&position.user);

        // Add new triggers
        self.add_position_triggers(position);
    }

    /// Remove all triggers for a user.
    pub fn remove_user(&self, user: &Address) {
        for mut entry in self.triggers_by_asset.iter_mut() {
            entry.value_mut().retain(|t| &t.user != user);
        }
    }

    /// Add trigger entries for a position.
    fn add_position_triggers(&self, position: &TrackedPosition) {
        // Calculate total debt in USD
        let total_debt: f64 = position.debts.iter().map(|(_, d)| d.value_usd).sum();

        if total_debt <= 0.0 {
            return;
        }

        // Calculate trigger for each collateral (price drop)
        for (asset, collateral) in &position.collaterals {
            if !collateral.enabled || collateral.value_usd <= 0.0 {
                continue;
            }

            // Calculate other collateral value
            let other_collateral_adjusted: f64 = position
                .collaterals
                .iter()
                .filter(|(a, c)| a != asset && c.enabled)
                .map(|(_, c)| c.risk_adjusted_value())
                .sum();

            // Trigger price = (total_debt - other_collateral) / (amount × LT)
            // At this price, HF = 1.0
            let required_value = total_debt - other_collateral_adjusted;

            if required_value <= 0.0 {
                // Other collateral covers debt, no trigger
                continue;
            }

            // Convert required value to trigger price
            // value = amount * price / 10^decimals / 10^8 * LT
            // trigger_price = required_value * 10^8 * 10^decimals / (amount * LT)
            let amount_f64 = collateral
                .amount
                .to_string()
                .parse::<f64>()
                .unwrap_or(0.0);
            let lt = collateral.lt_decimal();

            if amount_f64 <= 0.0 || lt <= 0.0 {
                continue;
            }

            let trigger_price_f64 = required_value * 1e8 * 10_f64.powi(collateral.decimals as i32)
                / (amount_f64 * lt);

            if trigger_price_f64 <= 0.0 || trigger_price_f64.is_nan() || trigger_price_f64.is_infinite() {
                continue;
            }

            let trigger_price = U256::from(trigger_price_f64 as u128);

            let entry = TriggerEntry {
                user: position.user,
                trigger_price,
                direction: PriceDirection::Down,
                current_hf: position.health_factor,
            };

            self.triggers_by_asset
                .entry(*asset)
                .or_default()
                .push(entry);
        }

        // Calculate trigger for each debt (price rise)
        for (asset, debt) in &position.debts {
            if debt.value_usd <= 0.0 {
                continue;
            }

            // Calculate total collateral adjusted value
            let total_collateral_adjusted: f64 = position
                .collaterals
                .iter()
                .filter(|(_, c)| c.enabled)
                .map(|(_, c)| c.risk_adjusted_value())
                .sum();

            // Calculate other debt value
            let other_debt: f64 = position
                .debts
                .iter()
                .filter(|(a, _)| a != asset)
                .map(|(_, d)| d.value_usd)
                .sum();

            // At trigger: collateral_adjusted = this_debt + other_debt
            // this_debt = collateral_adjusted - other_debt
            let trigger_debt_value = total_collateral_adjusted - other_debt;

            if trigger_debt_value <= 0.0 {
                // Already underwater or very close
                continue;
            }

            // Convert to trigger price
            // value = amount * price / 10^decimals / 10^8
            // trigger_price = trigger_debt_value * 10^8 * 10^decimals / amount
            // Use fast U256 to f64 conversion via WAD intermediate
            let amount_wad = debt.amount * u256_math::pow10(18 - debt.decimals);
            let amount_f64 = u256_math::wad_to_f64(amount_wad);

            if amount_f64 <= 0.0 {
                continue;
            }

            let trigger_price_f64 =
                trigger_debt_value * 1e8 * 10_f64.powi(debt.decimals as i32) / amount_f64;

            if trigger_price_f64 <= 0.0 || trigger_price_f64.is_nan() || trigger_price_f64.is_infinite() {
                continue;
            }

            let trigger_price = U256::from(trigger_price_f64 as u128);

            let entry = TriggerEntry {
                user: position.user,
                trigger_price,
                direction: PriceDirection::Up,
                current_hf: position.health_factor,
            };

            self.triggers_by_asset
                .entry(*asset)
                .or_default()
                .push(entry);
        }
    }

    /// Get total number of triggers.
    pub fn len(&self) -> usize {
        self.triggers_by_asset
            .iter()
            .map(|e| e.value().len())
            .sum()
    }

    /// Check if index is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get number of assets with triggers.
    pub fn asset_count(&self) -> usize {
        self.triggers_by_asset.len()
    }
}

impl Default for TriggerIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trigger_direction_down() {
        let entry = TriggerEntry {
            user: Address::ZERO,
            trigger_price: U256::from(100_000_000u64), // $1.00
            direction: PriceDirection::Down,
            current_hf: 1.1,
        };

        // Price drops from $1.10 to $0.90 - should trigger
        assert!(entry.is_triggered(
            U256::from(110_000_000u64),
            U256::from(90_000_000u64)
        ));

        // Price stays above trigger - should not trigger
        assert!(!entry.is_triggered(
            U256::from(120_000_000u64),
            U256::from(110_000_000u64)
        ));
    }

    #[test]
    fn test_trigger_direction_up() {
        let entry = TriggerEntry {
            user: Address::ZERO,
            trigger_price: U256::from(110_000_000u64), // $1.10
            direction: PriceDirection::Up,
            current_hf: 1.1,
        };

        // Price rises from $1.00 to $1.20 - should trigger
        assert!(entry.is_triggered(
            U256::from(100_000_000u64),
            U256::from(120_000_000u64)
        ));

        // Price stays below trigger - should not trigger
        assert!(!entry.is_triggered(
            U256::from(100_000_000u64),
            U256::from(105_000_000u64)
        ));
    }

    #[test]
    fn test_distance_calculation() {
        let entry = TriggerEntry {
            user: Address::ZERO,
            trigger_price: U256::from(90_000_000u64), // $0.90
            direction: PriceDirection::Down,
            current_hf: 1.1,
        };

        // Current price $1.00, trigger at $0.90 = 10% distance
        let distance = entry.distance_pct(U256::from(100_000_000u64));
        assert!((distance - 10.0).abs() < 0.1);
    }
}
