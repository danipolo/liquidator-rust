//! Health factor sensitivity estimation for fast HF approximation.
//!
//! Uses linear approximation to estimate HF changes from price movements
//! in ~10ns instead of full recalculation (~1μs).

use alloy::primitives::{Address, U256};
use dashmap::DashMap;
use smallvec::SmallVec;
use std::time::Instant;

use crate::position::TrackedPosition;
use crate::u256_math;
use liquidator_chain::OraclePrice;

/// Pre-computed sensitivity coefficients for fast HF estimation.
#[derive(Debug, Clone)]
pub struct PositionSensitivity {
    /// User address
    pub user: Address,
    /// Base health factor at computation time
    pub base_hf: f64,
    /// dHF/d(%price) for each asset
    pub sensitivities: SmallVec<[(Address, f64); 8]>,
    /// Price snapshot when computed (for drift detection)
    pub price_snapshot: SmallVec<[(Address, U256); 8]>,
    /// When sensitivities were computed
    pub computed_at: Instant,
}

impl PositionSensitivity {
    /// Compute sensitivities from a position and current prices.
    pub fn compute(
        position: &TrackedPosition,
        prices: &DashMap<Address, OraclePrice>,
    ) -> Self {
        let mut sensitivities = SmallVec::new();
        let mut price_snapshot = SmallVec::new();

        // Get totals for sensitivity calculation
        let total_debt: f64 = position.debts.iter().map(|(_, d)| d.value_usd).sum();

        if total_debt <= 0.0 {
            return Self {
                user: position.user,
                base_hf: position.health_factor,
                sensitivities,
                price_snapshot,
                computed_at: Instant::now(),
            };
        }

        // Collateral sensitivity: dHF/d(%price) = (value × LT) / total_debt / 100
        // A 1% increase in collateral price increases HF by this amount
        for (asset, collateral) in &position.collaterals {
            if !collateral.enabled {
                continue;
            }

            let sensitivity = (collateral.value_usd * collateral.lt_decimal()) / total_debt / 100.0;
            sensitivities.push((*asset, sensitivity));

            if let Some(price) = prices.get(asset) {
                price_snapshot.push((*asset, price.price));
            }
        }

        // Debt sensitivity: dHF/d(%price) = -HF × debt_value / total_debt / 100
        // A 1% increase in debt price decreases HF by this amount
        for (asset, debt) in &position.debts {
            let sensitivity = -position.health_factor * debt.value_usd / total_debt / 100.0;

            // Check if asset already has a sensitivity (can be both collateral and debt)
            if let Some(existing) = sensitivities.iter_mut().find(|(a, _)| a == asset) {
                existing.1 += sensitivity;
            } else {
                sensitivities.push((*asset, sensitivity));

                if let Some(price) = prices.get(asset) {
                    price_snapshot.push((*asset, price.price));
                }
            }
        }

        Self {
            user: position.user,
            base_hf: position.health_factor,
            sensitivities,
            price_snapshot,
            computed_at: Instant::now(),
        }
    }

    /// Estimate HF at new prices using linear approximation.
    ///
    /// Takes price changes as (asset, percentage_change) tuples.
    /// Returns estimated health factor (~10ns computation).
    pub fn estimate_hf(&self, price_changes: &[(Address, f64)]) -> f64 {
        let mut hf = self.base_hf;

        for (asset, pct_change) in price_changes {
            if let Some((_, sensitivity)) = self.sensitivities.iter().find(|(a, _)| a == asset) {
                hf += sensitivity * pct_change;
            }
        }

        hf
    }

    /// Estimate HF from new absolute prices.
    /// Uses native U256 arithmetic for performance (2-5x faster than String parsing).
    pub fn estimate_hf_from_prices(&self, new_prices: &[(Address, U256)]) -> f64 {
        let price_changes: Vec<(Address, f64)> = new_prices
            .iter()
            .filter_map(|(asset, new_price)| {
                // Find old price in snapshot
                let old_price = self.price_snapshot.iter().find(|(a, _)| a == asset)?.1;

                if old_price.is_zero() {
                    return None;
                }

                // Calculate percentage change using native U256 arithmetic
                // Returns basis points, convert to percentage
                let bps = u256_math::pct_diff_bps(old_price, *new_price);
                let pct_change = bps as f64 / 100.0; // Convert bps to percentage

                Some((*asset, pct_change))
            })
            .collect();

        self.estimate_hf(&price_changes)
    }

    /// Check if sensitivities are stale (prices drifted too much).
    /// Uses native U256 arithmetic for performance (2-5x faster than String parsing).
    pub fn is_stale(&self, prices: &DashMap<Address, OraclePrice>, threshold_pct: f64) -> bool {
        // Convert threshold from percentage to basis points for comparison
        let threshold_bps = (threshold_pct * 100.0) as i64;

        for (asset, old_price) in &self.price_snapshot {
            if old_price.is_zero() {
                continue;
            }

            if let Some(current) = prices.get(asset) {
                // Calculate drift using native U256 arithmetic
                let drift_bps = u256_math::pct_diff_bps(*old_price, current.price).abs();

                if drift_bps > threshold_bps {
                    return true;
                }
            }
        }
        false
    }

    /// Get the most sensitive asset (largest absolute sensitivity).
    pub fn most_sensitive_asset(&self) -> Option<(Address, f64)> {
        self.sensitivities
            .iter()
            .max_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap())
            .map(|(a, s)| (*a, *s))
    }

    /// Get assets that would push HF below 1.0 with given price movement.
    pub fn critical_assets(&self, max_move_pct: f64) -> Vec<(Address, f64)> {
        let threshold = 1.0 - self.base_hf;

        self.sensitivities
            .iter()
            .filter_map(|(asset, sensitivity)| {
                // For collateral (positive sensitivity), we care about price drops
                // For debt (negative sensitivity), we care about price rises
                let required_move = if *sensitivity != 0.0 {
                    threshold / sensitivity
                } else {
                    return None;
                };

                // Check if this move is within our threshold
                if required_move.abs() <= max_move_pct {
                    Some((*asset, required_move))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Age of the sensitivity computation.
    pub fn age(&self) -> std::time::Duration {
        self.computed_at.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_sensitivity() -> PositionSensitivity {
        let mut sensitivities = SmallVec::new();
        let mut price_snapshot = SmallVec::new();

        // Collateral with 0.008 sensitivity (1% price change = 0.008 HF change)
        let collateral_addr = Address::repeat_byte(1);
        sensitivities.push((collateral_addr, 0.008));
        price_snapshot.push((collateral_addr, U256::from(100_000_000u64)));

        // Debt with -0.011 sensitivity
        let debt_addr = Address::repeat_byte(2);
        sensitivities.push((debt_addr, -0.011));
        price_snapshot.push((debt_addr, U256::from(100_000_000u64)));

        PositionSensitivity {
            user: Address::ZERO,
            base_hf: 1.1,
            sensitivities,
            price_snapshot,
            computed_at: Instant::now(),
        }
    }

    #[test]
    fn test_estimate_hf() {
        let sens = create_test_sensitivity();

        // 10% collateral drop: 1.1 + (0.008 * -10) = 1.02
        let hf = sens.estimate_hf(&[(Address::repeat_byte(1), -10.0)]);
        assert!((hf - 1.02).abs() < 0.001);

        // 10% debt rise: 1.1 + (-0.011 * 10) = 0.99
        let hf = sens.estimate_hf(&[(Address::repeat_byte(2), 10.0)]);
        assert!((hf - 0.99).abs() < 0.001);
    }

    #[test]
    fn test_estimate_from_prices() {
        let sens = create_test_sensitivity();

        // Collateral drops 10%: new price = 90M (from 100M)
        let new_prices = vec![(Address::repeat_byte(1), U256::from(90_000_000u64))];

        let hf = sens.estimate_hf_from_prices(&new_prices);
        assert!((hf - 1.02).abs() < 0.001);
    }

    #[test]
    fn test_critical_assets() {
        let sens = create_test_sensitivity();

        // Find assets that could trigger liquidation with 15% move
        let critical = sens.critical_assets(15.0);

        // Debt at -0.011 sensitivity needs: (1.0 - 1.1) / -0.011 = 9.09% rise
        // This is within 15%, so debt should be critical
        assert!(!critical.is_empty());
    }
}
