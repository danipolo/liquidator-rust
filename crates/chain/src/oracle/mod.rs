//! Oracle abstraction layer for multi-oracle support.
//!
//! This module provides traits and implementations for interacting with
//! different oracle types (Chainlink, RedStone, Pyth, etc.) in a unified way.
//!
//! # Architecture
//!
//! The oracle layer is organized into:
//!
//! - [`Oracle`]: Core trait for oracle interactions (price fetching, validation)
//! - [`OracleProvider`]: Manages multiple oracles and provides price aggregation
//! - [`OracleConfig`]: Configuration for oracle setup from TOML files
//!
//! # Supported Oracle Types
//!
//! - **Chainlink**: Standard Chainlink aggregators (8 decimal prices)
//! - **RedStone**: RedStone price feeds
//! - **Pyth**: Pyth Network oracles
//! - **DualOracle**: Multi-tier fallback oracles (Primary → Secondary → Emergency)
//!
//! # Example
//!
//! ```rust,ignore
//! use liquidator_chain::oracle::{Oracle, OracleProvider, ChainlinkOracle};
//!
//! // Create an oracle
//! let oracle = ChainlinkOracle::new(aggregator_address, asset_address, 8);
//!
//! // Get latest price
//! let price = oracle.get_price().await?;
//!
//! // Check staleness
//! if oracle.is_stale(3600).await? {
//!     warn!("Price is stale!");
//! }
//! ```

mod chainlink;
mod config;
mod provider;
mod types;

pub use chainlink::{ChainlinkOracle, ChainlinkOracleBuilder};
pub use config::{OracleConfig, OracleFactory, OracleTypeConfig, OraclesConfig};
pub use provider::{OracleProvider, PriceCache};
pub use types::{OraclePrice, OracleType, PriceData, PriceSource};

use alloy::primitives::{Address, U256};
use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;
use std::time::Duration;

/// Core trait for oracle interactions.
///
/// This trait defines the interface for fetching and validating prices
/// from different oracle implementations.
#[async_trait]
pub trait Oracle: Send + Sync + Debug {
    /// Get the oracle type identifier.
    fn oracle_type(&self) -> OracleType;

    /// Get the oracle contract address.
    fn address(&self) -> Address;

    /// Get the asset address this oracle prices.
    fn asset(&self) -> Address;

    /// Get the price decimals (typically 8 for Chainlink).
    fn decimals(&self) -> u8;

    /// Get the current price from the oracle.
    async fn get_price(&self) -> Result<PriceData>;

    /// Get the latest round data (price, timestamp, round ID).
    async fn get_latest_round(&self) -> Result<RoundData>;

    /// Check if the oracle price is stale.
    ///
    /// # Arguments
    /// * `threshold` - Maximum age in seconds before considering stale
    async fn is_stale(&self, threshold: u64) -> Result<bool> {
        let round = self.get_latest_round().await?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Ok(now.saturating_sub(round.updated_at) > threshold)
    }

    /// Get the heartbeat interval for this oracle (if known).
    fn heartbeat(&self) -> Option<Duration> {
        None
    }

    /// Validate price against sanity checks.
    fn validate_price(&self, price: U256) -> bool {
        // Basic sanity: price should be positive and not impossibly large
        !price.is_zero() && price < U256::from(10u128.pow(20)) // Max ~10^12 USD
    }
}

/// Round data from an oracle.
#[derive(Debug, Clone)]
pub struct RoundData {
    /// Round ID
    pub round_id: u128,
    /// Price answer
    pub answer: U256,
    /// Timestamp when round started
    pub started_at: u64,
    /// Timestamp when answer was computed
    pub updated_at: u64,
    /// Round ID for which answer was computed
    pub answered_in_round: u128,
}

impl RoundData {
    /// Check if this round's data is valid.
    pub fn is_valid(&self) -> bool {
        !self.answer.is_zero() && self.updated_at > 0 && self.answered_in_round >= self.round_id
    }

    /// Convert answer to f64 with given decimals.
    pub fn price_f64(&self, decimals: u8) -> f64 {
        let divisor = 10_f64.powi(decimals as i32);
        self.answer.to_string().parse::<f64>().unwrap_or(0.0) / divisor
    }
}

/// Trait for oracle event handling.
pub trait OracleEventHandler: Send + Sync {
    /// Handle a price update event.
    fn on_price_update(&self, oracle: Address, asset: Address, price: U256, timestamp: u64);

    /// Handle a staleness alert.
    fn on_stale_price(&self, oracle: Address, asset: Address, staleness_secs: u64);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_data_validity() {
        let valid_round = RoundData {
            round_id: 100,
            answer: U256::from(200_000_000_000u64), // $2000
            started_at: 1700000000,
            updated_at: 1700000100,
            answered_in_round: 100,
        };
        assert!(valid_round.is_valid());

        let invalid_round = RoundData {
            round_id: 100,
            answer: U256::ZERO,
            started_at: 1700000000,
            updated_at: 0,
            answered_in_round: 99,
        };
        assert!(!invalid_round.is_valid());
    }

    #[test]
    fn test_round_data_price_conversion() {
        let round = RoundData {
            round_id: 100,
            answer: U256::from(200_000_000_000u64), // $2000 with 8 decimals
            started_at: 1700000000,
            updated_at: 1700000100,
            answered_in_round: 100,
        };

        let price = round.price_f64(8);
        assert!((price - 2000.0).abs() < 0.01);
    }
}
