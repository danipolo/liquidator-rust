//! Unified deployment loader that ties together all configuration.
//!
//! This module provides a single entry point for loading all configuration
//! needed to run the liquidation bot, replacing hardcoded values with
//! config-driven setup.

use super::{
    AssetsConfig, BotConfig, BotConfigOverrides, ChainConfig, DeploymentConfig, ProtocolConfig,
    ConfigRegistry,
};
use alloy::primitives::Address;
use anyhow::{Context, Result};
use std::path::Path;
use tracing::info;

/// Fully resolved deployment configuration.
///
/// This struct contains all the configuration needed to run the bot,
/// resolved from the deployment, chain, protocol, and assets configs.
#[derive(Debug, Clone)]
pub struct ResolvedDeployment {
    /// Deployment name
    pub name: String,
    /// Chain configuration
    pub chain: ChainDetails,
    /// Protocol configuration
    pub protocol: ProtocolDetails,
    /// Asset configurations
    pub assets: Vec<ResolvedAsset>,
    /// Bot configuration (with deployment overrides applied)
    pub bot: BotConfig,
    /// Contract addresses
    pub contracts: ResolvedContracts,
}

/// Resolved chain details.
#[derive(Debug, Clone)]
pub struct ChainDetails {
    /// Chain ID
    pub chain_id: u64,
    /// Chain name
    pub name: String,
    /// Native token symbol
    pub native_token: String,
    /// Block time in milliseconds
    pub block_time_ms: u64,
    /// RPC URLs
    pub rpc: RpcUrls,
    /// Gas configuration
    pub gas: GasDetails,
    /// Swap configuration
    pub swap_adapter: String,
}

/// RPC URLs with environment variable expansion.
#[derive(Debug, Clone)]
pub struct RpcUrls {
    pub http: String,
    pub ws: String,
    pub archive: String,
    pub send: String,
}

/// Gas configuration details.
#[derive(Debug, Clone)]
pub struct GasDetails {
    /// Pricing model ("Legacy" or "Eip1559")
    pub pricing: String,
    /// Gas limit multiplier
    pub limit_multiplier: f64,
    /// Maximum gas price in gwei
    pub max_gas_price_gwei: f64,
    /// Default gas price in gwei
    pub default_gas_price_gwei: f64,
    /// Priority fee in gwei (for EIP-1559)
    pub priority_fee_gwei: Option<f64>,
}

/// Resolved protocol details.
#[derive(Debug, Clone)]
pub struct ProtocolDetails {
    /// Protocol ID
    pub id: String,
    /// Protocol name
    pub name: String,
    /// Protocol version (e.g., "aave-v3")
    pub version: String,
    /// Close factor (0.0-1.0)
    pub close_factor: f64,
    /// Default liquidation bonus in basis points
    pub default_liquidation_bonus_bps: u16,
    /// Position discovery API URL
    pub position_api_url: Option<String>,
    /// Swap API URL
    pub swap_api_url: Option<String>,
}

/// Resolved contract addresses.
#[derive(Debug, Clone)]
pub struct ResolvedContracts {
    /// Pool contract address
    pub pool: Address,
    /// Balances reader contract address
    pub balances_reader: Address,
    /// Oracle contract address (optional)
    pub oracle: Option<Address>,
    /// Liquidator contract address
    pub liquidator: Address,
    /// Profit receiver address
    pub profit_receiver: Address,
}

/// Resolved asset configuration.
#[derive(Debug, Clone)]
pub struct ResolvedAsset {
    /// Asset symbol
    pub symbol: String,
    /// Token address
    pub token: Address,
    /// Oracle address
    pub oracle: Address,
    /// Oracle type string
    pub oracle_type: String,
    /// Token decimals
    pub decimals: u8,
    /// Staleness threshold in seconds
    pub staleness_secs: u64,
    /// Liquidation priority
    pub priority: u8,
    /// Liquidation bonus in basis points
    pub liquidation_bonus_bps: u16,
    /// Whether asset is active
    pub active: bool,
    /// Maturity timestamp (for Pendle PT assets)
    pub maturity: Option<u64>,
}

/// Deployment loader for unified configuration.
pub struct DeploymentLoader {
    /// Config registry
    registry: ConfigRegistry,
    /// Config directory path
    config_dir: std::path::PathBuf,
}

impl DeploymentLoader {
    /// Create a new deployment loader from a config directory.
    pub fn new(config_dir: impl AsRef<Path>) -> Result<Self> {
        let config_dir = config_dir.as_ref().to_path_buf();
        let registry = ConfigRegistry::load_from_dir(&config_dir)
            .context("Failed to load config registry")?;

        Ok(Self { registry, config_dir })
    }

    /// Load a deployment by name.
    ///
    /// This resolves all configuration from the deployment file and its
    /// referenced chain, protocol, and assets configs.
    pub fn load(&self, deployment_name: &str) -> Result<ResolvedDeployment> {
        info!(deployment = deployment_name, "Loading deployment configuration");

        // Get the full deployment with chain and protocol
        let (deployment, chain_config, protocol_config) = self
            .registry
            .get_full_deployment(deployment_name)
            .ok_or_else(|| anyhow::anyhow!("Deployment '{}' not found", deployment_name))?;

        // Load assets config
        let assets_path = self
            .config_dir
            .join("assets")
            .join(format!("{}.toml", deployment.deployment.assets));
        let assets_config = AssetsConfig::from_file(&assets_path)
            .with_context(|| format!("Failed to load assets from {:?}", assets_path))?;

        // Resolve chain details
        let chain = self.resolve_chain(chain_config)?;

        // Resolve protocol details
        let protocol = self.resolve_protocol(protocol_config);

        // Resolve assets
        let assets = self.resolve_assets(&assets_config)?;

        // Resolve contracts (with env var overrides)
        let contracts = self.resolve_contracts(protocol_config, deployment)?;

        // Build bot config with deployment overrides
        let bot = self.build_bot_config(deployment.bot.as_ref());

        Ok(ResolvedDeployment {
            name: deployment_name.to_string(),
            chain,
            protocol,
            assets,
            bot,
            contracts,
        })
    }

    /// Load deployment from environment variable DEPLOYMENT.
    pub fn load_from_env(&self) -> Result<ResolvedDeployment> {
        let deployment_name = std::env::var("DEPLOYMENT")
            .unwrap_or_else(|_| "hyperlend-prod".to_string());
        self.load(&deployment_name)
    }

    fn resolve_chain(&self, config: &ChainConfig) -> Result<ChainDetails> {
        let rpc = &config.chain.rpc;

        // Expand environment variables in RPC URLs
        let expand_env = |s: &str| -> String {
            if s.starts_with("${") && s.ends_with("}") {
                let var_name = &s[2..s.len()-1];
                std::env::var(var_name).unwrap_or_else(|_| s.to_string())
            } else {
                s.to_string()
            }
        };

        // Get pricing model as string
        let pricing = match config.chain.gas.pricing {
            super::GasPricingModel::Legacy => "Legacy".to_string(),
            super::GasPricingModel::Eip1559 => "Eip1559".to_string(),
            super::GasPricingModel::Custom => "Custom".to_string(),
        };

        // Get swap adapter from optional swap config
        let swap_adapter = config
            .chain
            .swap
            .as_ref()
            .map(|s| s.default_adapter.clone())
            .unwrap_or_else(|| "uniswap_v3".to_string());

        Ok(ChainDetails {
            chain_id: config.chain.chain_id,
            name: config.chain.name.clone(),
            native_token: config.chain.native_token.clone(),
            block_time_ms: config.chain.block_time_ms,
            rpc: RpcUrls {
                http: expand_env(&rpc.http),
                ws: expand_env(&rpc.ws),
                archive: rpc.archive.as_ref().map(|s| expand_env(s)).unwrap_or_else(|| expand_env(&rpc.http)),
                send: rpc.send.as_ref().map(|s| expand_env(s)).unwrap_or_else(|| expand_env(&rpc.http)),
            },
            gas: GasDetails {
                pricing,
                limit_multiplier: config.chain.gas.limit_multiplier,
                max_gas_price_gwei: config.chain.gas.max_gas_price_gwei,
                default_gas_price_gwei: config.chain.gas.default_gas_price_gwei,
                priority_fee_gwei: config.chain.gas.priority_fee_gwei,
            },
            swap_adapter,
        })
    }

    fn resolve_protocol(&self, config: &ProtocolConfig) -> ProtocolDetails {
        ProtocolDetails {
            id: config.protocol.id.clone(),
            name: config.protocol.name.clone(),
            version: config.protocol.version.clone(),
            close_factor: config.protocol.parameters.close_factor,
            default_liquidation_bonus_bps: config.protocol.parameters.default_liquidation_bonus_bps,
            position_api_url: config.protocol.api.as_ref().and_then(|a| a.position_api.clone()),
            swap_api_url: config.protocol.api.as_ref().and_then(|a| a.swap_api.clone()),
        }
    }

    fn resolve_assets(&self, config: &AssetsConfig) -> Result<Vec<ResolvedAsset>> {
        config
            .assets
            .iter()
            .map(|asset| {
                Ok(ResolvedAsset {
                    symbol: asset.symbol.clone(),
                    token: asset.token_address()?,
                    oracle: asset.oracle_address()?,
                    oracle_type: asset.oracle_type.clone(),
                    decimals: asset.decimals,
                    staleness_secs: asset.staleness_secs,
                    priority: asset.priority,
                    liquidation_bonus_bps: asset.liquidation_bonus_bps,
                    active: asset.active,
                    maturity: asset.maturity,
                })
            })
            .collect()
    }

    fn resolve_contracts(
        &self,
        protocol: &ProtocolConfig,
        deployment: &DeploymentConfig,
    ) -> Result<ResolvedContracts> {
        let contracts = &protocol.protocol.contracts;

        // Parse addresses with env var fallback
        let parse_addr = |s: &str, env_var: &str| -> Result<Address> {
            // Check if it's an env var reference
            if s.starts_with("${") && s.ends_with("}") {
                let var_name = &s[2..s.len()-1];
                let value = std::env::var(var_name)
                    .or_else(|_| std::env::var(env_var))
                    .map_err(|_| anyhow::anyhow!("Missing env var: {} or {}", var_name, env_var))?;
                value.parse().map_err(|e| anyhow::anyhow!("Invalid address: {}", e))
            } else {
                s.parse().map_err(|e| anyhow::anyhow!("Invalid address '{}': {}", s, e))
            }
        };

        // Get liquidator address from deployment override or protocol config
        let liquidator_str = deployment
            .deployment
            .contracts
            .as_ref()
            .and_then(|c| c.liquidator.as_ref())
            .or(contracts.liquidator.as_ref())
            .ok_or_else(|| anyhow::anyhow!("Liquidator address not configured"))?;

        // Get profit receiver from deployment or env
        let profit_receiver_str = deployment
            .deployment
            .contracts
            .as_ref()
            .and_then(|c| c.profit_receiver.as_ref())
            .map(|s| s.as_str())
            .unwrap_or("${PROFIT_RECEIVER}");

        // Get balances reader (optional in protocol config)
        let balances_reader = if let Some(br) = &contracts.balances_reader {
            parse_addr(br, "BALANCES_READER")?
        } else {
            // Try env var fallback
            let addr_str = std::env::var("BALANCES_READER")
                .map_err(|_| anyhow::anyhow!("Balances reader not configured and BALANCES_READER env var not set"))?;
            addr_str.parse().map_err(|e| anyhow::anyhow!("Invalid BALANCES_READER: {}", e))?
        };

        Ok(ResolvedContracts {
            pool: parse_addr(&contracts.pool, "POOL")?,
            balances_reader,
            oracle: contracts.oracle.as_ref().map(|s| parse_addr(s, "ORACLE")).transpose()?,
            liquidator: parse_addr(liquidator_str, "LIQUIDATOR")?,
            profit_receiver: parse_addr(profit_receiver_str, "PROFIT_RECEIVER")?,
        })
    }

    fn build_bot_config(&self, overrides: Option<&BotConfigOverrides>) -> BotConfig {
        // Start with base config from profile or default
        let mut config = if let Some(ovr) = overrides {
            if let Some(profile) = &ovr.profile {
                BotConfig::load_profile(profile).unwrap_or_default()
            } else {
                BotConfig::default()
            }
        } else {
            BotConfig::from_env()
        };

        // Apply overrides
        if let Some(ovr) = overrides {
            if let Some(pos) = &ovr.position {
                if let Some(v) = pos.dust_threshold_usd {
                    config.position.dust_threshold_usd = v;
                }
                if let Some(v) = pos.bad_debt_hf_threshold {
                    config.position.bad_debt_hf_threshold = v;
                }
                if let Some(v) = pos.seed_hf_max {
                    config.position.seed_hf_max = v;
                }
                if let Some(v) = pos.seed_limit {
                    config.position.seed_limit = v;
                }
            }

            if let Some(tiers) = &ovr.tiers {
                if let Some(v) = tiers.critical_hf_threshold {
                    config.tiers.critical_hf_threshold = v;
                }
                if let Some(v) = tiers.hot_hf_threshold {
                    config.tiers.hot_hf_threshold = v;
                }
                if let Some(v) = tiers.warm_hf_threshold {
                    config.tiers.warm_hf_threshold = v;
                }
            }

            if let Some(scanner) = &ovr.scanner {
                if let Some(v) = scanner.bootstrap_interval_secs {
                    config.scanner.bootstrap_interval_secs = v;
                }
                if let Some(v) = scanner.critical_interval_ms {
                    config.scanner.critical_interval_ms = v;
                }
                if let Some(v) = scanner.hot_interval_ms {
                    config.scanner.hot_interval_ms = v;
                }
                if let Some(v) = scanner.warm_interval_ms {
                    // Convert ms to secs if needed (deployment config uses ms)
                    config.scanner.warm_interval_secs = v / 1000;
                }
                if let Some(v) = scanner.cold_interval_ms {
                    // Convert ms to secs if needed (deployment config uses ms)
                    config.scanner.cold_interval_secs = v / 1000;
                }
            }

            if let Some(liq) = &ovr.liquidation {
                if let Some(v) = liq.close_factor {
                    config.liquidation.close_factor = v;
                }
                if let Some(v) = liq.min_profit_usd {
                    config.liquidation.min_profit_usd = v;
                }
                if let Some(v) = liq.max_slippage_pct {
                    config.liquidation.max_slippage_pct = v;
                }
                if let Some(v) = liq.gas_multiplier {
                    config.liquidation.gas_price_multiplier = v;
                }
            }
        }

        config
    }

    /// Get list of available deployment names.
    pub fn available_deployments(&self) -> Vec<&str> {
        self.registry.deployment_names().collect()
    }

    /// Get the config registry for direct access.
    pub fn registry(&self) -> &ConfigRegistry {
        &self.registry
    }
}

/// Load a deployment from the default config directory.
///
/// Uses CONFIG_DIR env var or defaults to "./config".
pub fn load_deployment(deployment_name: &str) -> Result<ResolvedDeployment> {
    let config_dir = std::env::var("CONFIG_DIR").unwrap_or_else(|_| "./config".to_string());
    let loader = DeploymentLoader::new(&config_dir)?;
    loader.load(deployment_name)
}

/// Load deployment from DEPLOYMENT env var.
pub fn load_deployment_from_env() -> Result<ResolvedDeployment> {
    let config_dir = std::env::var("CONFIG_DIR").unwrap_or_else(|_| "./config".to_string());
    let loader = DeploymentLoader::new(&config_dir)?;
    loader.load_from_env()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_expansion() {
        std::env::set_var("TEST_VAR", "test_value");

        let expand = |s: &str| -> String {
            if s.starts_with("${") && s.ends_with("}") {
                let var_name = &s[2..s.len()-1];
                std::env::var(var_name).unwrap_or_else(|_| s.to_string())
            } else {
                s.to_string()
            }
        };

        assert_eq!(expand("${TEST_VAR}"), "test_value");
        assert_eq!(expand("literal"), "literal");
        assert_eq!(expand("${NONEXISTENT}"), "${NONEXISTENT}");

        std::env::remove_var("TEST_VAR");
    }
}
