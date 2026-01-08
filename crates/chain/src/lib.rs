//! HyperLend chain interaction layer.
//!
//! This crate provides:
//! - Provider management for HTTP and WebSocket connections
//! - Contract bindings for Pool, BalancesReader, Oracle, Liquidator
//! - Event listeners for real-time oracle and pool events
//! - Oracle price monitoring and caching
//! - DualOracle tier tracking for LST assets
//! - Transaction signing and sending

mod contracts;
mod dual_oracle;
mod event_listener;
mod oracle_monitor;
mod provider;
mod signer;

pub use contracts::{
    event_signatures, LiquidatorContract, OracleAggregator, PoolContract, SwapAllocation,
};
pub use dual_oracle::{DualOracleMonitor, DualOracleTier, TierTransition};
pub use event_listener::{EventListener, OracleType, OracleUpdate, PoolEvent};
pub use oracle_monitor::{OracleMonitor, OraclePrice};
pub use provider::{BalanceData, ProviderManager};
pub use signer::TransactionSender;
