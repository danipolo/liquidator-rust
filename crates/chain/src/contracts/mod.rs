//! Contract bindings for lending protocols.
//!
//! This module provides type definitions and ABI constants for interacting
//! with various lending protocol smart contracts.
//!
//! # Protocol Support
//!
//! Protocol support is controlled via feature flags:
//! - `aave-v3` (default): AAVE V3 and forks (HyperLend, etc.)
//! - `aave-v4`: AAVE V4 (upcoming)
//! - `compound-v3`: Compound V3 (Comet)
//!
//! # Execution Modes
//!
//! The [`executor`] module provides flexible execution strategies:
//! - **Flash Loan**: Borrow → Liquidate → Swap → Repay (no capital needed)
//! - **Direct**: Swap → Liquidate (requires capital)
//! - **Legacy**: Old interface where contract handles flash loan internally
//!
//! # Example
//!
//! ```rust,ignore
//! use liquidator_chain::contracts::{aave_v3, executor, common};
//!
//! // New: Instruction-based execution (bot decides strategy)
//! let strategy = executor::build_flash_loan_strategy(
//!     FlashLoanProvider::AaveV3,
//!     pool, user, collateral, debt,
//!     debt_amount, min_collateral,
//!     SwapAdapter::UniswapV3, swap_data,
//!     profit_token, min_profit,
//! );
//!
//! // Legacy: Direct liquidation call
//! let calldata = aave_v3::encode_liquidation(user, collateral, debt, amount, min_out, swap_data);
//! ```

pub mod aave_v3;
pub mod bindings;
pub mod common;
pub mod executor;

// Re-export commonly used types
pub use aave_v3::{wrap_swap_data, SwapAdapter, SwapAllocation};
pub use executor::{
    build_direct_strategy, build_flash_loan_strategy, ExecutionMode, FlashLoanProvider,
    InstructionBuilder, InstructionType, LiquidationStrategy,
};

// Re-export contract bindings from JSON artifacts
pub use bindings::{ILiquidSwap, ILiquidator, IPool, ISwapAdapter, ISwapRouter, IWETH};

use alloy::primitives::{Address, Bytes, B256, U256};
use std::sync::Arc;
use std::time::Instant;

use crate::signer::TransactionSender;

// Backward compatibility: re-export event_signatures module
pub mod event_signatures {
    pub use super::aave_v3::aave_v3_signatures::*;
    pub use super::common::common_signatures::ANSWER_UPDATED;

    use alloy::primitives::B256;

    /// Get all pool event signatures (for backward compatibility).
    pub fn pool_signatures() -> Vec<B256> {
        super::aave_v3::aave_v3_signatures::pool_signatures()
    }
}

/// Liquidator contract wrapper with transaction sending capability.
///
/// This wrapper provides a high-level interface for interacting with
/// custom liquidator contracts that handle flash loans and swap routing.
pub struct LiquidatorContract {
    /// Contract address
    pub address: Address,
    /// Encoded calldata cache for pre-staging
    calldata_cache: parking_lot::RwLock<Option<Bytes>>,
    /// Transaction sender (optional)
    sender: Option<Arc<TransactionSender>>,
}

impl LiquidatorContract {
    /// Create a new Liquidator contract wrapper.
    pub fn new(address: Address) -> Self {
        Self {
            address,
            calldata_cache: parking_lot::RwLock::new(None),
            sender: None,
        }
    }

    /// Create a new Liquidator contract wrapper with transaction sender.
    pub fn with_sender(address: Address, sender: Arc<TransactionSender>) -> Self {
        Self {
            address,
            calldata_cache: parking_lot::RwLock::new(None),
            sender: Some(sender),
        }
    }

    /// Set the transaction sender.
    pub fn set_sender(&mut self, sender: Arc<TransactionSender>) {
        self.sender = Some(sender);
    }

    /// Encode liquidation calldata for pre-staging or dry-run.
    /// Uses the new interface with adapter-specific swapData encoding.
    pub fn encode_liquidate(
        &self,
        user: Address,
        collateral: Address,
        debt: Address,
        debt_to_cover: U256,
        min_amount_out: U256,
        swap_data: Bytes,
    ) -> Bytes {
        aave_v3::encode_liquidation(user, collateral, debt, debt_to_cover, min_amount_out, swap_data)
    }

    /// Encode liquidation calldata with adapter-specific swap data.
    pub fn encode_liquidate_with_adapter(
        &self,
        user: Address,
        collateral: Address,
        debt: Address,
        debt_to_cover: U256,
        min_amount_out: U256,
        adapter: SwapAdapter,
        hops: Vec<Vec<SwapAllocation>>,
        tokens: Vec<Address>,
    ) -> Bytes {
        aave_v3::encode_liquidation_with_adapter(
            user, collateral, debt, debt_to_cover, min_amount_out, adapter, hops, tokens,
        )
    }

    /// Encode swap data for the appropriate adapter.
    /// Returns self-describing swapData: abi.encode(uint8 adapterType, bytes adapterData)
    pub fn encode_swap_data(
        &self,
        adapter: SwapAdapter,
        hops: Vec<Vec<SwapAllocation>>,
        tokens: Vec<Address>,
    ) -> Bytes {
        match adapter {
            SwapAdapter::LiquidSwap => aave_v3::encode_liquidswap_data(hops, tokens),
            SwapAdapter::UniswapV3 => {
                let fee = hops.first()
                    .and_then(|h| h.first())
                    .map(|a| a.fee)
                    .unwrap_or(3000);
                aave_v3::encode_uniswap_v3_data(&tokens, fee)
            }
            SwapAdapter::Direct => aave_v3::encode_direct_swap_data(),
        }
    }

    /// Encode rescue tokens calldata (rescues all tokens).
    pub fn encode_rescue_tokens(&self, token: Address, recipient: Address) -> Bytes {
        aave_v3::encode_rescue_tokens(token, recipient)
    }

    /// Encode rescue tokens calldata with specific amount.
    pub fn encode_rescue_tokens_amount(
        &self,
        token: Address,
        amount: U256,
        recipient: Address,
    ) -> Bytes {
        aave_v3::encode_rescue_tokens_amount(token, amount, recipient)
    }

    /// Execute a liquidation transaction with adapter-specific swap data.
    pub async fn liquidate(
        &self,
        user: Address,
        collateral: Address,
        debt: Address,
        debt_to_cover: U256,
        min_amount_out: U256,
        adapter: SwapAdapter,
        hops: Vec<Vec<SwapAllocation>>,
        tokens: Vec<Address>,
    ) -> anyhow::Result<B256> {
        let encode_start = Instant::now();
        let calldata = self.encode_liquidate_with_adapter(
            user,
            collateral,
            debt,
            debt_to_cover,
            min_amount_out,
            adapter,
            hops,
            tokens,
        );
        let encode_elapsed = encode_start.elapsed();

        *self.calldata_cache.write() = Some(calldata.clone());

        if let Some(sender) = &self.sender {
            tracing::info!(
                contract = %self.address,
                user = %user,
                collateral = %collateral,
                debt = %debt,
                encode_us = encode_elapsed.as_micros(),
                calldata_len = calldata.len(),
                "[CONTRACT] Sending liquidation"
            );

            sender
                .send_transaction(self.address, calldata, U256::ZERO)
                .await
        } else {
            anyhow::bail!(
                "Transaction ready but signer not configured. Calldata: {} bytes",
                calldata.len()
            )
        }
    }

    /// Execute a liquidation with pre-encoded calldata (fastest path).
    pub async fn execute_preencoded(&self, calldata: Bytes) -> anyhow::Result<B256> {
        *self.calldata_cache.write() = Some(calldata.clone());

        if let Some(sender) = &self.sender {
            tracing::info!(
                contract = %self.address,
                calldata_len = calldata.len(),
                "[CONTRACT] Executing pre-encoded liquidation (FAST PATH)"
            );

            sender
                .send_transaction(self.address, calldata, U256::ZERO)
                .await
        } else {
            anyhow::bail!(
                "Transaction ready but signer not configured. Calldata: {} bytes",
                calldata.len()
            )
        }
    }

    /// Rescue tokens from the contract.
    pub async fn rescue_tokens(&self, token: Address, recipient: Address) -> anyhow::Result<B256> {
        let calldata = self.encode_rescue_tokens(token, recipient);

        if let Some(sender) = &self.sender {
            tracing::info!(
                contract = %self.address,
                token = %token,
                recipient = %recipient,
                "Sending rescue tokens transaction"
            );

            sender
                .send_transaction(self.address, calldata, U256::ZERO)
                .await
        } else {
            anyhow::bail!(
                "Transaction ready but signer not configured. Calldata: {} bytes",
                calldata.len()
            )
        }
    }

    /// Get cached calldata (for inspection/debugging).
    pub fn cached_calldata(&self) -> Option<Bytes> {
        self.calldata_cache.read().clone()
    }

    // ========== NEW EXECUTOR INTERFACE ==========

    /// Execute a liquidation strategy (new instruction-based interface).
    ///
    /// This uses the flexible executor contract that supports:
    /// - Flash loan mode: Bot encodes flash loan + liquidation + swap
    /// - Direct mode: Bot encodes swap + liquidation
    ///
    /// The contract blindly executes the instructions in sequence.
    pub async fn execute_strategy(
        &self,
        strategy: &LiquidationStrategy,
    ) -> anyhow::Result<B256> {
        use alloy::sol_types::SolCall;

        let encode_start = Instant::now();

        // Encode based on execution mode
        let calldata = match strategy.mode {
            ExecutionMode::FlashLoan => {
                // Use executeWithFlashLoan if flash loan provider is set
                if let Some(provider) = strategy.flash_provider {
                    // For now, encode as regular execute - contract handles flash loan internally
                    // In future, could use executeWithFlashLoan for explicit control
                    let call = executor::IExecutor::executeCall {
                        instructions: strategy.instructions.clone(),
                    };
                    Bytes::from(call.abi_encode())
                } else {
                    anyhow::bail!("Flash loan mode requires flash_provider to be set");
                }
            }
            ExecutionMode::Direct => {
                let call = executor::IExecutor::executeCall {
                    instructions: strategy.instructions.clone(),
                };
                Bytes::from(call.abi_encode())
            }
        };

        let encode_elapsed = encode_start.elapsed();
        *self.calldata_cache.write() = Some(calldata.clone());

        if let Some(sender) = &self.sender {
            tracing::info!(
                contract = %self.address,
                mode = ?strategy.mode,
                encode_us = encode_elapsed.as_micros(),
                calldata_len = calldata.len(),
                min_profit = %strategy.min_profit,
                "[EXECUTOR] Executing liquidation strategy"
            );

            sender
                .send_transaction(self.address, calldata, U256::ZERO)
                .await
        } else {
            anyhow::bail!(
                "Transaction ready but signer not configured. Calldata: {} bytes",
                calldata.len()
            )
        }
    }

    /// Execute raw instructions directly (advanced usage).
    ///
    /// Use this when you've built instructions manually with InstructionBuilder.
    pub async fn execute_instructions(&self, instructions: Bytes) -> anyhow::Result<B256> {
        use alloy::sol_types::SolCall;

        let call = executor::IExecutor::executeCall { instructions };
        let calldata = Bytes::from(call.abi_encode());

        *self.calldata_cache.write() = Some(calldata.clone());

        if let Some(sender) = &self.sender {
            tracing::info!(
                contract = %self.address,
                calldata_len = calldata.len(),
                "[EXECUTOR] Executing raw instructions"
            );

            sender
                .send_transaction(self.address, calldata, U256::ZERO)
                .await
        } else {
            anyhow::bail!(
                "Transaction ready but signer not configured. Calldata: {} bytes",
                calldata.len()
            )
        }
    }

    /// Build a flash loan liquidation strategy.
    ///
    /// Convenience method that wraps [`build_flash_loan_strategy`].
    pub fn build_flash_loan_strategy(
        &self,
        pool: Address,
        user: Address,
        collateral: Address,
        debt: Address,
        debt_amount: U256,
        min_collateral_out: U256,
        swap_adapter: SwapAdapter,
        swap_data: Bytes,
        profit_token: Address,
        min_profit: U256,
    ) -> LiquidationStrategy {
        executor::build_flash_loan_strategy(
            FlashLoanProvider::AaveV3, // Default to AAVE V3
            pool,
            user,
            collateral,
            debt,
            debt_amount,
            min_collateral_out,
            swap_adapter,
            swap_data,
            profit_token,
            min_profit,
        )
    }

    /// Build a direct liquidation strategy (no flash loan).
    ///
    /// Convenience method that wraps [`build_direct_strategy`].
    pub fn build_direct_strategy(
        &self,
        pool: Address,
        user: Address,
        collateral: Address,
        debt: Address,
        debt_amount: U256,
        min_collateral_out: U256,
        swap_adapter: SwapAdapter,
        swap_data: Bytes,
        profit_token: Address,
        min_profit: U256,
    ) -> LiquidationStrategy {
        executor::build_direct_strategy(
            pool,
            user,
            collateral,
            debt,
            debt_amount,
            min_collateral_out,
            swap_adapter,
            swap_data,
            profit_token,
            min_profit,
        )
    }
}

/// Pool contract wrapper for event filtering.
pub struct PoolContract {
    pub address: Address,
}

impl PoolContract {
    pub fn new(address: Address) -> Self {
        Self { address }
    }

    /// Get event signatures for subscription.
    pub fn event_signatures() -> Vec<B256> {
        event_signatures::pool_signatures()
    }
}

/// Oracle aggregator utilities.
pub struct OracleAggregator;

impl OracleAggregator {
    /// Get event signature for AnswerUpdated.
    pub fn answer_updated_signature() -> B256 {
        event_signatures::ANSWER_UPDATED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_signatures() {
        let sigs = PoolContract::event_signatures();
        assert_eq!(sigs.len(), 5);

        let answer_sig = OracleAggregator::answer_updated_signature();
        assert!(!answer_sig.is_zero());
    }
}
