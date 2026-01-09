//! Liquidator API clients for external services.
//!
//! This crate provides HTTP clients for:
//! - Swap routing: Abstracted swap routing for liquidation execution
//!
//! # Swap Routing
//!
//! The [`swap`] module provides a trait-based abstraction for DEX aggregators:
//! - [`swap::LiqdRouter`]: Liqd.ag integration with caching (HyperLiquid)
//! - [`swap::UniswapV3Router`]: Uniswap V3 for Plasma, Arbitrum, Base, Optimism, Celo
//! - `SwapRouter` trait for implementing additional routers

pub mod swap;

// Swap routing (canonical types)
pub use swap::{
    FeeTier, LiqdRouter, SwapAllocation, SwapHop, SwapParams, SwapRoute, SwapRouter,
    SwapRouterRegistry, UniswapV3Addresses, UniswapV3Router,
};
