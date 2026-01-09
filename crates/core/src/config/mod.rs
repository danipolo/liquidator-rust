//! Configuration system for multi-chain, multi-protocol liquidation bot.
//!
//! This module provides:
//! - Bot runtime configuration (profiles, thresholds, timing)
//! - Chain configuration (RPC endpoints, gas settings)
//! - Protocol configuration (contract addresses, parameters)
//! - Asset configuration (tokens, oracles, liquidation bonuses)
//! - Deployment configuration (ties everything together)
//! - Configuration registry for runtime loading

mod asset_config;
mod bot;
mod chain;
mod deployment;
mod loader;
mod protocol;
mod registry;

// Re-export bot config (main runtime config)
pub use bot::{
    config, init_config, BotConfig, LiquidationConfig, PositionConfig, PreStagingConfigValues,
    ScannerTimingConfig, TierConfig,
};

// Re-export chain config
pub use chain::{
    ChainConfig, ChainDetails, GasConfig, GasPricingModel, LiquidSwapConfig, RpcConfig, SwapConfig,
    UniswapV3Config,
};

// Re-export protocol config
pub use protocol::{
    ProtocolApi, ProtocolConfig, ProtocolContracts, ProtocolDetails, ProtocolParameters,
    ProtocolVersion,
};

// Re-export asset config
pub use asset_config::{AssetConfig, AssetsConfig};

// Re-export deployment config
pub use deployment::{
    BotConfigOverrides, DeploymentConfig, DeploymentContracts, DeploymentDetails,
    LiquidationOverrides, PositionOverrides, PreStagingOverrides, ScannerOverrides, TierOverrides,
};

// Re-export config registry
pub use registry::ConfigRegistry;

// Re-export deployment loader
pub use loader::{
    load_deployment, load_deployment_from_env, ChainDetails as ResolvedChainDetails,
    DeploymentLoader, GasDetails, ProtocolDetails as ResolvedProtocolDetails, ResolvedAsset,
    ResolvedContracts, ResolvedDeployment, RpcUrls,
};
