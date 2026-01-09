//! Configuration management with profile support.
//!
//! Provides centralized configuration for all bot parameters with
//! support for different profiles (testing, production, aggressive).

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Main configuration structure containing all bot parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    /// Profile name (for logging/identification)
    #[serde(default = "default_profile_name")]
    pub profile: String,

    /// Position filtering thresholds
    #[serde(default)]
    pub position: PositionConfig,

    /// Position tier classification thresholds
    #[serde(default)]
    pub tiers: TierConfig,

    /// Scanner/orchestration timing
    #[serde(default)]
    pub scanner: ScannerTimingConfig,

    /// Pre-staging configuration
    #[serde(default)]
    pub pre_staging: PreStagingConfigValues,

    /// Liquidation execution parameters
    #[serde(default)]
    pub liquidation: LiquidationConfig,
}

fn default_profile_name() -> String {
    "default".to_string()
}

/// Position filtering and classification thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionConfig {
    /// Minimum collateral USD to consider (filter dust positions)
    #[serde(default = "default_dust_threshold")]
    pub dust_threshold_usd: f64,

    /// Health factor below which position is considered bad debt
    #[serde(default = "default_bad_debt_hf")]
    pub bad_debt_hf_threshold: f64,

    /// Maximum HF for initial seeding from BlockAnalitica
    #[serde(default = "default_seed_hf_max")]
    pub seed_hf_max: f64,

    /// Maximum number of wallets to seed
    #[serde(default = "default_seed_limit")]
    pub seed_limit: usize,
}

fn default_dust_threshold() -> f64 {
    0.10
}
fn default_bad_debt_hf() -> f64 {
    0.01
}
fn default_seed_hf_max() -> f64 {
    1.25
}
fn default_seed_limit() -> usize {
    100
}

impl Default for PositionConfig {
    fn default() -> Self {
        Self {
            dust_threshold_usd: default_dust_threshold(),
            bad_debt_hf_threshold: default_bad_debt_hf(),
            seed_hf_max: default_seed_hf_max(),
            seed_limit: default_seed_limit(),
        }
    }
}

/// Position tier classification thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierConfig {
    // Health factor thresholds
    /// HF threshold for Critical tier (below this = Critical)
    #[serde(default = "default_critical_hf")]
    pub critical_hf_threshold: f64,

    /// HF threshold for Hot tier (Critical < HF < this = Hot)
    #[serde(default = "default_hot_hf")]
    pub hot_hf_threshold: f64,

    /// HF threshold for Warm tier (Hot < HF < this = Warm, above = Cold)
    #[serde(default = "default_warm_hf")]
    pub warm_hf_threshold: f64,

    // Trigger distance thresholds (percentage)
    /// Trigger distance for Critical tier (below this % = Critical)
    #[serde(default = "default_critical_trigger")]
    pub critical_trigger_distance_pct: f64,

    /// Trigger distance for Hot tier
    #[serde(default = "default_hot_trigger")]
    pub hot_trigger_distance_pct: f64,

    /// Trigger distance for Warm tier
    #[serde(default = "default_warm_trigger")]
    pub warm_trigger_distance_pct: f64,
}

fn default_critical_hf() -> f64 {
    1.02
}
fn default_hot_hf() -> f64 {
    1.08
}
fn default_warm_hf() -> f64 {
    1.15
}
fn default_critical_trigger() -> f64 {
    1.0
}
fn default_hot_trigger() -> f64 {
    3.0
}
fn default_warm_trigger() -> f64 {
    7.0
}

impl Default for TierConfig {
    fn default() -> Self {
        Self {
            critical_hf_threshold: default_critical_hf(),
            hot_hf_threshold: default_hot_hf(),
            warm_hf_threshold: default_warm_hf(),
            critical_trigger_distance_pct: default_critical_trigger(),
            hot_trigger_distance_pct: default_hot_trigger(),
            warm_trigger_distance_pct: default_warm_trigger(),
        }
    }
}

/// Scanner timing configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannerTimingConfig {
    /// Bootstrap/resync interval (seconds)
    #[serde(default = "default_bootstrap_interval")]
    pub bootstrap_interval_secs: u64,

    /// Critical tier update interval (milliseconds)
    #[serde(default = "default_critical_interval")]
    pub critical_interval_ms: u64,

    /// Hot tier update interval (milliseconds)
    #[serde(default = "default_hot_interval")]
    pub hot_interval_ms: u64,

    /// Warm tier update interval (seconds)
    #[serde(default = "default_warm_interval")]
    pub warm_interval_secs: u64,

    /// Cold tier update interval (seconds)
    #[serde(default = "default_cold_interval")]
    pub cold_interval_secs: u64,

    /// DualOracle check interval (seconds)
    #[serde(default = "default_dual_oracle_interval")]
    pub dual_oracle_interval_secs: u64,

    /// Heartbeat prediction interval (seconds)
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_secs: u64,
}

fn default_bootstrap_interval() -> u64 {
    60
}
fn default_critical_interval() -> u64 {
    100
}
fn default_hot_interval() -> u64 {
    500
}
fn default_warm_interval() -> u64 {
    2
}
fn default_cold_interval() -> u64 {
    10
}
fn default_dual_oracle_interval() -> u64 {
    5
}
fn default_heartbeat_interval() -> u64 {
    1
}

impl Default for ScannerTimingConfig {
    fn default() -> Self {
        Self {
            bootstrap_interval_secs: default_bootstrap_interval(),
            critical_interval_ms: default_critical_interval(),
            hot_interval_ms: default_hot_interval(),
            warm_interval_secs: default_warm_interval(),
            cold_interval_secs: default_cold_interval(),
            dual_oracle_interval_secs: default_dual_oracle_interval(),
            heartbeat_interval_secs: default_heartbeat_interval(),
        }
    }
}

impl ScannerTimingConfig {
    pub fn bootstrap_interval(&self) -> Duration {
        Duration::from_secs(self.bootstrap_interval_secs)
    }
    pub fn critical_interval(&self) -> Duration {
        Duration::from_millis(self.critical_interval_ms)
    }
    pub fn hot_interval(&self) -> Duration {
        Duration::from_millis(self.hot_interval_ms)
    }
    pub fn warm_interval(&self) -> Duration {
        Duration::from_secs(self.warm_interval_secs)
    }
    pub fn cold_interval(&self) -> Duration {
        Duration::from_secs(self.cold_interval_secs)
    }
    pub fn dual_oracle_interval(&self) -> Duration {
        Duration::from_secs(self.dual_oracle_interval_secs)
    }
    pub fn heartbeat_interval(&self) -> Duration {
        Duration::from_secs(self.heartbeat_interval_secs)
    }
}

/// Pre-staging configuration values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreStagingConfigValues {
    /// HF threshold to start pre-staging
    #[serde(default = "default_staging_hf")]
    pub staging_hf_threshold: f64,

    /// TTL for staged transactions (seconds)
    #[serde(default = "default_staged_ttl")]
    pub staged_tx_ttl_secs: u64,

    /// Price deviation threshold for invalidation (percentage)
    #[serde(default = "default_price_deviation")]
    pub price_deviation_threshold_pct: f64,

    /// Minimum debt USD value to stage
    #[serde(default = "default_min_debt_to_stage")]
    pub min_debt_usd_to_stage: f64,
}

fn default_staging_hf() -> f64 {
    1.05
}
fn default_staged_ttl() -> u64 {
    15
}
fn default_price_deviation() -> f64 {
    0.5
}
fn default_min_debt_to_stage() -> f64 {
    0.0001
}

impl Default for PreStagingConfigValues {
    fn default() -> Self {
        Self {
            staging_hf_threshold: default_staging_hf(),
            staged_tx_ttl_secs: default_staged_ttl(),
            price_deviation_threshold_pct: default_price_deviation(),
            min_debt_usd_to_stage: default_min_debt_to_stage(),
        }
    }
}

impl PreStagingConfigValues {
    pub fn staged_tx_ttl(&self) -> Duration {
        Duration::from_secs(self.staged_tx_ttl_secs)
    }
}

/// Liquidation execution parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidationConfig {
    /// Close factor (fraction of position to liquidate)
    #[serde(default = "default_close_factor")]
    pub close_factor: f64,

    /// Minimum profit USD to execute liquidation
    #[serde(default = "default_min_profit")]
    pub min_profit_usd: f64,

    /// Maximum slippage tolerance (percentage)
    #[serde(default = "default_max_slippage")]
    pub max_slippage_pct: f64,

    /// Whether to use multi-hop swap routing
    #[serde(default = "default_multi_hop")]
    pub use_multi_hop: bool,

    /// Gas price multiplier for priority
    #[serde(default = "default_gas_multiplier")]
    pub gas_price_multiplier: f64,
}

fn default_close_factor() -> f64 {
    0.5
}
fn default_min_profit() -> f64 {
    0.0
}
fn default_max_slippage() -> f64 {
    1.0
}
fn default_multi_hop() -> bool {
    true
}
fn default_gas_multiplier() -> f64 {
    1.0
}

impl Default for LiquidationConfig {
    fn default() -> Self {
        Self {
            close_factor: default_close_factor(),
            min_profit_usd: default_min_profit(),
            max_slippage_pct: default_max_slippage(),
            use_multi_hop: default_multi_hop(),
            gas_price_multiplier: default_gas_multiplier(),
        }
    }
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            profile: default_profile_name(),
            position: PositionConfig::default(),
            tiers: TierConfig::default(),
            scanner: ScannerTimingConfig::default(),
            pre_staging: PreStagingConfigValues::default(),
            liquidation: LiquidationConfig::default(),
        }
    }
}

impl BotConfig {
    /// Load configuration from a TOML file.
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    /// Create a testing profile optimized for dust positions.
    pub fn testing() -> Self {
        Self {
            profile: "testing".to_string(),
            position: PositionConfig {
                dust_threshold_usd: 0.0001,    // $0.0001 - allow tiny positions
                bad_debt_hf_threshold: 0.0001, // Very low - only filter truly dead positions
                seed_hf_max: 1.5,              // Wider range
                seed_limit: 500,               // More positions
            },
            tiers: TierConfig {
                critical_hf_threshold: 1.05,
                hot_hf_threshold: 1.15,
                warm_hf_threshold: 1.25,
                critical_trigger_distance_pct: 2.0,
                hot_trigger_distance_pct: 5.0,
                warm_trigger_distance_pct: 10.0,
            },
            scanner: ScannerTimingConfig {
                bootstrap_interval_secs: 30, // Faster resync
                critical_interval_ms: 200,
                hot_interval_ms: 1000,
                warm_interval_secs: 5,
                cold_interval_secs: 30,
                dual_oracle_interval_secs: 10,
                heartbeat_interval_secs: 2,
            },
            pre_staging: PreStagingConfigValues {
                staging_hf_threshold: 1.10,
                staged_tx_ttl_secs: 30,
                price_deviation_threshold_pct: 2.0,
                min_debt_usd_to_stage: 0.0001, // Allow dust
            },
            liquidation: LiquidationConfig {
                close_factor: 0.5,
                min_profit_usd: 0.0, // No minimum profit for testing
                max_slippage_pct: 5.0,
                use_multi_hop: true,
                gas_price_multiplier: 1.0,
            },
        }
    }

    /// Create a production profile with conservative settings.
    pub fn production() -> Self {
        Self {
            profile: "production".to_string(),
            position: PositionConfig {
                dust_threshold_usd: 10.0,  // $10 minimum
                bad_debt_hf_threshold: 0.1,
                seed_hf_max: 1.15,
                seed_limit: 200,
            },
            tiers: TierConfig::default(),
            scanner: ScannerTimingConfig::default(),
            pre_staging: PreStagingConfigValues {
                staging_hf_threshold: 1.05,
                staged_tx_ttl_secs: 15,
                price_deviation_threshold_pct: 0.5,
                min_debt_usd_to_stage: 10.0,
            },
            liquidation: LiquidationConfig {
                close_factor: 0.5,
                min_profit_usd: 1.0, // $1 minimum profit
                max_slippage_pct: 0.5,
                use_multi_hop: true,
                gas_price_multiplier: 1.1,
            },
        }
    }

    /// Create an aggressive profile for maximum speed.
    pub fn aggressive() -> Self {
        Self {
            profile: "aggressive".to_string(),
            position: PositionConfig {
                dust_threshold_usd: 1.0,   // $1 minimum
                bad_debt_hf_threshold: 0.05,
                seed_hf_max: 1.20,
                seed_limit: 300,
            },
            tiers: TierConfig {
                critical_hf_threshold: 1.03,
                hot_hf_threshold: 1.10,
                warm_hf_threshold: 1.20,
                ..Default::default()
            },
            scanner: ScannerTimingConfig {
                bootstrap_interval_secs: 30,
                critical_interval_ms: 50,  // Faster critical updates
                hot_interval_ms: 250,
                warm_interval_secs: 1,
                cold_interval_secs: 5,
                dual_oracle_interval_secs: 2,
                heartbeat_interval_secs: 1,
            },
            pre_staging: PreStagingConfigValues {
                staging_hf_threshold: 1.08,
                staged_tx_ttl_secs: 10,
                price_deviation_threshold_pct: 0.3,
                min_debt_usd_to_stage: 1.0,
            },
            liquidation: LiquidationConfig {
                close_factor: 0.5,
                min_profit_usd: 0.5,
                max_slippage_pct: 1.0,
                use_multi_hop: true,
                gas_price_multiplier: 1.2, // Higher gas for priority
            },
        }
    }

    /// Get profile from environment variable BOT_PROFILE, or default.
    /// Supported values: testing, production, aggressive
    pub fn from_env() -> Self {
        let profile = std::env::var("BOT_PROFILE").unwrap_or_else(|_| "default".to_string());
        match profile.to_lowercase().as_str() {
            "testing" | "test" => Self::testing(),
            "production" | "prod" => Self::production(),
            "aggressive" | "aggro" => Self::aggressive(),
            _ => Self::default(),
        }
    }

    /// Log the current configuration.
    pub fn log_config(&self) {
        tracing::info!(profile = %self.profile, "Bot configuration loaded");
        tracing::info!(
            dust_threshold = self.position.dust_threshold_usd,
            bad_debt_hf = self.position.bad_debt_hf_threshold,
            seed_hf_max = self.position.seed_hf_max,
            seed_limit = self.position.seed_limit,
            "Position thresholds"
        );
        tracing::info!(
            critical_hf = self.tiers.critical_hf_threshold,
            hot_hf = self.tiers.hot_hf_threshold,
            warm_hf = self.tiers.warm_hf_threshold,
            "Tier HF thresholds"
        );
        tracing::info!(
            staging_hf = self.pre_staging.staging_hf_threshold,
            min_debt = self.pre_staging.min_debt_usd_to_stage,
            "Pre-staging thresholds"
        );
        tracing::info!(
            close_factor = self.liquidation.close_factor,
            min_profit = self.liquidation.min_profit_usd,
            max_slippage = self.liquidation.max_slippage_pct,
            "Liquidation parameters"
        );
    }
}

/// Global configuration holder using lazy initialization.
use std::sync::OnceLock;

static GLOBAL_CONFIG: OnceLock<BotConfig> = OnceLock::new();

/// Initialize global configuration.
pub fn init_config(config: BotConfig) {
    let _ = GLOBAL_CONFIG.set(config);
}

/// Get the global configuration, initializing from environment if needed.
pub fn config() -> &'static BotConfig {
    GLOBAL_CONFIG.get_or_init(BotConfig::from_env)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = BotConfig::default();
        assert_eq!(config.position.dust_threshold_usd, 0.10);
        assert_eq!(config.tiers.critical_hf_threshold, 1.02);
    }

    #[test]
    fn test_testing_profile() {
        let config = BotConfig::testing();
        assert_eq!(config.profile, "testing");
        assert!(config.position.dust_threshold_usd < 0.01);
    }

    #[test]
    fn test_production_profile() {
        let config = BotConfig::production();
        assert_eq!(config.profile, "production");
        assert!(config.position.dust_threshold_usd >= 10.0);
    }

    #[test]
    fn test_serialization() {
        let config = BotConfig::testing();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("profile = \"testing\""));

        let parsed: BotConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.profile, "testing");
    }
}
