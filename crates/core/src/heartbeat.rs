//! Heartbeat prediction for oracle updates.
//!
//! Since HyperEVM has no mempool, we can't front-run oracle updates.
//! Instead, predict when heartbeat updates are likely based on observed patterns.

use alloy::primitives::Address;
use dashmap::DashMap;
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::assets::ASSETS;

/// Heartbeat predictor for oracle update timing.
pub struct HeartbeatPredictor {
    /// Last update time per oracle (timestamp, block_number)
    last_updates: DashMap<Address, (u64, u64)>,

    /// Observed heartbeat intervals per oracle
    observed_intervals: DashMap<Address, Vec<Duration>>,

    /// Expected staleness thresholds from config
    expected_staleness: HashMap<Address, Duration>,

    /// Average heartbeat interval per oracle
    average_intervals: DashMap<Address, Duration>,
}

impl HeartbeatPredictor {
    /// Create a new heartbeat predictor initialized from asset registry.
    pub fn new() -> Self {
        let mut expected_staleness = HashMap::new();

        for asset in ASSETS {
            expected_staleness.insert(asset.oracle, asset.staleness);
        }

        Self {
            last_updates: DashMap::new(),
            observed_intervals: DashMap::new(),
            expected_staleness,
            average_intervals: DashMap::new(),
        }
    }

    /// Record an oracle update.
    pub fn record_update(&self, oracle: Address, timestamp: u64, block_number: u64) {
        // Calculate interval from previous update
        if let Some(prev) = self.last_updates.get(&oracle) {
            let interval_secs = timestamp.saturating_sub(prev.0);
            let interval = Duration::from_secs(interval_secs);

            self.observed_intervals
                .entry(oracle)
                .or_default()
                .push(interval);

            // Recalculate average (keep last 10 observations)
            self.update_average(oracle);
        }

        // Update last known
        self.last_updates.insert(oracle, (timestamp, block_number));
    }

    /// Get the predicted next update time.
    pub fn next_expected_update(&self, oracle: Address) -> Option<Instant> {
        let (last_ts, _) = *self.last_updates.get(&oracle)?;
        let interval = self.get_effective_interval(&oracle)?;

        let now_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_secs();

        let elapsed = now_ts.saturating_sub(last_ts);
        let remaining = interval.as_secs().saturating_sub(elapsed);

        Some(Instant::now() + Duration::from_secs(remaining))
    }

    /// Check if an update is imminent (within threshold).
    pub fn is_update_imminent(&self, oracle: Address, threshold: Duration) -> bool {
        self.next_expected_update(oracle)
            .is_some_and(|next| next.saturating_duration_since(Instant::now()) < threshold)
    }

    /// Check if an update is imminent using default threshold (400ms = 2 blocks).
    pub fn is_update_imminent_default(&self, oracle: Address) -> bool {
        self.is_update_imminent(oracle, Duration::from_millis(400))
    }

    /// Get time since last update.
    pub fn time_since_update(&self, oracle: Address) -> Option<Duration> {
        let (last_ts, _) = *self.last_updates.get(&oracle)?;

        let now_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_secs();

        Some(Duration::from_secs(now_ts.saturating_sub(last_ts)))
    }

    /// Get staleness as percentage of expected interval.
    pub fn staleness_pct(&self, oracle: Address) -> Option<f64> {
        let elapsed = self.time_since_update(oracle)?;
        let expected = self.get_effective_interval(&oracle)?;

        Some(elapsed.as_secs_f64() / expected.as_secs_f64() * 100.0)
    }

    /// Check if oracle is stale (past expected staleness threshold).
    pub fn is_stale(&self, oracle: Address) -> bool {
        self.staleness_pct(oracle).is_some_and(|pct| pct > 100.0)
    }

    /// Get oracles approaching staleness (> 80% of expected interval).
    pub fn approaching_stale(&self) -> Vec<(Address, f64)> {
        self.last_updates
            .iter()
            .filter_map(|entry| {
                let oracle = *entry.key();
                let pct = self.staleness_pct(oracle)?;
                if pct > 80.0 {
                    Some((oracle, pct))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all stale oracles.
    pub fn stale_oracles(&self) -> Vec<Address> {
        self.last_updates
            .iter()
            .filter_map(|entry| {
                let oracle = *entry.key();
                if self.is_stale(oracle) {
                    Some(oracle)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get oracles with imminent updates.
    pub fn imminent_updates(&self, threshold: Duration) -> Vec<Address> {
        self.last_updates
            .iter()
            .filter_map(|entry| {
                let oracle = *entry.key();
                if self.is_update_imminent(oracle, threshold) {
                    Some(oracle)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get observed average interval for an oracle.
    pub fn observed_interval(&self, oracle: &Address) -> Option<Duration> {
        self.average_intervals.get(oracle).map(|d| *d)
    }

    /// Get expected staleness threshold for an oracle.
    pub fn expected_interval(&self, oracle: &Address) -> Option<Duration> {
        self.expected_staleness.get(oracle).copied()
    }

    /// Get statistics for an oracle.
    pub fn oracle_stats(&self, oracle: Address) -> Option<OracleHeartbeatStats> {
        let (last_ts, last_block) = *self.last_updates.get(&oracle)?;
        let time_since = self.time_since_update(oracle)?;
        let expected = self.get_effective_interval(&oracle)?;
        let observed = self.observed_interval(&oracle);
        let staleness_pct = self.staleness_pct(oracle)?;

        Some(OracleHeartbeatStats {
            oracle,
            last_update_ts: last_ts,
            last_update_block: last_block,
            time_since_update: time_since,
            expected_interval: expected,
            observed_interval: observed,
            staleness_pct,
            is_stale: staleness_pct > 100.0,
        })
    }

    // Private helpers

    fn get_effective_interval(&self, oracle: &Address) -> Option<Duration> {
        // Prefer observed if we have enough data, otherwise use expected
        if let Some(observed) = self.average_intervals.get(oracle) {
            if observed.as_secs() > 0 {
                return Some(*observed);
            }
        }
        self.expected_staleness.get(oracle).copied()
    }

    fn update_average(&self, oracle: Address) {
        if let Some(mut intervals) = self.observed_intervals.get_mut(&oracle) {
            // Keep only last 10 observations
            if intervals.len() > 10 {
                let drain_count = intervals.len() - 10;
                intervals.drain(0..drain_count);
            }

            if !intervals.is_empty() {
                let total: Duration = intervals.iter().sum();
                let avg = total / intervals.len() as u32;
                self.average_intervals.insert(oracle, avg);
            }
        }
    }
}

impl Default for HeartbeatPredictor {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics for an oracle's heartbeat.
#[derive(Debug, Clone)]
pub struct OracleHeartbeatStats {
    pub oracle: Address,
    pub last_update_ts: u64,
    pub last_update_block: u64,
    pub time_since_update: Duration,
    pub expected_interval: Duration,
    pub observed_interval: Option<Duration>,
    pub staleness_pct: f64,
    pub is_stale: bool,
}

/// Get known heartbeat characteristics for assets.
pub fn known_heartbeats() -> Vec<(&'static str, Duration, &'static str)> {
    vec![
        ("wHYPE", Duration::from_secs(3600), "RedStone"),
        ("USDT", Duration::from_secs(32400), "Anomalous (9h+)"),
        ("USDC", Duration::from_secs(3600), "Standard"),
        ("LSTs", Duration::from_secs(1800), "DualOracle primary"),
        ("USDHL", Duration::from_secs(60), "Pyth (very fresh)"),
        ("Synthetics", Duration::from_secs(5400), "RedStone"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_predict() {
        let predictor = HeartbeatPredictor::new();
        let oracle = Address::repeat_byte(1);

        // Manually set expected staleness
        let mut predictor = predictor;
        predictor
            .expected_staleness
            .insert(oracle, Duration::from_secs(3600));

        // Record an update
        let now_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        predictor.record_update(oracle, now_ts - 1800, 100); // 30 min ago

        // Check staleness (should be ~50%)
        let pct = predictor.staleness_pct(oracle);
        assert!(pct.is_some());
        assert!((pct.unwrap() - 50.0).abs() < 5.0); // Allow some tolerance

        // Should not be stale yet
        assert!(!predictor.is_stale(oracle));
    }

    #[test]
    fn test_imminent_detection() {
        let predictor = HeartbeatPredictor::new();
        let oracle = Address::repeat_byte(1);

        // Set up with known staleness
        let mut predictor = predictor;
        predictor
            .expected_staleness
            .insert(oracle, Duration::from_secs(60)); // 1 minute

        // Record an update 59 seconds ago
        let now_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        predictor.record_update(oracle, now_ts - 59, 100);

        // Should be imminent with 5 second threshold
        assert!(predictor.is_update_imminent(oracle, Duration::from_secs(5)));
    }
}
