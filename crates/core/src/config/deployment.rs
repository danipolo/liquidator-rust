//! Deployment configuration that ties together chain, protocol, and assets.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Full deployment configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentConfig {
    /// Deployment metadata
    pub deployment: DeploymentDetails,
    /// Bot configuration overrides
    #[serde(default)]
    pub bot: Option<BotConfigOverrides>,
}

/// Deployment details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentDetails {
    /// Deployment name (e.g., "hyperlend-prod")
    pub name: String,
    /// Chain config file name (without extension)
    pub chain: String,
    /// Protocol config file name (without extension)
    pub protocol: String,
    /// Assets config file name (without extension)
    pub assets: String,
    /// Contract overrides for this deployment
    #[serde(default)]
    pub contracts: Option<DeploymentContracts>,
}

/// Deployment-specific contract overrides.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeploymentContracts {
    /// Liquidator contract address
    #[serde(default)]
    pub liquidator: Option<String>,
    /// Profit receiver address
    #[serde(default)]
    pub profit_receiver: Option<String>,
}

/// Bot configuration overrides.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BotConfigOverrides {
    /// Position tracking config
    #[serde(default)]
    pub position: Option<PositionOverrides>,
    /// Tier thresholds
    #[serde(default)]
    pub tiers: Option<TierOverrides>,
    /// Scanner intervals
    #[serde(default)]
    pub scanner: Option<ScannerOverrides>,
    /// Pre-staging config
    #[serde(default)]
    pub pre_staging: Option<PreStagingOverrides>,
    /// Liquidation config
    #[serde(default)]
    pub liquidation: Option<LiquidationOverrides>,
}

/// Position tracking overrides.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PositionOverrides {
    #[serde(default)]
    pub dust_threshold_usd: Option<f64>,
    #[serde(default)]
    pub bad_debt_hf_threshold: Option<f64>,
    #[serde(default)]
    pub seed_hf_max: Option<f64>,
    #[serde(default)]
    pub seed_limit: Option<usize>,
}

/// Tier threshold overrides.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TierOverrides {
    #[serde(default)]
    pub critical_hf_threshold: Option<f64>,
    #[serde(default)]
    pub hot_hf_threshold: Option<f64>,
    #[serde(default)]
    pub warm_hf_threshold: Option<f64>,
}

/// Scanner interval overrides.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScannerOverrides {
    #[serde(default)]
    pub bootstrap_interval_secs: Option<u64>,
    #[serde(default)]
    pub critical_interval_ms: Option<u64>,
    #[serde(default)]
    pub hot_interval_ms: Option<u64>,
    #[serde(default)]
    pub warm_interval_ms: Option<u64>,
    #[serde(default)]
    pub cold_interval_ms: Option<u64>,
    #[serde(default)]
    pub dual_oracle_interval_ms: Option<u64>,
    #[serde(default)]
    pub heartbeat_interval_ms: Option<u64>,
}

/// Pre-staging overrides.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PreStagingOverrides {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub hf_threshold: Option<f64>,
    #[serde(default)]
    pub price_deviation_bps: Option<u64>,
    #[serde(default)]
    pub ttl_secs: Option<u64>,
}

/// Liquidation overrides.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LiquidationOverrides {
    #[serde(default)]
    pub close_factor: Option<f64>,
    #[serde(default)]
    pub min_profit_usd: Option<f64>,
    #[serde(default)]
    pub max_slippage_pct: Option<f64>,
    #[serde(default)]
    pub gas_multiplier: Option<f64>,
}

impl DeploymentConfig {
    /// Load deployment config from a TOML file.
    pub fn from_file(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())?;
        let config: DeploymentConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Get the config directory path from the deployment file path.
    pub fn config_dir(deployment_path: impl AsRef<Path>) -> Option<std::path::PathBuf> {
        deployment_path
            .as_ref()
            .parent() // deployments/
            .and_then(|p| p.parent()) // config/
            .map(|p| p.to_path_buf())
    }
}
