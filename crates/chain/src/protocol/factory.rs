//! Protocol factory for creating protocol instances from configuration.
//!
//! This module provides a factory that creates [`LendingProtocol`] and
//! [`LiquidatableProtocol`] implementations based on configuration.

use super::{AaveV3Config, AaveV3Protocol, AssetConfig, LiquidatableProtocol};
use crate::contracts::SwapAdapter;
use crate::provider::ProviderManager;
use crate::signer::TransactionSender;
use alloy::primitives::Address;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;

/// Protocol factory for creating protocol instances from configuration.
///
/// The factory centralizes protocol creation and ensures consistent
/// configuration handling across the application.
#[derive(Debug, Default)]
pub struct ProtocolFactory {
    /// Default swap adapter override (if set)
    default_swap_adapter: Option<SwapAdapter>,
}

impl ProtocolFactory {
    /// Create a new protocol factory.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the default swap adapter for all protocols.
    pub fn with_default_swap_adapter(mut self, adapter: SwapAdapter) -> Self {
        self.default_swap_adapter = Some(adapter);
        self
    }

    /// Create an AAVE V3 protocol instance from configuration.
    ///
    /// # Arguments
    ///
    /// * `protocol_id` - Unique identifier for this protocol instance
    /// * `chain_id` - Chain ID where the protocol is deployed
    /// * `pool_address` - Pool contract address
    /// * `balances_reader_address` - Balances reader contract address
    /// * `oracle_address` - Optional oracle contract address
    /// * `liquidator_address` - Liquidator contract address
    /// * `provider` - Provider manager for RPC calls
    pub fn create_aave_v3(
        &self,
        protocol_id: impl Into<String>,
        chain_id: u64,
        pool_address: Address,
        balances_reader_address: Address,
        oracle_address: Option<Address>,
        liquidator_address: Address,
        provider: Arc<ProviderManager>,
    ) -> AaveV3Protocol {
        let config = AaveV3Config {
            protocol_id: protocol_id.into(),
            chain_id,
            pool_address,
            balances_reader_address,
            oracle_address,
            liquidator_address,
            close_factor: 0.5,
            default_liquidation_bonus_bps: 500,
            assets: HashMap::new(),
        };

        AaveV3Protocol::new(config, provider)
    }

    /// Create an AAVE V3 protocol from a config struct.
    pub fn create_aave_v3_from_config(
        &self,
        config: AaveV3Config,
        provider: Arc<ProviderManager>,
    ) -> AaveV3Protocol {
        AaveV3Protocol::new(config, provider)
    }

    /// Create an AAVE V3 protocol with transaction sender.
    pub fn create_aave_v3_with_sender(
        &self,
        config: AaveV3Config,
        provider: Arc<ProviderManager>,
        sender: Arc<TransactionSender>,
    ) -> AaveV3Protocol {
        AaveV3Protocol::with_sender(config, provider, sender)
    }

    /// Get swap adapter for a chain, using config override if set.
    pub fn swap_adapter_for_chain(&self, chain_id: u64) -> SwapAdapter {
        self.default_swap_adapter
            .unwrap_or_else(|| SwapAdapter::for_chain(chain_id))
    }
}

/// Builder for creating AAVE V3 configuration.
#[derive(Debug, Default)]
pub struct AaveV3ConfigBuilder {
    protocol_id: String,
    chain_id: u64,
    pool_address: Address,
    balances_reader_address: Address,
    oracle_address: Option<Address>,
    liquidator_address: Address,
    close_factor: f64,
    default_liquidation_bonus_bps: u16,
    assets: HashMap<Address, AssetConfig>,
}

impl AaveV3ConfigBuilder {
    /// Create a new builder.
    pub fn new(protocol_id: impl Into<String>) -> Self {
        Self {
            protocol_id: protocol_id.into(),
            close_factor: 0.5,
            default_liquidation_bonus_bps: 500,
            ..Default::default()
        }
    }

    /// Set chain ID.
    pub fn chain_id(mut self, chain_id: u64) -> Self {
        self.chain_id = chain_id;
        self
    }

    /// Set pool address.
    pub fn pool_address(mut self, address: Address) -> Self {
        self.pool_address = address;
        self
    }

    /// Set balances reader address.
    pub fn balances_reader_address(mut self, address: Address) -> Self {
        self.balances_reader_address = address;
        self
    }

    /// Set oracle address.
    pub fn oracle_address(mut self, address: Address) -> Self {
        self.oracle_address = Some(address);
        self
    }

    /// Set liquidator address.
    pub fn liquidator_address(mut self, address: Address) -> Self {
        self.liquidator_address = address;
        self
    }

    /// Set close factor.
    pub fn close_factor(mut self, factor: f64) -> Self {
        self.close_factor = factor;
        self
    }

    /// Set default liquidation bonus.
    pub fn default_liquidation_bonus_bps(mut self, bps: u16) -> Self {
        self.default_liquidation_bonus_bps = bps;
        self
    }

    /// Add an asset configuration.
    pub fn add_asset(mut self, config: AssetConfig) -> Self {
        self.assets.insert(config.address, config);
        self
    }

    /// Add multiple asset configurations.
    pub fn add_assets(mut self, configs: impl IntoIterator<Item = AssetConfig>) -> Self {
        for config in configs {
            self.assets.insert(config.address, config);
        }
        self
    }

    /// Build the configuration.
    pub fn build(self) -> AaveV3Config {
        AaveV3Config {
            protocol_id: self.protocol_id,
            chain_id: self.chain_id,
            pool_address: self.pool_address,
            balances_reader_address: self.balances_reader_address,
            oracle_address: self.oracle_address,
            liquidator_address: self.liquidator_address,
            close_factor: self.close_factor,
            default_liquidation_bonus_bps: self.default_liquidation_bonus_bps,
            assets: self.assets,
        }
    }
}

/// Extension trait to add config-aware swap adapter to protocols.
pub trait ProtocolSwapConfig {
    /// Get the configured swap adapter for this protocol.
    fn configured_swap_adapter(&self, swap_adapter_id: Option<u8>) -> SwapAdapter;
}

impl<T: LiquidatableProtocol + ?Sized> ProtocolSwapConfig for T {
    fn configured_swap_adapter(&self, swap_adapter_id: Option<u8>) -> SwapAdapter {
        if let Some(id) = swap_adapter_id {
            SwapAdapter::from_id(id).unwrap_or_else(|| SwapAdapter::for_chain(self.chain_id()))
        } else {
            SwapAdapter::for_chain(self.chain_id())
        }
    }
}

/// Parse address from string, returning error on failure.
pub fn parse_address(s: &str) -> Result<Address> {
    s.parse()
        .map_err(|e| anyhow::anyhow!("Invalid address '{}': {}", s, e))
}

/// Configuration-based protocol creation.
///
/// This struct wraps protocol configuration from TOML files and provides
/// methods to create protocol instances.
#[derive(Debug, Clone)]
pub struct ProtocolConfig {
    /// Protocol identifier
    pub id: String,
    /// Protocol version string (e.g., "aave-v3")
    pub version: String,
    /// Chain ID
    pub chain_id: u64,
    /// Pool contract address
    pub pool_address: String,
    /// Balances reader address
    pub balances_reader: Option<String>,
    /// Oracle address
    pub oracle: Option<String>,
    /// Liquidator contract address
    pub liquidator: Option<String>,
    /// Close factor
    pub close_factor: f64,
    /// Default liquidation bonus (bps)
    pub default_liquidation_bonus_bps: u16,
    /// Swap adapter override
    pub swap_adapter_id: Option<u8>,
}

impl ProtocolConfig {
    /// Create an AAVE V3 config from this protocol config.
    pub fn to_aave_v3_config(&self) -> Result<AaveV3Config> {
        Ok(AaveV3Config {
            protocol_id: self.id.clone(),
            chain_id: self.chain_id,
            pool_address: parse_address(&self.pool_address)?,
            balances_reader_address: self
                .balances_reader
                .as_ref()
                .map(|s| parse_address(s))
                .transpose()?
                .unwrap_or(Address::ZERO),
            oracle_address: self
                .oracle
                .as_ref()
                .map(|s| parse_address(s))
                .transpose()?,
            liquidator_address: self
                .liquidator
                .as_ref()
                .map(|s| parse_address(s))
                .transpose()?
                .unwrap_or(Address::ZERO),
            close_factor: self.close_factor,
            default_liquidation_bonus_bps: self.default_liquidation_bonus_bps,
            assets: HashMap::new(),
        })
    }

    /// Get the swap adapter for this protocol.
    pub fn swap_adapter(&self) -> SwapAdapter {
        if let Some(id) = self.swap_adapter_id {
            SwapAdapter::from_id(id).unwrap_or_else(|| SwapAdapter::for_chain(self.chain_id))
        } else {
            SwapAdapter::for_chain(self.chain_id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config = AaveV3ConfigBuilder::new("test-protocol")
            .chain_id(1)
            .pool_address(Address::ZERO)
            .balances_reader_address(Address::ZERO)
            .liquidator_address(Address::ZERO)
            .close_factor(0.5)
            .default_liquidation_bonus_bps(500)
            .build();

        assert_eq!(config.protocol_id, "test-protocol");
        assert_eq!(config.chain_id, 1);
        assert_eq!(config.close_factor, 0.5);
    }

    #[test]
    fn test_factory_swap_adapter() {
        let factory = ProtocolFactory::new();

        // Default: use chain-based selection
        assert_eq!(factory.swap_adapter_for_chain(999), SwapAdapter::LiquidSwap);
        assert_eq!(factory.swap_adapter_for_chain(42161), SwapAdapter::UniswapV3);

        // With override
        let factory = ProtocolFactory::new()
            .with_default_swap_adapter(SwapAdapter::Direct);
        assert_eq!(factory.swap_adapter_for_chain(999), SwapAdapter::Direct);
    }

    #[test]
    fn test_parse_address() {
        let addr = parse_address("0x0000000000000000000000000000000000000000").unwrap();
        assert_eq!(addr, Address::ZERO);

        let err = parse_address("invalid");
        assert!(err.is_err());
    }
}
