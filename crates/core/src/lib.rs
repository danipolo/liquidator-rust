//! Liquidator core logic.
//!
//! This crate provides the core liquidation bot functionality:
//! - Asset registry with oracle configurations
//! - Tiered position tracking (Critical/Hot/Warm/Cold)
//! - Trigger-based position index for instant liquidation detection
//! - Health factor sensitivity estimation
//! - Transaction pre-staging for critical positions
//! - Heartbeat prediction for oracle updates
//! - Scanner orchestration
//!
//! Supports multiple lending protocols (AAVE v3/v4) and EVM chains.

mod assets;
pub mod config;
mod heartbeat;
mod liquidator;
mod position;
mod position_tracker;
mod pre_staging;
mod scanner;
mod sensitivity;
mod trigger_index;
pub mod u256_math;

pub use assets::{Asset, AssetRegistry, DynamicAsset, DynamicAssetRegistry, OracleType, ASSETS, REGISTRY};
pub use config::{
    BotConfig, config, init_config, load_deployment, load_deployment_from_env,
    ResolvedDeployment, ResolvedAsset, ResolvedContracts, ChainDetails as ResolvedChainDetails,
    ProtocolDetails as ResolvedProtocolDetails, RpcUrls, GasDetails,
};
pub use heartbeat::HeartbeatPredictor;
pub use liquidator::{Liquidator, LiquidationParams, LiquidationResult, ProfitEstimate};
pub use position::{CollateralData, DebtData, PositionTier, TrackedPosition};
pub use position_tracker::TieredPositionTracker;
pub use pre_staging::{PreStager, StagedLiquidation};
pub use scanner::{Scanner, ScannerConfig};
pub use sensitivity::PositionSensitivity;
pub use trigger_index::{PriceDirection, TriggerEntry, TriggerIndex};
