//! DualOracle tier monitoring for LST assets.
//!
//! LST assets (kHYPE, wstHYPE, beHYPE) use 3-tier fallback oracles:
//! - Primary: RedStone fundamental rate
//! - Secondary: Chainlink fundamental rate
//! - Emergency: Market rate fallback
//!
//! Tier transitions can cause price jumps and create liquidation opportunities.

use alloy::primitives::Address;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, info};

/// DualOracle tier levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DualOracleTier {
    /// Primary tier: RedStone fundamental rate
    Primary,
    /// Secondary tier: Chainlink fundamental rate
    Secondary,
    /// Emergency tier: Market rate fallback
    Emergency,
}

impl DualOracleTier {
    /// Get the next tier in fallback sequence.
    pub fn next_tier(&self) -> Option<Self> {
        match self {
            Self::Primary => Some(Self::Secondary),
            Self::Secondary => Some(Self::Emergency),
            Self::Emergency => None,
        }
    }

    /// Get staleness threshold for this tier (in seconds).
    pub fn staleness_threshold(&self) -> Duration {
        match self {
            Self::Primary => Duration::from_secs(1800),   // 30 minutes
            Self::Secondary => Duration::from_secs(3600), // 1 hour
            Self::Emergency => Duration::from_secs(86400), // 24 hours (rarely used)
        }
    }

    /// Get tier priority (lower = higher priority).
    pub fn priority(&self) -> u8 {
        match self {
            Self::Primary => 0,
            Self::Secondary => 1,
            Self::Emergency => 2,
        }
    }
}

/// Information about a tier transition.
#[derive(Debug, Clone)]
pub struct TierTransition {
    /// Oracle address
    pub oracle: Address,
    /// Tier transitioning from
    pub from: DualOracleTier,
    /// Tier transitioning to
    pub to: DualOracleTier,
    /// Expected price impact (percentage)
    pub expected_price_impact: Option<f64>,
    /// Time until transition (None if already transitioning)
    pub time_until: Option<Duration>,
}

/// Staleness info for a specific tier.
#[derive(Debug, Clone)]
pub struct TierStaleness {
    /// Last update timestamp
    pub last_update: u64,
    /// Current staleness duration
    pub staleness: Duration,
    /// Staleness as percentage of threshold
    pub staleness_pct: f64,
    /// Whether this tier is stale
    pub is_stale: bool,
}

/// DualOracle monitor for tracking tier states and transitions.
pub struct DualOracleMonitor {
    /// Current tier per LST oracle
    current_tiers: DashMap<Address, DualOracleTier>,

    /// Last update timestamp per (oracle, tier)
    tier_updates: DashMap<(Address, DualOracleTier), u64>,

    /// Observed price deviations between tiers
    tier_deviations: DashMap<Address, f64>,

    /// Known DualOracle addresses
    dual_oracles: Vec<Address>,
}

impl DualOracleMonitor {
    /// Create a new DualOracle monitor.
    pub fn new(dual_oracles: Vec<Address>) -> Self {
        let monitor = Self {
            current_tiers: DashMap::new(),
            tier_updates: DashMap::new(),
            tier_deviations: DashMap::new(),
            dual_oracles,
        };

        // Initialize all oracles to Primary tier
        for oracle in &monitor.dual_oracles {
            monitor.current_tiers.insert(*oracle, DualOracleTier::Primary);
        }

        monitor
    }

    /// Record an update for a specific tier.
    pub fn record_tier_update(&self, oracle: Address, tier: DualOracleTier, timestamp: u64) {
        self.tier_updates.insert((oracle, tier), timestamp);
        debug!(
            oracle = %oracle,
            tier = ?tier,
            timestamp = timestamp,
            "Recorded tier update"
        );
    }

    /// Set the current tier for an oracle.
    pub fn set_current_tier(&self, oracle: Address, tier: DualOracleTier) {
        let previous = self.current_tiers.insert(oracle, tier);

        if previous.is_some_and(|p| p != tier) {
            info!(
                oracle = %oracle,
                from = ?previous,
                to = ?tier,
                "DualOracle tier changed"
            );
        }
    }

    /// Get current tier for an oracle.
    pub fn current_tier(&self, oracle: &Address) -> Option<DualOracleTier> {
        self.current_tiers.get(oracle).map(|t| *t)
    }

    /// Check for tier transition opportunity.
    pub fn check_transition(&self, oracle: Address) -> Option<TierTransition> {
        let current = *self.current_tiers.get(&oracle)?;
        let next = current.next_tier()?;

        // Get current staleness
        let staleness = self.get_tier_staleness(oracle, current)?;

        if staleness.is_stale {
            Some(TierTransition {
                oracle,
                from: current,
                to: next,
                expected_price_impact: self.tier_deviations.get(&oracle).map(|d| *d),
                time_until: None,
            })
        } else if staleness.staleness_pct > 80.0 {
            // Approaching staleness
            let remaining = current.staleness_threshold()
                .saturating_sub(staleness.staleness);

            Some(TierTransition {
                oracle,
                from: current,
                to: next,
                expected_price_impact: self.tier_deviations.get(&oracle).map(|d| *d),
                time_until: Some(remaining),
            })
        } else {
            None
        }
    }

    /// Get staleness info for a specific tier.
    pub fn get_tier_staleness(
        &self,
        oracle: Address,
        tier: DualOracleTier,
    ) -> Option<TierStaleness> {
        let last_update = *self.tier_updates.get(&(oracle, tier))?;
        let threshold = tier.staleness_threshold();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_secs();

        let staleness_secs = now.saturating_sub(last_update);
        let staleness = Duration::from_secs(staleness_secs);
        let staleness_pct = (staleness_secs as f64 / threshold.as_secs() as f64) * 100.0;

        Some(TierStaleness {
            last_update,
            staleness,
            staleness_pct,
            is_stale: staleness > threshold,
        })
    }

    /// Record observed price deviation between tiers.
    pub fn record_tier_deviation(&self, oracle: Address, deviation_pct: f64) {
        self.tier_deviations.insert(oracle, deviation_pct);
        debug!(
            oracle = %oracle,
            deviation = deviation_pct,
            "Recorded tier deviation"
        );
    }

    /// Get oracles approaching tier transition.
    pub fn approaching_transitions(&self) -> Vec<TierTransition> {
        self.dual_oracles
            .iter()
            .filter_map(|oracle| self.check_transition(*oracle))
            .filter(|t| t.time_until.is_some()) // Only approaching, not already stale
            .collect()
    }

    /// Get oracles currently in transition (stale tier).
    pub fn active_transitions(&self) -> Vec<TierTransition> {
        self.dual_oracles
            .iter()
            .filter_map(|oracle| self.check_transition(*oracle))
            .filter(|t| t.time_until.is_none()) // Already stale
            .collect()
    }

    /// Get all stale tiers.
    pub fn stale_tiers(&self) -> Vec<(Address, DualOracleTier)> {
        let mut stale = Vec::new();

        for oracle in &self.dual_oracles {
            if let Some(current) = self.current_tier(oracle) {
                if let Some(staleness) = self.get_tier_staleness(*oracle, current) {
                    if staleness.is_stale {
                        stale.push((*oracle, current));
                    }
                }
            }
        }

        stale
    }

    /// Get statistics.
    pub fn stats(&self) -> DualOracleStats {
        let total = self.dual_oracles.len();
        let primary = self.current_tiers.iter()
            .filter(|e| *e.value() == DualOracleTier::Primary)
            .count();
        let secondary = self.current_tiers.iter()
            .filter(|e| *e.value() == DualOracleTier::Secondary)
            .count();
        let emergency = self.current_tiers.iter()
            .filter(|e| *e.value() == DualOracleTier::Emergency)
            .count();
        let approaching = self.approaching_transitions().len();
        let active = self.active_transitions().len();

        DualOracleStats {
            total_oracles: total,
            on_primary: primary,
            on_secondary: secondary,
            on_emergency: emergency,
            approaching_transition: approaching,
            actively_transitioning: active,
        }
    }
}

impl Default for DualOracleMonitor {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

/// Statistics about DualOracle states.
#[derive(Debug, Clone)]
pub struct DualOracleStats {
    pub total_oracles: usize,
    pub on_primary: usize,
    pub on_secondary: usize,
    pub on_emergency: usize,
    pub approaching_transition: usize,
    pub actively_transitioning: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_sequence() {
        assert_eq!(
            DualOracleTier::Primary.next_tier(),
            Some(DualOracleTier::Secondary)
        );
        assert_eq!(
            DualOracleTier::Secondary.next_tier(),
            Some(DualOracleTier::Emergency)
        );
        assert_eq!(DualOracleTier::Emergency.next_tier(), None);
    }

    #[test]
    fn test_staleness_thresholds() {
        assert_eq!(
            DualOracleTier::Primary.staleness_threshold(),
            Duration::from_secs(1800)
        );
        assert_eq!(
            DualOracleTier::Secondary.staleness_threshold(),
            Duration::from_secs(3600)
        );
    }

    #[test]
    fn test_monitor_initialization() {
        let oracles = vec![
            Address::repeat_byte(1),
            Address::repeat_byte(2),
            Address::repeat_byte(3),
        ];

        let monitor = DualOracleMonitor::new(oracles.clone());

        // All should start on Primary
        for oracle in &oracles {
            assert_eq!(
                monitor.current_tier(oracle),
                Some(DualOracleTier::Primary)
            );
        }
    }

    #[test]
    fn test_tier_transition_detection() {
        let oracle = Address::repeat_byte(1);
        let monitor = DualOracleMonitor::new(vec![oracle]);

        // Record an old update (stale)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // 35 minutes ago (past 30 min threshold)
        monitor.record_tier_update(oracle, DualOracleTier::Primary, now - 2100);

        let transition = monitor.check_transition(oracle);
        assert!(transition.is_some());

        let t = transition.unwrap();
        assert_eq!(t.from, DualOracleTier::Primary);
        assert_eq!(t.to, DualOracleTier::Secondary);
        assert!(t.time_until.is_none()); // Already stale
    }
}
