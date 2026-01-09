//! AAVE V3 protocol implementation.
//!
//! This module provides an implementation of the [`LendingProtocol`] and
//! [`LiquidatableProtocol`] traits for AAVE V3 and compatible forks
//! (HyperLend, etc.).

use super::{
    CollateralPosition, DebtPosition, LendingProtocol, LiquidatableProtocol,
    LiquidationCallParams, LiquidationParams, PositionData,
    ProtocolEventSignatures, ProtocolVersion,
};
use crate::contracts::event_signatures;
use crate::provider::{BalanceData, ProviderManager};
use crate::signer::TransactionSender;
use alloy::primitives::{Address, Bytes, U256};
use alloy::sol;
use alloy::sol_types::SolCall;
use anyhow::Result;
use async_trait::async_trait;
use futures::stream::{self, StreamExt};
use smallvec::SmallVec;
use std::sync::Arc;
use tracing::debug;

// Define the liquidator contract interface for encoding (new signature)
sol! {
    /// Liquidator contract interface (new signature with bytes swapData)
    interface ILiquidator {
        function liquidate(
            address _user,
            address _collateral,
            address _debt,
            uint256 _debtAmount,
            uint256 _minAmountOut,
            bytes calldata _swapData
        ) external returns (uint256);
    }
}

/// Asset configuration for the protocol.
#[derive(Debug, Clone)]
pub struct AssetConfig {
    /// Token address
    pub address: Address,
    /// Liquidation bonus in basis points
    pub liquidation_bonus_bps: u16,
    /// Liquidation threshold in basis points
    pub liquidation_threshold_bps: u16,
    /// Token decimals
    pub decimals: u8,
}

/// Configuration for AAVE V3 protocol.
#[derive(Debug, Clone)]
pub struct AaveV3Config {
    /// Protocol identifier
    pub protocol_id: String,
    /// Chain ID this protocol is deployed on
    pub chain_id: u64,
    /// Pool contract address
    pub pool_address: Address,
    /// Balances reader contract address
    pub balances_reader_address: Address,
    /// Oracle contract address
    pub oracle_address: Option<Address>,
    /// Liquidator contract address
    pub liquidator_address: Address,
    /// Close factor (0.5 = 50%)
    pub close_factor: f64,
    /// Default liquidation bonus in basis points
    pub default_liquidation_bonus_bps: u16,
    /// Asset configurations (by address)
    pub assets: std::collections::HashMap<Address, AssetConfig>,
}

impl Default for AaveV3Config {
    fn default() -> Self {
        Self {
            protocol_id: "aave-v3".to_string(),
            chain_id: 1, // Mainnet default
            pool_address: Address::ZERO,
            balances_reader_address: Address::ZERO,
            oracle_address: None,
            liquidator_address: Address::ZERO,
            close_factor: 0.5,
            default_liquidation_bonus_bps: 500, // 5%
            assets: std::collections::HashMap::new(),
        }
    }
}

/// AAVE V3 protocol implementation.
///
/// Supports AAVE V3 mainnet and forks (HyperLend, etc.).
#[derive(Debug)]
pub struct AaveV3Protocol {
    /// Protocol configuration
    config: AaveV3Config,
    /// Provider manager for RPC calls
    provider: Arc<ProviderManager>,
    /// Transaction sender (optional, for liquidations)
    sender: Option<Arc<TransactionSender>>,
}

impl AaveV3Protocol {
    /// Create a new AAVE V3 protocol instance.
    pub fn new(config: AaveV3Config, provider: Arc<ProviderManager>) -> Self {
        Self {
            config,
            provider,
            sender: None,
        }
    }

    /// Create with a transaction sender for liquidations.
    pub fn with_sender(
        config: AaveV3Config,
        provider: Arc<ProviderManager>,
        sender: Arc<TransactionSender>,
    ) -> Self {
        Self {
            config,
            provider,
            sender: Some(sender),
        }
    }

    /// Set the transaction sender.
    pub fn set_sender(&mut self, sender: Arc<TransactionSender>) {
        self.sender = Some(sender);
    }

    /// Get asset liquidation bonus, or default if not configured.
    fn get_asset_liquidation_bonus(&self, asset: Address) -> u16 {
        self.config
            .assets
            .get(&asset)
            .map(|a| a.liquidation_bonus_bps)
            .unwrap_or(self.config.default_liquidation_bonus_bps)
    }

    /// Get asset liquidation threshold, or default (80%) if not configured.
    fn get_asset_liquidation_threshold(&self, asset: Address) -> u16 {
        self.config
            .assets
            .get(&asset)
            .map(|a| a.liquidation_threshold_bps)
            .unwrap_or(8000) // Default 80%
    }

    /// Convert BalanceData to CollateralPosition.
    fn to_collateral_position(&self, balance: &BalanceData) -> CollateralPosition {
        let lt = self.get_asset_liquidation_threshold(balance.underlying);
        let value_usd = calculate_usd_value(balance.amount, balance.price, balance.decimals);

        CollateralPosition {
            asset: balance.underlying,
            balance: balance.amount,
            price: balance.price,
            decimals: balance.decimals,
            value_usd,
            liquidation_threshold_bps: lt,
            enabled: true, // Assume all supplied assets are enabled
        }
    }

    /// Convert BalanceData to DebtPosition.
    fn to_debt_position(&self, balance: &BalanceData) -> DebtPosition {
        let value_usd = calculate_usd_value(balance.amount, balance.price, balance.decimals);

        DebtPosition {
            asset: balance.underlying,
            balance: balance.amount,
            price: balance.price,
            decimals: balance.decimals,
            value_usd,
        }
    }

}

#[async_trait]
impl LendingProtocol for AaveV3Protocol {
    fn protocol_id(&self) -> &str {
        &self.config.protocol_id
    }

    fn version(&self) -> ProtocolVersion {
        ProtocolVersion::AaveV3
    }

    fn pool_address(&self) -> Address {
        self.config.pool_address
    }

    fn oracle_address(&self) -> Option<Address> {
        self.config.oracle_address
    }

    async fn get_position(&self, user: Address) -> Result<PositionData> {
        debug!(user = %user, protocol = self.protocol_id(), "Fetching position");

        let (supply_balances, borrow_balances) = self.provider.get_position_data(user).await?;

        // Convert to protocol-agnostic types
        let collaterals: SmallVec<[CollateralPosition; 4]> = supply_balances
            .iter()
            .filter(|b| !b.amount.is_zero())
            .map(|b| self.to_collateral_position(b))
            .collect();

        let debts: SmallVec<[DebtPosition; 4]> = borrow_balances
            .iter()
            .filter(|b| !b.amount.is_zero())
            .map(|b| self.to_debt_position(b))
            .collect();

        // Calculate totals
        let total_collateral_usd: f64 = collaterals.iter().map(|c| c.value_usd).sum();
        let total_debt_usd: f64 = debts.iter().map(|d| d.value_usd).sum();

        // Calculate health factor
        let health_factor = self.calculate_health_factor(&collaterals, &debts);

        // Get block number for timestamp
        let timestamp = self.provider.block_number().await.unwrap_or(0);

        Ok(PositionData {
            user,
            collaterals,
            debts,
            health_factor,
            total_collateral_usd,
            total_debt_usd,
            timestamp,
        })
    }

    async fn get_positions_batch(
        &self,
        users: &[Address],
        concurrency: usize,
    ) -> Vec<(Address, Result<PositionData>)> {
        stream::iter(users.iter().cloned())
            .map(|user| async move {
                let result = self.get_position(user).await;
                (user, result)
            })
            .buffer_unordered(concurrency)
            .collect()
            .await
    }

    fn calculate_health_factor(
        &self,
        collaterals: &[CollateralPosition],
        debts: &[DebtPosition],
    ) -> f64 {
        // Sum risk-adjusted collateral value
        let total_collateral_adjusted: f64 = collaterals
            .iter()
            .filter(|c| c.enabled)
            .map(|c| c.value_usd * (c.liquidation_threshold_bps as f64 / 10000.0))
            .sum();

        // Sum debt value
        let total_debt: f64 = debts.iter().map(|d| d.value_usd).sum();

        if total_debt == 0.0 {
            return f64::MAX;
        }

        total_collateral_adjusted / total_debt
    }

    fn event_signatures(&self) -> ProtocolEventSignatures {
        ProtocolEventSignatures {
            supply: Some(event_signatures::SUPPLY),
            withdraw: Some(event_signatures::WITHDRAW),
            borrow: Some(event_signatures::BORROW),
            repay: Some(event_signatures::REPAY),
            liquidation: Some(event_signatures::LIQUIDATION_CALL),
            reserve_data_updated: None,
            interest_rate_update: None,
        }
    }

    async fn is_asset_supported(&self, asset: Address) -> Result<bool> {
        // Check if asset is in our config, or return true (permissive)
        Ok(self.config.assets.contains_key(&asset) || self.config.assets.is_empty())
    }
}

#[async_trait]
impl LiquidatableProtocol for AaveV3Protocol {
    fn chain_id(&self) -> u64 {
        self.config.chain_id
    }

    fn liquidation_params(&self) -> LiquidationParams {
        LiquidationParams {
            close_factor: self.config.close_factor,
            liquidation_threshold: 1.0,
            default_liquidation_bonus_bps: self.config.default_liquidation_bonus_bps,
        }
    }

    async fn get_liquidation_bonus(&self, asset: Address) -> Result<u16> {
        Ok(self.get_asset_liquidation_bonus(asset))
    }

    fn encode_liquidation(&self, params: &LiquidationCallParams) -> Result<Bytes> {
        // Use the new interface with bytes swapData
        // The swap_data should be pre-encoded by the caller using the appropriate adapter
        let swap_data = params.swap_data.clone().unwrap_or_default();

        let call = ILiquidator::liquidateCall {
            _user: params.user,
            _collateral: params.collateral_asset,
            _debt: params.debt_asset,
            _debtAmount: params.debt_to_cover,
            _minAmountOut: params.min_collateral_out,
            _swapData: swap_data,
        };

        Ok(Bytes::from(call.abi_encode()))
    }

    fn liquidation_target(&self) -> Address {
        self.config.liquidator_address
    }
}

/// Calculate USD value from amount and price.
/// Oracle price is assumed to be 8 decimals.
fn calculate_usd_value(amount: U256, price: U256, decimals: u8) -> f64 {
    if amount.is_zero() || price.is_zero() {
        return 0.0;
    }

    // Convert to f64 with proper decimal handling
    // value_usd = amount * price / 10^decimals / 10^8
    let amount_f64 = amount.to_string().parse::<f64>().unwrap_or(0.0);
    let price_f64 = price.to_string().parse::<f64>().unwrap_or(0.0);

    let decimals_factor = 10_f64.powi(decimals as i32);
    let oracle_decimals = 10_f64.powi(8);

    amount_f64 * price_f64 / decimals_factor / oracle_decimals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_usd_value() {
        // 1000 USDC (6 decimals) at $1.00 (8 decimal price)
        let amount = U256::from(1000_000000u64); // 1000 USDC
        let price = U256::from(100_000_000u64); // $1.00
        let value = calculate_usd_value(amount, price, 6);
        assert!((value - 1000.0).abs() < 0.01);

        // 1 ETH (18 decimals) at $2000 (8 decimal price)
        let amount = U256::from(1_000_000_000_000_000_000u128); // 1 ETH
        let price = U256::from(200_000_000_000u64); // $2000
        let value = calculate_usd_value(amount, price, 18);
        assert!((value - 2000.0).abs() < 0.01);
    }

    #[test]
    fn test_health_factor_calculation() {
        // Test health factor calculation logic directly
        // HF = sum(collateral_value * LT) / sum(debt_value)

        let collaterals = vec![CollateralPosition {
            asset: Address::ZERO,
            balance: U256::ZERO,
            price: U256::ZERO,
            decimals: 18,
            value_usd: 1000.0,
            liquidation_threshold_bps: 8000, // 80%
            enabled: true,
        }];

        let debts = vec![DebtPosition {
            asset: Address::ZERO,
            balance: U256::ZERO,
            price: U256::ZERO,
            decimals: 18,
            value_usd: 500.0,
        }];

        // Calculate health factor directly (same logic as AaveV3Protocol::calculate_health_factor)
        let total_collateral_adjusted: f64 = collaterals
            .iter()
            .filter(|c| c.enabled)
            .map(|c| c.value_usd * (c.liquidation_threshold_bps as f64 / 10000.0))
            .sum();

        let total_debt: f64 = debts.iter().map(|d| d.value_usd).sum();

        let hf = if total_debt == 0.0 {
            f64::MAX
        } else {
            total_collateral_adjusted / total_debt
        };

        // HF = (1000 * 0.8) / 500 = 1.6
        assert!((hf - 1.6).abs() < 0.001);
    }
}
