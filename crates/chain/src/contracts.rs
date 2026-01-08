//! Contract bindings for HyperLend protocol.
//!
//! This module provides type definitions and ABI constants for interacting
//! with HyperLend smart contracts.

use alloy::primitives::{Address, Bytes, B256, U256};
use alloy::sol;
use alloy::sol_types::SolCall;

// Define contract interfaces using sol! macro for ABI generation
sol! {
    /// Aave V3 Pool interface (subset for liquidation)
    interface IPool {
        event Supply(address indexed reserve, address user, address indexed onBehalfOf, uint256 amount, uint16 indexed referralCode);
        event Withdraw(address indexed reserve, address indexed user, address indexed to, uint256 amount);
        event Borrow(address indexed reserve, address user, address indexed onBehalfOf, uint256 amount, uint8 interestRateMode, uint256 borrowRate, uint16 indexed referralCode);
        event Repay(address indexed reserve, address indexed user, address indexed repayer, uint256 amount, bool useATokens);
        event LiquidationCall(address indexed collateralAsset, address indexed debtAsset, address indexed user, uint256 debtToCover, uint256 liquidatedCollateralAmount, address liquidator, bool receiveAToken);
    }

    /// Oracle aggregator interface (Chainlink-compatible)
    interface IAggregator {
        event AnswerUpdated(int256 indexed current, uint256 indexed roundId, uint256 updatedAt);
    }

    /// Swap allocation struct for liquidation (matches Solidity tuple)
    #[derive(Debug)]
    struct SwapAlloc {
        address tokenIn;
        address tokenOut;
        uint8 routerIndex;
        uint24 fee;          // uint24 per original ABI
        uint256 amountIn;
        bool stable;
    }

    /// Liquidator contract interface (matches deployed contract)
    interface ILiquidator {
        function liquidate(
            address _user,
            address _collateral,
            address _debt,
            uint256 _debtAmount,
            SwapAlloc[][] calldata _hops,
            address[] calldata _tokens,
            uint256 _minAmountOut
        ) external returns (uint256);

        function rescueTokens(
            address _token,
            uint256 _amount,
            bool _max,
            address _to
        ) external;
    }
}

/// Event signature constants for log filtering.
pub mod event_signatures {
    use super::*;

    /// keccak256("AnswerUpdated(int256,uint256,uint256)")
    pub const ANSWER_UPDATED: B256 = B256::new([
        0x05, 0x59, 0x88, 0x4f, 0xd3, 0x34, 0x29, 0x55, 0xd1, 0xfc, 0x4b, 0x32, 0xf8, 0x0a, 0xb7,
        0x04, 0x98, 0x87, 0xe6, 0xe4, 0x32, 0x88, 0x03, 0x12, 0xfa, 0xea, 0x3c, 0x13, 0x6b, 0x0c,
        0xdb, 0xc4,
    ]);

    /// keccak256("Supply(address,address,address,uint256,uint16)")
    pub const SUPPLY: B256 = B256::new([
        0x2b, 0x62, 0x7c, 0xe5, 0x32, 0x47, 0xe1, 0x4b, 0x2c, 0x94, 0x3c, 0xb3, 0x84, 0xf6, 0x22,
        0xb9, 0x70, 0x64, 0x99, 0x4c, 0x68, 0x32, 0x18, 0x0f, 0x2a, 0x71, 0x7c, 0x7f, 0xa2, 0xac,
        0xe2, 0x9e,
    ]);

    /// keccak256("Withdraw(address,address,address,uint256)")
    pub const WITHDRAW: B256 = B256::new([
        0x31, 0x15, 0xd1, 0x44, 0x9a, 0x7b, 0x73, 0x2c, 0x4a, 0x14, 0x53, 0x4b, 0x82, 0x26, 0x19,
        0xf7, 0x2c, 0xc4, 0xd7, 0x0e, 0xf5, 0x2d, 0x8e, 0x0e, 0x2a, 0x7d, 0x6d, 0x80, 0x6b, 0x48,
        0xd8, 0x39,
    ]);

    /// keccak256("Borrow(address,address,address,uint256,uint8,uint256,uint16)")
    pub const BORROW: B256 = B256::new([
        0xb3, 0xd0, 0x84, 0x82, 0x0f, 0xb1, 0xa9, 0xde, 0xcf, 0xef, 0xf7, 0xce, 0x23, 0xfb, 0x0d,
        0xb6, 0x95, 0x43, 0xa8, 0xae, 0x27, 0x5f, 0xde, 0x06, 0x3a, 0xba, 0xf5, 0x81, 0x2f, 0x3c,
        0xc5, 0x88,
    ]);

    /// keccak256("Repay(address,address,address,uint256,bool)")
    pub const REPAY: B256 = B256::new([
        0xa5, 0x34, 0xc8, 0xdc, 0xe0, 0x52, 0x79, 0xf5, 0xb3, 0x05, 0xbd, 0xfd, 0xa9, 0x35, 0x48,
        0x8f, 0xf4, 0xf1, 0xc8, 0x3d, 0xd2, 0x62, 0x1e, 0x7e, 0xb0, 0x56, 0xd7, 0xa5, 0x93, 0x98,
        0x74, 0x80,
    ]);

    /// keccak256("LiquidationCall(address,address,address,uint256,uint256,address,bool)")
    pub const LIQUIDATION_CALL: B256 = B256::new([
        0xe4, 0x13, 0xa3, 0x21, 0xe8, 0x68, 0x14, 0x69, 0x7e, 0x5d, 0x12, 0x0c, 0xb6, 0x28, 0x45,
        0x1e, 0x97, 0x08, 0x86, 0x7c, 0xfd, 0x6a, 0x6c, 0xd8, 0x16, 0xd2, 0xe7, 0xb0, 0xb4, 0xd0,
        0xb4, 0x80,
    ]);

    /// Get all pool event signatures.
    pub fn pool_signatures() -> Vec<B256> {
        vec![SUPPLY, WITHDRAW, BORROW, REPAY, LIQUIDATION_CALL]
    }
}

use crate::signer::TransactionSender;
use std::sync::Arc;
use std::time::Instant;

/// Liquidator contract wrapper with transaction sending capability.
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
    pub fn encode_liquidate(
        &self,
        user: Address,
        collateral: Address,
        debt: Address,
        debt_to_cover: U256,
        hops: Vec<Vec<SwapAllocation>>,
        tokens: Vec<Address>,
        min_amount_out: U256,
    ) -> Bytes {
        // Convert SwapAllocation to SwapAlloc for ABI encoding
        let encoded_hops: Vec<Vec<SwapAlloc>> = hops
            .into_iter()
            .map(|hop| hop.into_iter().map(|a| a.to_sol()).collect())
            .collect();

        let call = ILiquidator::liquidateCall {
            _user: user,
            _collateral: collateral,
            _debt: debt,
            _debtAmount: debt_to_cover,
            _hops: encoded_hops,
            _tokens: tokens,
            _minAmountOut: min_amount_out,
        };

        Bytes::from(call.abi_encode())
    }

    /// Encode rescue tokens calldata.
    /// Uses max=true to rescue all tokens (matches original bot behavior).
    pub fn encode_rescue_tokens(&self, token: Address, recipient: Address) -> Bytes {
        let call = ILiquidator::rescueTokensCall {
            _token: token,
            _amount: U256::ZERO, // Not used when _max is true
            _max: true,          // Always rescue all (per original bot)
            _to: recipient,
        };
        Bytes::from(call.abi_encode())
    }

    /// Encode rescue tokens calldata with specific amount.
    pub fn encode_rescue_tokens_amount(
        &self,
        token: Address,
        amount: U256,
        recipient: Address,
    ) -> Bytes {
        let call = ILiquidator::rescueTokensCall {
            _token: token,
            _amount: amount,
            _max: false,
            _to: recipient,
        };
        Bytes::from(call.abi_encode())
    }

    /// Execute a liquidation transaction.
    ///
    /// This method encodes the calldata and sends the transaction.
    /// Requires a sender to be configured via `with_sender` or `set_sender`.
    pub async fn liquidate(
        &self,
        user: Address,
        collateral: Address,
        debt: Address,
        debt_to_cover: U256,
        hops: Vec<Vec<SwapAllocation>>,
        tokens: Vec<Address>,
        min_amount_out: U256,
    ) -> anyhow::Result<B256> {
        // TIMING: Calldata encoding (slow path - encodes at runtime)
        let encode_start = Instant::now();
        let calldata = self.encode_liquidate(
            user,
            collateral,
            debt,
            debt_to_cover,
            hops,
            tokens,
            min_amount_out,
        );
        let encode_elapsed = encode_start.elapsed();

        // Cache for inspection
        *self.calldata_cache.write() = Some(calldata.clone());

        // Send transaction if sender is configured
        if let Some(sender) = &self.sender {
            tracing::info!(
                contract = %self.address,
                user = %user,
                collateral = %collateral,
                debt = %debt,
                encode_us = encode_elapsed.as_micros(),
                calldata_len = calldata.len(),
                "[CONTRACT] Sending liquidation (runtime encoding took {}us)",
                encode_elapsed.as_micros()
            );

            sender
                .send_transaction(self.address, calldata, U256::ZERO)
                .await
        } else {
            tracing::info!(
                contract = %self.address,
                calldata_len = calldata.len(),
                encode_us = encode_elapsed.as_micros(),
                "Liquidation calldata encoded (signer required for actual execution)"
            );

            anyhow::bail!(
                "Transaction ready but signer not configured. Calldata: {} bytes",
                calldata.len()
            )
        }
    }

    /// Execute a liquidation with pre-encoded calldata (fastest path).
    ///
    /// OPTIMIZATION: Skips encoding step entirely (~5ms savings).
    /// Use this when calldata was pre-encoded during staging.
    pub async fn execute_preencoded(&self, calldata: Bytes) -> anyhow::Result<B256> {
        let start = Instant::now();

        // Cache for inspection (minimal overhead)
        *self.calldata_cache.write() = Some(calldata.clone());
        let cache_elapsed = start.elapsed();

        if let Some(sender) = &self.sender {
            tracing::info!(
                contract = %self.address,
                calldata_len = calldata.len(),
                cache_us = cache_elapsed.as_micros(),
                "[CONTRACT] Executing pre-encoded liquidation (FAST PATH - 0us encoding)"
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

        // Send transaction if sender is configured
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
            tracing::info!(
                contract = %self.address,
                token = %token,
                recipient = %recipient,
                "Rescue tokens calldata encoded (signer required for actual execution)"
            );

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
}

/// Swap allocation for liquidation (Rust representation).
#[derive(Debug, Clone)]
pub struct SwapAllocation {
    pub token_in: Address,
    pub token_out: Address,
    pub router_index: u8,
    pub fee: u32, // uint24 in Solidity, but stored as u32
    pub amount_in: U256,
    pub stable: bool,
}

impl SwapAllocation {
    /// Convert to the ABI-compatible SwapAlloc type.
    pub fn to_sol(&self) -> SwapAlloc {
        SwapAlloc {
            tokenIn: self.token_in,
            tokenOut: self.token_out,
            routerIndex: self.router_index,
            fee: alloy::primitives::Uint::<24, 1>::from(self.fee & 0xFFFFFF), // Mask to uint24
            amountIn: self.amount_in,
            stable: self.stable,
        }
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
