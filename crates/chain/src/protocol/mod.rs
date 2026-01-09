//! Protocol abstraction layer for multi-protocol support.
//!
//! This module provides traits for interacting with different lending protocols
//! (AAVE v3, AAVE v4, Compound v3, etc.) in a unified way.
//!
//! # Architecture
//!
//! The protocol layer is organized into two main traits:
//!
//! - [`LendingProtocol`]: Core protocol operations (position queries, health factors)
//! - [`LiquidatableProtocol`]: Liquidation-specific operations
//!
//! # Example
//!
//! ```rust,ignore
//! use liquidator_chain::protocol::{LendingProtocol, LiquidatableProtocol};
//!
//! // Create a protocol instance
//! let protocol = AaveV3Protocol::new(config)?;
//!
//! // Fetch a user's position
//! let position = protocol.get_position(user_address).await?;
//!
//! // Check if liquidatable
//! if protocol.is_liquidatable(position.health_factor) {
//!     let calldata = protocol.encode_liquidation(&params)?;
//!     // Execute liquidation...
//! }
//! ```

mod aave_v3;
mod events;
mod factory;

pub use aave_v3::{AaveV3Config, AaveV3Protocol, AssetConfig};
pub use events::{PoolEvent, PoolEventType, ProtocolEventSignatures};
pub use factory::{
    parse_address, AaveV3ConfigBuilder, ProtocolConfig as ChainProtocolConfig, ProtocolFactory,
    ProtocolSwapConfig,
};

use crate::contracts::SwapAdapter;
use alloy::primitives::{Address, Bytes, U256};
use anyhow::Result;
use async_trait::async_trait;
use smallvec::SmallVec;
use std::fmt::Debug;

/// Protocol version identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProtocolVersion {
    /// AAVE V3 and forks (HyperLend, etc.)
    AaveV3,
    /// AAVE V4 (upcoming)
    AaveV4,
    /// Compound V3 (Comet)
    CompoundV3,
    /// Custom/unknown protocol
    Custom,
}

impl ProtocolVersion {
    /// Parse from string (e.g., from config).
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "aave-v3" | "aavev3" | "aave_v3" => Self::AaveV3,
            "aave-v4" | "aavev4" | "aave_v4" => Self::AaveV4,
            "compound-v3" | "compoundv3" | "compound_v3" | "comet" => Self::CompoundV3,
            _ => Self::Custom,
        }
    }
}

/// Collateral position in a lending protocol.
#[derive(Debug, Clone)]
pub struct CollateralPosition {
    /// Token address
    pub asset: Address,
    /// Raw balance (token decimals)
    pub balance: U256,
    /// Oracle price (8 decimals by default)
    pub price: U256,
    /// Token decimals
    pub decimals: u8,
    /// USD value
    pub value_usd: f64,
    /// Liquidation threshold (basis points, e.g., 8000 = 80%)
    pub liquidation_threshold_bps: u16,
    /// Whether enabled as collateral
    pub enabled: bool,
}

/// Debt position in a lending protocol.
#[derive(Debug, Clone)]
pub struct DebtPosition {
    /// Token address
    pub asset: Address,
    /// Raw debt amount (token decimals)
    pub balance: U256,
    /// Oracle price (8 decimals by default)
    pub price: U256,
    /// Token decimals
    pub decimals: u8,
    /// USD value
    pub value_usd: f64,
}

/// Complete position data for a user.
#[derive(Debug, Clone)]
pub struct PositionData {
    /// User address
    pub user: Address,
    /// Collateral positions
    pub collaterals: SmallVec<[CollateralPosition; 4]>,
    /// Debt positions
    pub debts: SmallVec<[DebtPosition; 4]>,
    /// Health factor (calculated by protocol or computed locally)
    pub health_factor: f64,
    /// Total collateral USD value
    pub total_collateral_usd: f64,
    /// Total debt USD value
    pub total_debt_usd: f64,
    /// Timestamp of fetch (block number or unix timestamp)
    pub timestamp: u64,
}

impl PositionData {
    /// Check if position is liquidatable (HF < 1.0).
    pub fn is_liquidatable(&self) -> bool {
        self.health_factor < 1.0
    }

    /// Get the largest collateral by USD value.
    pub fn largest_collateral(&self) -> Option<&CollateralPosition> {
        self.collaterals
            .iter()
            .filter(|c| c.enabled)
            .max_by(|a, b| a.value_usd.partial_cmp(&b.value_usd).unwrap())
    }

    /// Get the largest debt by USD value.
    pub fn largest_debt(&self) -> Option<&DebtPosition> {
        self.debts
            .iter()
            .max_by(|a, b| a.value_usd.partial_cmp(&b.value_usd).unwrap())
    }
}

/// Parameters for protocol liquidation.
#[derive(Debug, Clone)]
pub struct LiquidationParams {
    /// Close factor (0.0-1.0, e.g., 0.5 = 50%)
    pub close_factor: f64,
    /// Liquidation threshold (HF below which liquidation is allowed)
    pub liquidation_threshold: f64,
    /// Default liquidation bonus in basis points
    pub default_liquidation_bonus_bps: u16,
}

impl Default for LiquidationParams {
    fn default() -> Self {
        Self {
            close_factor: 0.5,
            liquidation_threshold: 1.0,
            default_liquidation_bonus_bps: 500, // 5%
        }
    }
}

/// Parameters for a liquidation call.
#[derive(Debug, Clone)]
pub struct LiquidationCallParams {
    /// User to liquidate
    pub user: Address,
    /// Collateral asset to seize
    pub collateral_asset: Address,
    /// Debt asset to repay
    pub debt_asset: Address,
    /// Amount of debt to cover (or max U256 for full liquidation)
    pub debt_to_cover: U256,
    /// Minimum collateral to receive (for slippage protection)
    pub min_collateral_out: U256,
    /// Swap data for flash liquidation (protocol-specific)
    pub swap_data: Option<Bytes>,
    /// Whether to receive underlying token or aToken
    pub receive_atoken: bool,
}

/// Core trait for lending protocol interactions.
///
/// This trait defines the interface for querying positions and calculating
/// health factors across different lending protocols.
#[async_trait]
pub trait LendingProtocol: Send + Sync + Debug {
    /// Get protocol identifier (e.g., "aave-v3", "compound-v3").
    fn protocol_id(&self) -> &str;

    /// Get protocol version.
    fn version(&self) -> ProtocolVersion;

    /// Get the pool/comptroller contract address.
    fn pool_address(&self) -> Address;

    /// Get oracle contract address (if applicable).
    fn oracle_address(&self) -> Option<Address>;

    /// Fetch position data for a user.
    async fn get_position(&self, user: Address) -> Result<PositionData>;

    /// Batch fetch positions for multiple users.
    ///
    /// Returns results for each user, allowing partial failures.
    async fn get_positions_batch(
        &self,
        users: &[Address],
        concurrency: usize,
    ) -> Vec<(Address, Result<PositionData>)>;

    /// Calculate health factor from raw position data.
    ///
    /// This allows recalculating HF locally when prices change without
    /// making RPC calls.
    fn calculate_health_factor(
        &self,
        collaterals: &[CollateralPosition],
        debts: &[DebtPosition],
    ) -> f64;

    /// Get event signatures for log subscription.
    fn event_signatures(&self) -> ProtocolEventSignatures;

    /// Check if the protocol supports a given asset.
    async fn is_asset_supported(&self, asset: Address) -> Result<bool>;
}

/// Trait for protocols that support liquidation.
///
/// Extends [`LendingProtocol`] with liquidation-specific operations.
#[async_trait]
pub trait LiquidatableProtocol: LendingProtocol {
    /// Get the chain ID this protocol is deployed on.
    fn chain_id(&self) -> u64;

    /// Get the swap adapter type for this protocol's chain.
    ///
    /// This determines how swap data is encoded for the liquidation contract.
    fn swap_adapter(&self) -> SwapAdapter {
        SwapAdapter::for_chain(self.chain_id())
    }

    /// Get liquidation parameters for this protocol.
    fn liquidation_params(&self) -> LiquidationParams;

    /// Get close factor (portion of debt that can be liquidated).
    fn close_factor(&self) -> f64 {
        self.liquidation_params().close_factor
    }

    /// Get liquidation threshold (HF below which liquidation is allowed).
    fn liquidation_threshold(&self) -> f64 {
        self.liquidation_params().liquidation_threshold
    }

    /// Check if a position with the given HF is liquidatable.
    fn is_liquidatable(&self, health_factor: f64) -> bool {
        health_factor < self.liquidation_threshold()
    }

    /// Get liquidation bonus for an asset (in basis points).
    ///
    /// Returns the default bonus if asset-specific bonus is not available.
    async fn get_liquidation_bonus(&self, asset: Address) -> Result<u16>;

    /// Encode liquidation calldata for the protocol's liquidation function.
    ///
    /// This creates the ABI-encoded calldata to send to the protocol's
    /// pool contract or custom liquidator contract.
    fn encode_liquidation(&self, params: &LiquidationCallParams) -> Result<Bytes>;

    /// Get the target contract address for liquidation calls.
    ///
    /// This may be the pool address or a custom liquidator contract.
    fn liquidation_target(&self) -> Address;

    /// Calculate the maximum debt that can be covered in a single liquidation.
    ///
    /// Takes into account close factor and available collateral.
    fn max_liquidatable_debt(
        &self,
        debt: &DebtPosition,
        collateral: &CollateralPosition,
        liquidation_bonus_bps: u16,
    ) -> U256 {
        // Max debt = debt_balance * close_factor
        let close_factor_scaled = U256::from((self.close_factor() * 10000.0) as u64);
        let max_by_close_factor = debt.balance * close_factor_scaled / U256::from(10000);

        // Also limit by available collateral value
        let bonus_multiplier = 10000 + liquidation_bonus_bps as u64;
        let collateral_value_scaled = U256::from((collateral.value_usd * 1e8) as u128);
        let debt_price = debt.price;

        // max_debt = collateral_value / (1 + bonus) / debt_price
        let max_by_collateral = if !debt_price.is_zero() {
            collateral_value_scaled * U256::from(10000) / U256::from(bonus_multiplier) / debt_price
        } else {
            U256::MAX
        };

        max_by_close_factor.min(max_by_collateral)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_version_parsing() {
        assert_eq!(ProtocolVersion::from_str("aave-v3"), ProtocolVersion::AaveV3);
        assert_eq!(ProtocolVersion::from_str("AaveV3"), ProtocolVersion::AaveV3);
        assert_eq!(
            ProtocolVersion::from_str("compound-v3"),
            ProtocolVersion::CompoundV3
        );
        assert_eq!(ProtocolVersion::from_str("comet"), ProtocolVersion::CompoundV3);
        assert_eq!(ProtocolVersion::from_str("unknown"), ProtocolVersion::Custom);
    }

    #[test]
    fn test_liquidation_params_default() {
        let params = LiquidationParams::default();
        assert_eq!(params.close_factor, 0.5);
        assert_eq!(params.liquidation_threshold, 1.0);
        assert_eq!(params.default_liquidation_bonus_bps, 500);
    }

    #[test]
    fn test_position_data_largest() {
        let mut position = PositionData {
            user: Address::ZERO,
            collaterals: SmallVec::new(),
            debts: SmallVec::new(),
            health_factor: 1.5,
            total_collateral_usd: 0.0,
            total_debt_usd: 0.0,
            timestamp: 0,
        };

        // Add collaterals
        position.collaterals.push(CollateralPosition {
            asset: Address::ZERO,
            balance: U256::ZERO,
            price: U256::ZERO,
            decimals: 18,
            value_usd: 100.0,
            liquidation_threshold_bps: 8000,
            enabled: true,
        });
        position.collaterals.push(CollateralPosition {
            asset: Address::ZERO,
            balance: U256::ZERO,
            price: U256::ZERO,
            decimals: 18,
            value_usd: 200.0,
            liquidation_threshold_bps: 8000,
            enabled: true,
        });

        // Largest should be 200
        let largest = position.largest_collateral().unwrap();
        assert_eq!(largest.value_usd, 200.0);
    }
}
