//! HyperLend API clients for external services.
//!
//! This crate provides HTTP clients for:
//! - BlockAnalitica: At-risk wallet discovery and position data
//! - Liqd.ag: Swap routing for liquidation execution

mod blockanalitica;
mod liqd;

pub use blockanalitica::{AtRiskWallet, BlockAnaliticaClient, PositionDistribution, ProfitabilityFilter, WalletAsset, WalletStats};
pub use liqd::{LiqdClient, SwapAllocation, SwapHop, SwapRoute};
