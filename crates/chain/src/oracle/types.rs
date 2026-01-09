//! Oracle type definitions.

use alloy::primitives::{Address, U256};
use serde::{Deserialize, Serialize};

/// Oracle type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum OracleType {
    /// Standard Chainlink aggregator
    #[default]
    Chainlink,
    /// RedStone oracle
    RedStone,
    /// Pyth Network oracle
    Pyth,
    /// DualOracle (multi-tier fallback)
    DualOracle,
    /// Custom oracle implementation
    Custom,
}

impl OracleType {
    /// Parse oracle type from string.
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "chainlink" | "chainlink-aggregator" => Self::Chainlink,
            "redstone" => Self::RedStone,
            "pyth" => Self::Pyth,
            "dual" | "dualoracle" | "dual-oracle" => Self::DualOracle,
            _ => Self::Custom,
        }
    }

    /// Get standard decimals for this oracle type.
    pub fn default_decimals(&self) -> u8 {
        match self {
            Self::Chainlink => 8,
            Self::RedStone => 8,
            Self::Pyth => 8,
            Self::DualOracle => 8,
            Self::Custom => 18,
        }
    }

    /// Get default heartbeat for this oracle type.
    pub fn default_heartbeat_secs(&self) -> u64 {
        match self {
            Self::Chainlink => 3600,   // 1 hour typical
            Self::RedStone => 1800,    // 30 minutes
            Self::Pyth => 60,          // 1 minute
            Self::DualOracle => 1800,  // 30 minutes for primary
            Self::Custom => 3600,
        }
    }
}

/// Price data with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceData {
    /// Price value (in oracle decimals)
    pub price: U256,
    /// Price decimals
    pub decimals: u8,
    /// Timestamp when price was updated
    pub timestamp: u64,
    /// Block number of the update
    pub block_number: u64,
    /// Source oracle type
    pub oracle_type: OracleType,
    /// Confidence/deviation (if available)
    pub confidence: Option<f64>,
}

impl PriceData {
    /// Create new price data.
    pub fn new(price: U256, decimals: u8, timestamp: u64, block_number: u64, oracle_type: OracleType) -> Self {
        Self {
            price,
            decimals,
            timestamp,
            block_number,
            oracle_type,
            confidence: None,
        }
    }

    /// Get price as f64.
    pub fn price_f64(&self) -> f64 {
        let divisor = 10_f64.powi(self.decimals as i32);
        self.price.to_string().parse::<f64>().unwrap_or(0.0) / divisor
    }

    /// Check if price is stale.
    pub fn is_stale(&self, threshold_secs: u64, current_time: u64) -> bool {
        current_time.saturating_sub(self.timestamp) > threshold_secs
    }

    /// Get age in seconds.
    pub fn age_secs(&self, current_time: u64) -> u64 {
        current_time.saturating_sub(self.timestamp)
    }

    /// Normalize price to 18 decimals.
    pub fn normalize_to_18(&self) -> U256 {
        if self.decimals == 18 {
            self.price
        } else if self.decimals < 18 {
            self.price * U256::from(10u64.pow((18 - self.decimals) as u32))
        } else {
            self.price / U256::from(10u64.pow((self.decimals - 18) as u32))
        }
    }
}

/// Price source identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PriceSource {
    /// Oracle address
    pub oracle: Address,
    /// Asset address
    pub asset: Address,
    /// Oracle type
    pub oracle_type: OracleType,
}

impl PriceSource {
    /// Create a new price source.
    pub fn new(oracle: Address, asset: Address, oracle_type: OracleType) -> Self {
        Self {
            oracle,
            asset,
            oracle_type,
        }
    }
}

/// Oracle price with source information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OraclePrice {
    /// Price data
    pub data: PriceData,
    /// Source information
    pub source: PriceSource,
}

impl OraclePrice {
    /// Create a new oracle price.
    pub fn new(data: PriceData, source: PriceSource) -> Self {
        Self { data, source }
    }

    /// Get price as f64.
    pub fn price_f64(&self) -> f64 {
        self.data.price_f64()
    }

    /// Check if stale.
    pub fn is_stale(&self, threshold_secs: u64, current_time: u64) -> bool {
        self.data.is_stale(threshold_secs, current_time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oracle_type_parsing() {
        assert_eq!(OracleType::from_str("chainlink"), OracleType::Chainlink);
        assert_eq!(OracleType::from_str("RedStone"), OracleType::RedStone);
        assert_eq!(OracleType::from_str("pyth"), OracleType::Pyth);
        assert_eq!(OracleType::from_str("dual-oracle"), OracleType::DualOracle);
        assert_eq!(OracleType::from_str("unknown"), OracleType::Custom);
    }

    #[test]
    fn test_price_data_conversion() {
        let price = PriceData::new(
            U256::from(200_000_000_000u64), // $2000 with 8 decimals
            8,
            1700000000,
            100,
            OracleType::Chainlink,
        );

        assert!((price.price_f64() - 2000.0).abs() < 0.01);
    }

    #[test]
    fn test_price_normalization() {
        // 8 decimal price → 18 decimals
        let price = PriceData::new(
            U256::from(100_000_000u64), // $1.00 with 8 decimals
            8,
            1700000000,
            100,
            OracleType::Chainlink,
        );

        let normalized = price.normalize_to_18();
        // 1e8 * 1e10 = 1e18
        assert_eq!(normalized, U256::from(10u128.pow(18)));
    }

    #[test]
    fn test_staleness() {
        let price = PriceData::new(
            U256::from(100_000_000u64),
            8,
            1700000000,
            100,
            OracleType::Chainlink,
        );

        // 1 hour threshold
        let threshold = 3600;
        let current = 1700000000 + 3601;
        assert!(price.is_stale(threshold, current));

        let current = 1700000000 + 3599;
        assert!(!price.is_stale(threshold, current));
    }
}
