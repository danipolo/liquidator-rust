//! Liquidator chain interaction layer.
//!
//! This crate provides:
//! - Provider management for HTTP and WebSocket connections
//! - Contract bindings for Pool, BalancesReader, Oracle, Liquidator
//! - Event listeners for real-time oracle and pool events
//! - Oracle price monitoring and caching
//! - DualOracle tier tracking for LST assets
//! - Transaction signing and sending
//! - Gas strategy abstraction (Legacy + EIP-1559)
//!
//! Supports multiple EVM chains with configurable RPC endpoints and gas settings.

mod contracts;
mod dual_oracle;
mod event_listener;
pub mod gas;
pub mod oracle;
mod oracle_monitor;
pub mod protocol;
mod provider;
mod signer;

pub use contracts::{
    event_signatures, ExecutionMode, FlashLoanProvider, InstructionBuilder, InstructionType,
    LiquidationStrategy, LiquidatorContract, OracleAggregator, PoolContract, SwapAdapter,
    SwapAllocation,
};
pub use dual_oracle::{DualOracleMonitor, DualOracleTier, TierTransition};
pub use event_listener::{EventListener, OracleType as EventOracleType, OracleUpdate, PoolEvent};
pub use oracle::{
    ChainlinkOracle, ChainlinkOracleBuilder, Oracle, OracleConfig, OracleEventHandler,
    OracleFactory, OraclePrice as OraclePriceData, OracleProvider, OracleType, OracleTypeConfig,
    OraclesConfig, PriceCache, PriceData, PriceSource, RoundData,
};
pub use oracle_monitor::{OracleMonitor, OraclePrice};
pub use protocol::{
    AaveV3Config, AaveV3ConfigBuilder, AaveV3Protocol, AssetConfig as ProtocolAssetConfig,
    ChainProtocolConfig, CollateralPosition, DebtPosition, LendingProtocol, LiquidatableProtocol,
    LiquidationCallParams, LiquidationParams, PoolEvent as ProtocolPoolEvent, PoolEventType,
    PositionData, ProtocolEventSignatures, ProtocolFactory, ProtocolSwapConfig, ProtocolVersion,
};
pub use provider::{BalanceData, ProviderManager};
pub use signer::TransactionSender;
