//! Oracle configuration for TOML-based setup.

use super::OracleType;
use alloy::primitives::Address;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Oracle configuration from TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleConfig {
    /// Asset address this oracle prices
    pub asset: String,
    /// Oracle aggregator address
    pub oracle: String,
    /// Oracle type
    #[serde(default)]
    pub oracle_type: OracleTypeConfig,
    /// Price decimals
    #[serde(default = "default_decimals")]
    pub decimals: u8,
    /// Heartbeat in seconds
    #[serde(default)]
    pub heartbeat_secs: Option<u64>,
    /// Human-readable description
    #[serde(default)]
    pub description: Option<String>,
}

fn default_decimals() -> u8 {
    8
}

/// Oracle type configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OracleTypeConfig {
    #[default]
    #[serde(rename = "chainlink")]
    Chainlink,
    #[serde(rename = "redstone")]
    RedStone,
    #[serde(rename = "pyth")]
    Pyth,
    #[serde(rename = "dual")]
    DualOracle,
    #[serde(rename = "custom")]
    Custom,
}

impl From<OracleTypeConfig> for OracleType {
    fn from(config: OracleTypeConfig) -> Self {
        match config {
            OracleTypeConfig::Chainlink => OracleType::Chainlink,
            OracleTypeConfig::RedStone => OracleType::RedStone,
            OracleTypeConfig::Pyth => OracleType::Pyth,
            OracleTypeConfig::DualOracle => OracleType::DualOracle,
            OracleTypeConfig::Custom => OracleType::Custom,
        }
    }
}

impl OracleConfig {
    /// Parse asset address.
    pub fn asset_address(&self) -> Result<Address> {
        self.asset.parse()
            .map_err(|e| anyhow::anyhow!("Invalid asset address '{}': {}", self.asset, e))
    }

    /// Parse oracle address.
    pub fn oracle_address(&self) -> Result<Address> {
        self.oracle.parse()
            .map_err(|e| anyhow::anyhow!("Invalid oracle address '{}': {}", self.oracle, e))
    }

    /// Get oracle type.
    pub fn oracle_type(&self) -> OracleType {
        self.oracle_type.into()
    }

    /// Get heartbeat duration.
    pub fn heartbeat(&self) -> Option<std::time::Duration> {
        self.heartbeat_secs.map(std::time::Duration::from_secs)
    }
}

/// Collection of oracle configurations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OraclesConfig {
    /// List of oracle configurations
    #[serde(default)]
    pub oracles: Vec<OracleConfig>,
}

impl OraclesConfig {
    /// Load from TOML content.
    pub fn from_toml(content: &str) -> Result<Self> {
        toml::from_str(content)
            .map_err(|e| anyhow::anyhow!("Failed to parse oracle config: {}", e))
    }

    /// Load from file.
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())?;
        Self::from_toml(&content)
    }

    /// Get oracle config for an asset.
    pub fn get_for_asset(&self, asset: &str) -> Option<&OracleConfig> {
        self.oracles.iter().find(|o| o.asset == asset)
    }
}

/// Oracle factory for creating oracles from configuration.
pub struct OracleFactory;

impl OracleFactory {
    /// Create oracle addresses and asset mappings from config.
    pub fn parse_configs(configs: &[OracleConfig]) -> Result<Vec<(Address, Address, OracleType)>> {
        let mut result = Vec::with_capacity(configs.len());

        for config in configs {
            let oracle = config.oracle_address()?;
            let asset = config.asset_address()?;
            let oracle_type = config.oracle_type();

            result.push((oracle, asset, oracle_type));
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oracle_config_parsing() {
        let toml = r#"
[[oracles]]
asset = "0x0000000000000000000000000000000000000001"
oracle = "0x0000000000000000000000000000000000000002"
oracle_type = "chainlink"
decimals = 8
heartbeat_secs = 3600
description = "ETH/USD"

[[oracles]]
asset = "0x0000000000000000000000000000000000000003"
oracle = "0x0000000000000000000000000000000000000004"
oracle_type = "redstone"
"#;

        let config: OraclesConfig = OraclesConfig::from_toml(toml).unwrap();
        assert_eq!(config.oracles.len(), 2);

        let first = &config.oracles[0];
        assert_eq!(first.decimals, 8);
        assert_eq!(first.heartbeat_secs, Some(3600));
        assert_eq!(first.oracle_type, OracleTypeConfig::Chainlink);

        let second = &config.oracles[1];
        assert_eq!(second.decimals, 8); // default
        assert_eq!(second.oracle_type, OracleTypeConfig::RedStone);
    }

    #[test]
    fn test_oracle_type_conversion() {
        assert_eq!(OracleType::from(OracleTypeConfig::Chainlink), OracleType::Chainlink);
        assert_eq!(OracleType::from(OracleTypeConfig::RedStone), OracleType::RedStone);
        assert_eq!(OracleType::from(OracleTypeConfig::Pyth), OracleType::Pyth);
    }
}
