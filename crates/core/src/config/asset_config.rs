//! Asset configuration loading from TOML files.

use alloy::primitives::Address;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

use crate::assets::OracleType;

/// Asset configuration file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetsConfig {
    /// List of assets
    pub assets: Vec<AssetConfig>,
}

/// Individual asset configuration (TOML-loadable).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetConfig {
    /// Asset symbol (e.g., "wHYPE", "USDC")
    pub symbol: String,
    /// Token contract address (as hex string)
    pub token: String,
    /// Oracle aggregator address (as hex string)
    pub oracle: String,
    /// Oracle type
    pub oracle_type: String,
    /// Token decimals
    pub decimals: u8,
    /// Expected staleness threshold in seconds
    pub staleness_secs: u64,
    /// Liquidation priority (higher = prefer as collateral to seize)
    pub priority: u8,
    /// Liquidation bonus in basis points (e.g., 500 = 5%)
    pub liquidation_bonus_bps: u16,
    /// Whether this asset is active
    #[serde(default = "default_true")]
    pub active: bool,
    /// Maturity date for Pendle PT assets (Unix timestamp)
    #[serde(default)]
    pub maturity: Option<u64>,
}

fn default_true() -> bool {
    true
}

impl AssetConfig {
    /// Parse token address.
    pub fn token_address(&self) -> anyhow::Result<Address> {
        self.token
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid token address '{}': {}", self.token, e))
    }

    /// Parse oracle address.
    pub fn oracle_address(&self) -> anyhow::Result<Address> {
        self.oracle
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid oracle address '{}': {}", self.oracle, e))
    }

    /// Parse oracle type.
    pub fn oracle_type_enum(&self) -> anyhow::Result<OracleType> {
        match self.oracle_type.to_lowercase().as_str() {
            "standard" => Ok(OracleType::Standard),
            "redstone" => Ok(OracleType::RedStone),
            "pyth" => Ok(OracleType::Pyth),
            "dualoracle" | "dual_oracle" => Ok(OracleType::DualOracle),
            "pendlept" | "pendle_pt" => Ok(OracleType::PendlePT),
            _ => Err(anyhow::anyhow!("Unknown oracle type: {}", self.oracle_type)),
        }
    }

    /// Get staleness as Duration.
    pub fn staleness(&self) -> Duration {
        Duration::from_secs(self.staleness_secs)
    }

    /// Get liquidation bonus as decimal.
    pub fn liquidation_bonus(&self) -> f64 {
        self.liquidation_bonus_bps as f64 / 10000.0
    }
}

impl AssetsConfig {
    /// Load assets config from a TOML file.
    pub fn from_file(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())?;
        let config: AssetsConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Get active assets only.
    pub fn active_assets(&self) -> impl Iterator<Item = &AssetConfig> {
        self.assets.iter().filter(|a| a.active)
    }

    /// Get assets by oracle type.
    pub fn assets_by_oracle_type(&self, oracle_type: &str) -> impl Iterator<Item = &AssetConfig> {
        let ot = oracle_type.to_lowercase();
        self.assets
            .iter()
            .filter(move |a| a.oracle_type.to_lowercase() == ot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_asset_config() {
        let toml_str = r#"
            [[assets]]
            symbol = "TEST"
            token = "0x1111111111111111111111111111111111111111"
            oracle = "0x2222222222222222222222222222222222222222"
            oracle_type = "Standard"
            decimals = 18
            staleness_secs = 3600
            priority = 50
            liquidation_bonus_bps = 500
        "#;

        let config: AssetsConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.assets.len(), 1);
        assert_eq!(config.assets[0].symbol, "TEST");
        assert!(config.assets[0].active);
        assert!(config.assets[0].token_address().is_ok());
        assert_eq!(
            config.assets[0].oracle_type_enum().unwrap(),
            OracleType::Standard
        );
    }

    #[test]
    fn test_oracle_type_parsing() {
        let config = AssetConfig {
            symbol: "TEST".to_string(),
            token: "0x1111111111111111111111111111111111111111".to_string(),
            oracle: "0x2222222222222222222222222222222222222222".to_string(),
            oracle_type: "DualOracle".to_string(),
            decimals: 18,
            staleness_secs: 3600,
            priority: 50,
            liquidation_bonus_bps: 500,
            active: true,
            maturity: None,
        };

        assert_eq!(config.oracle_type_enum().unwrap(), OracleType::DualOracle);
    }
}
