//! Chain configuration for multi-chain support.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

/// Chain configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    /// Chain details
    pub chain: ChainDetails,
}

/// Chain details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainDetails {
    /// Chain ID
    pub chain_id: u64,
    /// Human-readable name
    pub name: String,
    /// Native token symbol (e.g., "ETH", "HYPE", "MATIC")
    pub native_token: String,
    /// Block time in milliseconds
    pub block_time_ms: u64,
    /// Explorer URL for transaction links
    #[serde(default)]
    pub explorer_url: Option<String>,
    /// RPC configuration
    pub rpc: RpcConfig,
    /// Gas configuration
    pub gas: GasConfig,
    /// Swap routing configuration
    #[serde(default)]
    pub swap: Option<SwapConfig>,
}

impl ChainDetails {
    /// Get block time as Duration.
    pub fn block_time(&self) -> Duration {
        Duration::from_millis(self.block_time_ms)
    }
}

/// RPC endpoint configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcConfig {
    /// Primary HTTP RPC endpoint
    pub http: String,
    /// WebSocket RPC endpoint for subscriptions
    #[serde(default)]
    pub ws: String,
    /// Archive node RPC endpoint (optional)
    #[serde(default)]
    pub archive: Option<String>,
    /// Dedicated send RPC endpoint for faster tx submission (optional)
    #[serde(default)]
    pub send: Option<String>,
}

/// Gas pricing configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasConfig {
    /// Gas pricing model
    pub pricing: GasPricingModel,
    /// Gas limit multiplier (e.g., 1.1 for 10% buffer)
    #[serde(default = "default_limit_multiplier")]
    pub limit_multiplier: f64,
    /// Maximum gas price willing to pay (in gwei)
    #[serde(default = "default_max_gas_price")]
    pub max_gas_price_gwei: f64,
    /// Default gas price for legacy transactions (in gwei)
    #[serde(default = "default_gas_price")]
    pub default_gas_price_gwei: f64,
    /// Priority fee for EIP-1559 transactions (in gwei)
    #[serde(default)]
    pub priority_fee_gwei: Option<f64>,
}

fn default_limit_multiplier() -> f64 {
    1.1
}

fn default_max_gas_price() -> f64 {
    100.0
}

fn default_gas_price() -> f64 {
    1.0
}

/// Gas pricing model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GasPricingModel {
    /// Legacy gas pricing (gas price only)
    Legacy,
    /// EIP-1559 (base fee + priority fee)
    Eip1559,
    /// Custom pricing (chain-specific)
    Custom,
}

/// Swap routing configuration for a chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapConfig {
    /// Default swap adapter type for this chain.
    /// Values: "liquidswap" (0), "uniswap_v3" (1), "direct" (2)
    pub default_adapter: String,
    /// Uniswap V3 contract addresses (if applicable)
    #[serde(default)]
    pub uniswap_v3: Option<UniswapV3Config>,
    /// LiquidSwap/Liqd.ag configuration (if applicable)
    #[serde(default)]
    pub liquidswap: Option<LiquidSwapConfig>,
}

impl SwapConfig {
    /// Get the default adapter ID for this chain.
    pub fn default_adapter_id(&self) -> u8 {
        match self.default_adapter.to_lowercase().as_str() {
            "liquidswap" | "liqd" => 0,
            "uniswap_v3" | "uniswapv3" | "uniswap" => 1,
            "direct" => 2,
            _ => 0, // Default to liquidswap
        }
    }
}

/// Uniswap V3 contract addresses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniswapV3Config {
    /// SwapRouter02 address
    pub swap_router: String,
    /// QuoterV2 address
    pub quoter_v2: String,
    /// Factory address
    pub factory: String,
}

/// LiquidSwap/Liqd configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidSwapConfig {
    /// Liqd.ag API endpoint
    #[serde(default)]
    pub api_url: Option<String>,
    /// Router contract address
    #[serde(default)]
    pub router: Option<String>,
}

impl ChainConfig {
    /// Load chain config from a TOML file.
    pub fn from_file(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())?;
        let config: ChainConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Expand environment variables in config values.
    pub fn expand_env_vars(&mut self) {
        self.chain.rpc.http = expand_env(&self.chain.rpc.http);
        self.chain.rpc.ws = expand_env(&self.chain.rpc.ws);
        if let Some(ref mut archive) = self.chain.rpc.archive {
            *archive = expand_env(archive);
        }
        if let Some(ref mut send) = self.chain.rpc.send {
            *send = expand_env(send);
        }
    }
}

/// Expand ${VAR_NAME} patterns with environment variable values.
fn expand_env(s: &str) -> String {
    let mut result = s.to_string();
    let re = regex_lite::Regex::new(r"\$\{([^}]+)\}").unwrap();

    for cap in re.captures_iter(s) {
        if let (Some(full_match), Some(var_match)) = (cap.get(0), cap.get(1)) {
            let var_name = var_match.as_str();
            if let Ok(value) = std::env::var(var_name) {
                result = result.replace(full_match.as_str(), &value);
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_env() {
        // Use unique var name to avoid conflicts with parallel tests
        std::env::set_var("CHAIN_TEST_VAR", "test_value");
        assert_eq!(expand_env("${CHAIN_TEST_VAR}"), "test_value");
        assert_eq!(expand_env("prefix_${CHAIN_TEST_VAR}_suffix"), "prefix_test_value_suffix");
        assert_eq!(expand_env("no_vars"), "no_vars");
        std::env::remove_var("CHAIN_TEST_VAR");
    }
}
