//! Position data structures for tracking user lending positions.

use alloy::primitives::{Address, U256};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::time::Instant;

use crate::config::config;
use crate::sensitivity::PositionSensitivity;
use crate::trigger_index::TriggerEntry;
use crate::u256_math;

/// Position tier based on health factor and trigger distance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PositionTier {
    /// HF < 1.02, update every 100ms, pre-staged transactions
    Critical,
    /// HF 1.02-1.08, update every 500ms, swap routes cached
    Hot,
    /// HF 1.08-1.15, update every 2s, triggers computed
    Warm,
    /// HF >= 1.15, update every 10s, basic tracking
    Cold,
}

impl PositionTier {
    /// Classify tier based on health factor.
    pub fn from_health_factor(hf: f64) -> Self {
        let cfg = &config().tiers;
        if hf < cfg.critical_hf_threshold {
            Self::Critical
        } else if hf < cfg.hot_hf_threshold {
            Self::Hot
        } else if hf < cfg.warm_hf_threshold {
            Self::Warm
        } else {
            Self::Cold
        }
    }

    /// Classify tier based on trigger distance percentage.
    pub fn from_trigger_distance(distance_pct: f64) -> Self {
        let cfg = &config().tiers;
        if distance_pct < cfg.critical_trigger_distance_pct {
            Self::Critical
        } else if distance_pct < cfg.hot_trigger_distance_pct {
            Self::Hot
        } else if distance_pct < cfg.warm_trigger_distance_pct {
            Self::Warm
        } else {
            Self::Cold
        }
    }

    /// Get the more aggressive tier between HF-based and trigger-distance-based.
    pub fn classify(hf: f64, trigger_distance_pct: f64) -> Self {
        let by_hf = Self::from_health_factor(hf);
        let by_trigger = Self::from_trigger_distance(trigger_distance_pct);

        // Return whichever is more critical (lower ordinal)
        match (by_hf, by_trigger) {
            (Self::Critical, _) | (_, Self::Critical) => Self::Critical,
            (Self::Hot, _) | (_, Self::Hot) => Self::Hot,
            (Self::Warm, _) | (_, Self::Warm) => Self::Warm,
            _ => Self::Cold,
        }
    }

    /// Get update interval for this tier.
    pub fn update_interval(&self) -> std::time::Duration {
        let cfg = &config().scanner;
        match self {
            Self::Critical => cfg.critical_interval(),
            Self::Hot => cfg.hot_interval(),
            Self::Warm => cfg.warm_interval(),
            Self::Cold => cfg.cold_interval(),
        }
    }

    /// Check if this tier should have pre-staged transactions.
    pub fn should_pre_stage(&self) -> bool {
        matches!(self, Self::Critical)
    }

    /// Check if this tier should cache swap routes.
    pub fn should_cache_swaps(&self) -> bool {
        matches!(self, Self::Critical | Self::Hot)
    }
}

/// Collateral position data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollateralData {
    /// Token address
    pub asset: Address,
    /// Raw balance (token decimals)
    pub amount: U256,
    /// Oracle price (8 decimals)
    pub price: U256,
    /// Token decimals
    pub decimals: u8,
    /// USD value (computed)
    pub value_usd: f64,
    /// Liquidation threshold (in basis points, e.g., 8000 = 80%)
    pub liquidation_threshold: u16,
    /// Whether this collateral is enabled for liquidation
    pub enabled: bool,
}

impl CollateralData {
    /// Calculate USD value from amount and price.
    /// Uses native U256 arithmetic for performance (2-5x faster than String parsing).
    #[inline]
    pub fn calculate_usd_value(amount: U256, price: U256, decimals: u8) -> f64 {
        u256_math::calculate_usd_f64(amount, price, decimals)
    }

    /// Calculate USD value as WAD (18 decimals) for precise arithmetic.
    /// Use this for internal calculations to avoid floating point errors.
    #[inline]
    pub fn calculate_usd_wad(amount: U256, price: U256, decimals: u8) -> U256 {
        u256_math::calculate_usd_wad(amount, price, decimals)
    }

    /// Get liquidation threshold as a decimal (e.g., 0.80 for 80%).
    pub fn lt_decimal(&self) -> f64 {
        self.liquidation_threshold as f64 / 10000.0
    }

    /// Calculate risk-adjusted value (value * LT).
    pub fn risk_adjusted_value(&self) -> f64 {
        self.value_usd * self.lt_decimal()
    }
}

/// Debt position data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebtData {
    /// Token address
    pub asset: Address,
    /// Raw debt amount (token decimals)
    pub amount: U256,
    /// Oracle price (8 decimals)
    pub price: U256,
    /// Token decimals
    pub decimals: u8,
    /// USD value (computed)
    pub value_usd: f64,
}

impl DebtData {
    /// Calculate USD value from amount and price.
    pub fn calculate_usd_value(amount: U256, price: U256, decimals: u8) -> f64 {
        CollateralData::calculate_usd_value(amount, price, decimals)
    }
}

/// Tracked position with all computed data for liquidation monitoring.
#[derive(Debug, Clone)]
pub struct TrackedPosition {
    /// User wallet address
    pub user: Address,
    /// Current health factor (1.0 = liquidatable)
    pub health_factor: f64,
    /// Position tier for update scheduling
    pub tier: PositionTier,
    /// Trigger prices for liquidation
    pub triggers: SmallVec<[TriggerEntry; 4]>,
    /// Minimum distance to any trigger (percentage)
    pub min_trigger_distance_pct: f64,
    /// Collateral positions
    pub collaterals: SmallVec<[(Address, CollateralData); 4]>,
    /// Debt positions
    pub debts: SmallVec<[(Address, DebtData); 4]>,
    /// Pre-computed sensitivity for fast HF estimation
    pub sensitivity: Option<PositionSensitivity>,
    /// Last update timestamp
    pub last_updated: Instant,
    /// Hash of position state for invalidation detection
    pub state_hash: u64,
}

impl TrackedPosition {
    /// Create a new tracked position.
    pub fn new(user: Address) -> Self {
        Self {
            user,
            health_factor: f64::MAX,
            tier: PositionTier::Cold,
            triggers: SmallVec::new(),
            min_trigger_distance_pct: 100.0,
            collaterals: SmallVec::new(),
            debts: SmallVec::new(),
            sensitivity: None,
            last_updated: Instant::now(),
            state_hash: 0,
        }
    }

    /// Calculate health factor from collaterals and debts.
    pub fn calculate_health_factor(&self) -> f64 {
        let total_collateral_adjusted: f64 = self
            .collaterals
            .iter()
            .filter(|(_, c)| c.enabled)
            .map(|(_, c)| c.risk_adjusted_value())
            .sum();

        let total_debt: f64 = self.debts.iter().map(|(_, d)| d.value_usd).sum();

        if total_debt == 0.0 {
            return f64::MAX;
        }

        total_collateral_adjusted / total_debt
    }

    /// Get total collateral USD value.
    pub fn total_collateral_usd(&self) -> f64 {
        self.collaterals.iter().map(|(_, c)| c.value_usd).sum()
    }

    /// Get total debt USD value.
    pub fn total_debt_usd(&self) -> f64 {
        self.debts.iter().map(|(_, d)| d.value_usd).sum()
    }

    /// Check if position is liquidatable (HF < 1.0).
    pub fn is_liquidatable(&self) -> bool {
        self.health_factor < 1.0
    }

    /// Check if position is bad debt (no seizable collateral).
    pub fn is_bad_debt(&self) -> bool {
        let cfg = &config().position;

        // Dust position
        if self.total_collateral_usd() < cfg.dust_threshold_usd {
            return true;
        }

        // Already bad debt
        if self.health_factor < cfg.bad_debt_hf_threshold {
            return true;
        }

        // Self-collateralized (single asset)
        if self.collaterals.len() == 1
            && self.debts.len() == 1
            && self.collaterals[0].0 == self.debts[0].0
        {
            return true;
        }

        // Largest collateral and debt are the same token (can't swap token for itself)
        if let (Some((collateral_addr, _)), Some((debt_addr, _))) =
            (self.largest_collateral(), self.largest_debt())
        {
            if collateral_addr == debt_addr {
                return true;
            }
        }

        false
    }

    /// Get the largest collateral by USD value.
    pub fn largest_collateral(&self) -> Option<&(Address, CollateralData)> {
        self.collaterals
            .iter()
            .filter(|(_, c)| c.enabled)
            .max_by(|a, b| a.1.value_usd.partial_cmp(&b.1.value_usd).unwrap())
    }

    /// Get the largest debt by USD value.
    pub fn largest_debt(&self) -> Option<&(Address, DebtData)> {
        self.debts
            .iter()
            .max_by(|a, b| a.1.value_usd.partial_cmp(&b.1.value_usd).unwrap())
    }

    /// Compute state hash for change detection.
    pub fn compute_state_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();

        // Hash collateral amounts
        for (addr, col) in &self.collaterals {
            addr.hash(&mut hasher);
            col.amount.to_string().hash(&mut hasher);
        }

        // Hash debt amounts
        for (addr, debt) in &self.debts {
            addr.hash(&mut hasher);
            debt.amount.to_string().hash(&mut hasher);
        }

        hasher.finish()
    }

    /// Update the tier based on current health factor and trigger distance.
    pub fn update_tier(&mut self) {
        self.tier = PositionTier::classify(self.health_factor, self.min_trigger_distance_pct);
    }

    /// Check if position needs update based on tier interval.
    pub fn needs_update(&self) -> bool {
        self.last_updated.elapsed() >= self.tier.update_interval()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_classification() {
        // HF-based
        assert_eq!(PositionTier::from_health_factor(1.01), PositionTier::Critical);
        assert_eq!(PositionTier::from_health_factor(1.05), PositionTier::Hot);
        assert_eq!(PositionTier::from_health_factor(1.10), PositionTier::Warm);
        assert_eq!(PositionTier::from_health_factor(1.20), PositionTier::Cold);

        // Combined classification (more aggressive wins)
        assert_eq!(
            PositionTier::classify(1.10, 0.5), // Warm by HF, Critical by trigger
            PositionTier::Critical
        );
    }

    #[test]
    fn test_health_factor_calculation() {
        let mut pos = TrackedPosition::new(Address::ZERO);

        // Add collateral: 1000 USD, 80% LT
        pos.collaterals.push((
            Address::ZERO,
            CollateralData {
                asset: Address::ZERO,
                amount: U256::from(1000_000000u64), // 1000 USDC
                price: U256::from(100_000_000u64),  // $1.00
                decimals: 6,
                value_usd: 1000.0,
                liquidation_threshold: 8000,
                enabled: true,
            },
        ));

        // Add debt: 500 USD
        pos.debts.push((
            Address::ZERO,
            DebtData {
                asset: Address::ZERO,
                amount: U256::from(500_000000u64),
                price: U256::from(100_000_000u64),
                decimals: 6,
                value_usd: 500.0,
            },
        ));

        // HF = (1000 * 0.80) / 500 = 1.6
        let hf = pos.calculate_health_factor();
        assert!((hf - 1.6).abs() < 0.001);
    }
}
