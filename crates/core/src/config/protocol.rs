//! Protocol configuration for multi-protocol support.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Protocol configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolConfig {
    /// Protocol details
    pub protocol: ProtocolDetails,
}

/// Protocol details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolDetails {
    /// Protocol identifier (e.g., "hyperlend", "aave-v3-ethereum")
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Protocol version (e.g., "aave-v3", "aave-v4", "compound-v3")
    pub version: String,
    /// Chain ID this protocol is deployed on
    pub chain_id: u64,
    /// Contract addresses
    pub contracts: ProtocolContracts,
    /// Protocol parameters
    pub parameters: ProtocolParameters,
    /// API endpoints (optional)
    #[serde(default)]
    pub api: Option<ProtocolApi>,
}

/// Protocol contract addresses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolContracts {
    /// Pool contract address
    pub pool: String,
    /// Balances reader contract address (if applicable)
    #[serde(default)]
    pub balances_reader: Option<String>,
    /// Oracle contract address (if applicable)
    #[serde(default)]
    pub oracle: Option<String>,
    /// Liquidator contract address (deployment-specific, often from env)
    #[serde(default)]
    pub liquidator: Option<String>,
}

/// Protocol parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolParameters {
    /// Close factor for liquidations (e.g., 0.5 for 50%)
    #[serde(default = "default_close_factor")]
    pub close_factor: f64,
    /// Default liquidation bonus in basis points
    #[serde(default = "default_liquidation_bonus")]
    pub default_liquidation_bonus_bps: u16,
    /// Health factor threshold for liquidation
    #[serde(default = "default_liquidation_threshold")]
    pub liquidation_threshold: f64,
}

fn default_close_factor() -> f64 {
    0.5
}

fn default_liquidation_bonus() -> u16 {
    500
}

fn default_liquidation_threshold() -> f64 {
    1.0
}

/// Protocol API endpoints.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProtocolApi {
    /// Position discovery API endpoint
    #[serde(default)]
    pub position_api: Option<String>,
    /// Swap routing API endpoint
    #[serde(default)]
    pub swap_api: Option<String>,
}

/// Protocol version enum for type-safe version handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProtocolVersion {
    /// AAVE V3 and forks
    AaveV3,
    /// AAVE V4 (upcoming)
    AaveV4,
    /// Compound V3
    CompoundV3,
    /// Custom/unknown protocol
    Custom,
}

impl ProtocolVersion {
    /// Parse protocol version from string.
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "aave-v3" | "aavev3" | "aave_v3" => Self::AaveV3,
            "aave-v4" | "aavev4" | "aave_v4" => Self::AaveV4,
            "compound-v3" | "compoundv3" | "compound_v3" => Self::CompoundV3,
            _ => Self::Custom,
        }
    }
}

impl ProtocolConfig {
    /// Load protocol config from a TOML file.
    pub fn from_file(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())?;
        let config: ProtocolConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Get protocol version enum.
    pub fn version(&self) -> ProtocolVersion {
        ProtocolVersion::from_str(&self.protocol.version)
    }
}
