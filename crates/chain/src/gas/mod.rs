//! Gas strategy abstraction for multi-chain support.
//!
//! This module provides a trait-based abstraction for gas pricing strategies,
//! supporting both Legacy and EIP-1559 transaction types.
//!
//! # Example
//!
//! ```rust,ignore
//! use liquidator_chain::gas::{GasStrategy, LegacyGasStrategy, Eip1559GasStrategy};
//!
//! // For chains like HyperLiquid that use Legacy gas pricing
//! let legacy = LegacyGasStrategy::new(1_000_000_000, 10_000_000_000);
//!
//! // For Ethereum/L2s that use EIP-1559
//! let eip1559 = Eip1559GasStrategy::new(2_000_000_000, 1.5);
//! ```

mod eip1559;
mod legacy;

pub use eip1559::Eip1559GasStrategy;
pub use legacy::LegacyGasStrategy;

use alloy::rpc::types::TransactionRequest;
use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;

/// Gas parameters fetched from the chain.
#[derive(Debug, Clone)]
pub enum GasParams {
    /// Legacy gas pricing (pre-EIP-1559).
    Legacy {
        /// Gas price in wei.
        gas_price: u128,
    },
    /// EIP-1559 gas pricing.
    Eip1559 {
        /// Maximum fee per gas in wei.
        max_fee_per_gas: u128,
        /// Maximum priority fee per gas in wei.
        max_priority_fee_per_gas: u128,
        /// Current base fee (for reference).
        base_fee: u128,
    },
}

impl GasParams {
    /// Get the effective gas price for estimation purposes.
    pub fn effective_gas_price(&self) -> u128 {
        match self {
            GasParams::Legacy { gas_price } => *gas_price,
            GasParams::Eip1559 {
                max_fee_per_gas, ..
            } => *max_fee_per_gas,
        }
    }
}

/// Trait for gas pricing strategies.
///
/// Implementations of this trait handle fetching gas prices from the chain
/// and applying them to transaction requests.
#[async_trait]
pub trait GasStrategy: Send + Sync + Debug {
    /// Fetch current gas parameters from the given RPC URL.
    ///
    /// This method should query the chain for current gas prices.
    /// The implementation may cache results to reduce RPC calls.
    async fn fetch_params(&self, rpc_url: &str) -> Result<GasParams>;

    /// Apply gas parameters to a transaction request.
    ///
    /// This modifies the transaction request in-place, adding the appropriate
    /// gas-related fields based on the strategy type.
    fn apply_gas(&self, tx: &mut TransactionRequest, params: &GasParams);

    /// Get the strategy name for logging/debugging.
    fn strategy_name(&self) -> &'static str;

    /// Check if this strategy supports the given chain ID.
    ///
    /// By default, returns true. Override for chain-specific strategies.
    fn supports_chain(&self, _chain_id: u64) -> bool {
        true
    }
}

/// Create a gas strategy from chain configuration.
///
/// # Arguments
/// * `pricing_model` - The gas pricing model string ("Legacy", "Eip1559", etc.)
/// * `default_gas_price_gwei` - Default gas price in gwei (for Legacy)
/// * `max_gas_price_gwei` - Maximum gas price in gwei
/// * `priority_fee_gwei` - Priority fee in gwei (for EIP-1559)
pub fn create_gas_strategy(
    pricing_model: &str,
    default_gas_price_gwei: f64,
    max_gas_price_gwei: f64,
    priority_fee_gwei: Option<f64>,
) -> Box<dyn GasStrategy> {
    match pricing_model.to_lowercase().as_str() {
        "eip1559" | "eip-1559" => {
            let priority_fee = priority_fee_gwei.unwrap_or(2.0);
            Box::new(Eip1559GasStrategy::new(
                (priority_fee * 1e9) as u128,
                max_gas_price_gwei / default_gas_price_gwei, // multiplier
            ))
        }
        _ => {
            // Default to Legacy
            Box::new(LegacyGasStrategy::new(
                (default_gas_price_gwei * 1e9) as u128,
                (max_gas_price_gwei * 1e9) as u128,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gas_params_effective_price() {
        let legacy = GasParams::Legacy {
            gas_price: 1_000_000_000,
        };
        assert_eq!(legacy.effective_gas_price(), 1_000_000_000);

        let eip1559 = GasParams::Eip1559 {
            max_fee_per_gas: 50_000_000_000,
            max_priority_fee_per_gas: 2_000_000_000,
            base_fee: 30_000_000_000,
        };
        assert_eq!(eip1559.effective_gas_price(), 50_000_000_000);
    }

    #[test]
    fn test_create_gas_strategy() {
        let legacy = create_gas_strategy("Legacy", 1.0, 10.0, None);
        assert_eq!(legacy.strategy_name(), "Legacy");

        let eip1559 = create_gas_strategy("Eip1559", 30.0, 500.0, Some(2.0));
        assert_eq!(eip1559.strategy_name(), "EIP-1559");

        // Unknown defaults to Legacy
        let unknown = create_gas_strategy("Unknown", 1.0, 10.0, None);
        assert_eq!(unknown.strategy_name(), "Legacy");
    }
}
