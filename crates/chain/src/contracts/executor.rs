//! Instruction-based executor contract interface.
//!
//! This module provides a flexible, extensible interface for encoding
//! multi-step liquidation instructions. The bot decides the execution
//! strategy (flash loan vs direct), and the contract blindly executes.
//!
//! # Architecture
//!
//! ```text
//! Bot (brain) → encodes instructions → Contract (muscle) → executes
//! ```
//!
//! # Execution Modes
//!
//! 1. **Flash Loan Mode**: Bot encodes flash loan initiation, liquidation
//!    happens in callback, profit kept automatically.
//!
//! 2. **Direct Mode**: Bot has funds, executes swap + liquidation directly.
//!
//! # Example
//!
//! ```rust,ignore
//! // Flash loan path
//! let instructions = InstructionBuilder::new()
//!     .flash_loan(debt_token, amount, FlashLoanProvider::Aave)
//!     .liquidate(user, collateral, debt, amount, min_out)
//!     .swap(SwapAdapter::UniswapV3, collateral, debt, swap_data)
//!     .build();
//!
//! // Direct path (no flash loan)
//! let instructions = InstructionBuilder::new()
//!     .swap(SwapAdapter::UniswapV3, debt, collateral, swap_data)
//!     .liquidate(user, collateral, debt, amount, min_out)
//!     .build();
//! ```

use alloy::primitives::{Address, Bytes, U256};
use alloy::sol;
use alloy::sol_types::SolType;

use super::aave_v3::SwapAdapter;

// Executor contract interface
sol! {
    /// Generic executor contract interface.
    /// The contract receives encoded instructions and executes them sequentially.
    interface IExecutor {
        /// Execute a sequence of instructions.
        /// Returns the profit amount (or reverts if unprofitable).
        function execute(bytes calldata instructions) external returns (uint256 profit);

        /// Execute with flash loan as first step.
        /// The flash loan callback will continue execution.
        function executeWithFlashLoan(
            address flashLoanProvider,
            address[] calldata assets,
            uint256[] calldata amounts,
            bytes calldata instructions
        ) external returns (uint256 profit);

        /// Rescue tokens from the contract.
        function rescueTokens(
            address token,
            uint256 amount,
            bool max,
            address to
        ) external;
    }

    /// Flash loan callback interface (AAVE V3 style).
    interface IFlashLoanReceiver {
        function executeOperation(
            address[] calldata assets,
            uint256[] calldata amounts,
            uint256[] calldata premiums,
            address initiator,
            bytes calldata params
        ) external returns (bool);
    }
}

/// Instruction type for the executor contract.
///
/// Each instruction is encoded as: `abi.encode(uint8 instructionType, bytes data)`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InstructionType {
    /// No operation (placeholder)
    Noop = 0,

    /// Execute a swap via adapter.
    /// Data: `abi.encode(uint8 adapterType, address tokenIn, address tokenOut, uint256 amountIn, uint256 minOut, bytes swapData)`
    Swap = 1,

    /// Execute liquidation on lending protocol.
    /// Data: `abi.encode(address pool, address user, address collateral, address debt, uint256 debtAmount, uint256 minCollateralOut)`
    Liquidate = 2,

    /// Transfer tokens to recipient.
    /// Data: `abi.encode(address token, address to, uint256 amount, bool max)`
    Transfer = 3,

    /// Approve tokens for spender.
    /// Data: `abi.encode(address token, address spender, uint256 amount)`
    Approve = 4,

    /// Initiate flash loan (triggers callback).
    /// Data: `abi.encode(uint8 provider, address[] assets, uint256[] amounts)`
    FlashLoan = 5,

    /// Repay flash loan (called in callback).
    /// Data: `abi.encode(uint8 provider, address[] assets, uint256[] amounts, uint256[] premiums)`
    FlashLoanRepay = 6,

    /// Check profit threshold (revert if below).
    /// Data: `abi.encode(address token, uint256 minProfit)`
    ProfitCheck = 7,

    /// Custom call to arbitrary contract (allowlisted only).
    /// Data: `abi.encode(address target, bytes calldata)`
    CustomCall = 8,
}

impl InstructionType {
    /// Get instruction type from ID.
    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::Noop),
            1 => Some(Self::Swap),
            2 => Some(Self::Liquidate),
            3 => Some(Self::Transfer),
            4 => Some(Self::Approve),
            5 => Some(Self::FlashLoan),
            6 => Some(Self::FlashLoanRepay),
            7 => Some(Self::ProfitCheck),
            8 => Some(Self::CustomCall),
            _ => None,
        }
    }
}

/// Flash loan provider type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum FlashLoanProvider {
    /// AAVE V3 flash loan (most common)
    #[default]
    AaveV3 = 0,
    /// Balancer flash loan (no fee)
    Balancer = 1,
    /// Uniswap V3 flash swap
    UniswapV3 = 2,
    /// Custom flash loan provider
    Custom = 255,
}

impl FlashLoanProvider {
    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::AaveV3),
            1 => Some(Self::Balancer),
            2 => Some(Self::UniswapV3),
            255 => Some(Self::Custom),
            _ => None,
        }
    }
}

// ABI encoding helper types
sol! {
    /// Single instruction wrapper
    #[derive(Debug)]
    struct Instruction {
        uint8 instructionType;
        bytes data;
    }

    /// Instructions array wrapper
    #[derive(Debug)]
    struct Instructions {
        Instruction[] steps;
    }

    /// Swap instruction data
    #[derive(Debug)]
    struct SwapInstruction {
        uint8 adapterType;
        address tokenIn;
        address tokenOut;
        uint256 amountIn;
        uint256 minOut;
        bytes swapData;
    }

    /// Liquidate instruction data
    #[derive(Debug)]
    struct LiquidateInstruction {
        address pool;
        address user;
        address collateral;
        address debt;
        uint256 debtAmount;
        uint256 minCollateralOut;
    }

    /// Transfer instruction data
    #[derive(Debug)]
    struct TransferInstruction {
        address token;
        address to;
        uint256 amount;
        bool max;
    }

    /// Approve instruction data
    #[derive(Debug)]
    struct ApproveInstruction {
        address token;
        address spender;
        uint256 amount;
    }

    /// Flash loan instruction data
    #[derive(Debug)]
    struct FlashLoanInstruction {
        uint8 provider;
        address[] assets;
        uint256[] amounts;
    }

    /// Profit check instruction data
    #[derive(Debug)]
    struct ProfitCheckInstruction {
        address token;
        uint256 minProfit;
    }

    /// Custom call instruction data
    #[derive(Debug)]
    struct CustomCallInstruction {
        address target;
        bytes callData;
    }
}

/// Builder for constructing instruction sequences.
///
/// Provides a fluent API for building liquidation strategies.
#[derive(Debug, Default)]
pub struct InstructionBuilder {
    instructions: Vec<(InstructionType, Bytes)>,
}

impl InstructionBuilder {
    /// Create a new instruction builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a swap instruction.
    pub fn swap(
        mut self,
        adapter: SwapAdapter,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        min_out: U256,
        swap_data: Bytes,
    ) -> Self {
        let data = SwapInstruction {
            adapterType: adapter.id(),
            tokenIn: token_in,
            tokenOut: token_out,
            amountIn: amount_in,
            minOut: min_out,
            swapData: swap_data,
        };
        self.instructions.push((
            InstructionType::Swap,
            Bytes::from(SwapInstruction::abi_encode(&data)),
        ));
        self
    }

    /// Add a liquidation instruction.
    pub fn liquidate(
        mut self,
        pool: Address,
        user: Address,
        collateral: Address,
        debt: Address,
        debt_amount: U256,
        min_collateral_out: U256,
    ) -> Self {
        let data = LiquidateInstruction {
            pool,
            user,
            collateral,
            debt,
            debtAmount: debt_amount,
            minCollateralOut: min_collateral_out,
        };
        self.instructions.push((
            InstructionType::Liquidate,
            Bytes::from(LiquidateInstruction::abi_encode(&data)),
        ));
        self
    }

    /// Add a transfer instruction.
    pub fn transfer(mut self, token: Address, to: Address, amount: U256, max: bool) -> Self {
        let data = TransferInstruction {
            token,
            to,
            amount,
            max,
        };
        self.instructions.push((
            InstructionType::Transfer,
            Bytes::from(TransferInstruction::abi_encode(&data)),
        ));
        self
    }

    /// Add an approval instruction.
    pub fn approve(mut self, token: Address, spender: Address, amount: U256) -> Self {
        let data = ApproveInstruction {
            token,
            spender,
            amount,
        };
        self.instructions.push((
            InstructionType::Approve,
            Bytes::from(ApproveInstruction::abi_encode(&data)),
        ));
        self
    }

    /// Add a flash loan instruction.
    pub fn flash_loan(
        mut self,
        provider: FlashLoanProvider,
        assets: Vec<Address>,
        amounts: Vec<U256>,
    ) -> Self {
        let data = FlashLoanInstruction {
            provider: provider as u8,
            assets,
            amounts,
        };
        self.instructions.push((
            InstructionType::FlashLoan,
            Bytes::from(FlashLoanInstruction::abi_encode(&data)),
        ));
        self
    }

    /// Add a profit check instruction.
    pub fn profit_check(mut self, token: Address, min_profit: U256) -> Self {
        let data = ProfitCheckInstruction {
            token,
            minProfit: min_profit,
        };
        self.instructions.push((
            InstructionType::ProfitCheck,
            Bytes::from(ProfitCheckInstruction::abi_encode(&data)),
        ));
        self
    }

    /// Add a custom call instruction.
    pub fn custom_call(mut self, target: Address, call_data: Bytes) -> Self {
        let data = CustomCallInstruction {
            target,
            callData: call_data,
        };
        self.instructions.push((
            InstructionType::CustomCall,
            Bytes::from(CustomCallInstruction::abi_encode(&data)),
        ));
        self
    }

    /// Build the final encoded instructions.
    pub fn build(self) -> Bytes {
        let steps: Vec<Instruction> = self
            .instructions
            .into_iter()
            .map(|(typ, data)| Instruction {
                instructionType: typ as u8,
                data,
            })
            .collect();

        let instructions = Instructions { steps };
        Bytes::from(Instructions::abi_encode(&instructions))
    }

    /// Get the number of instructions.
    pub fn len(&self) -> usize {
        self.instructions.len()
    }

    /// Check if builder is empty.
    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }
}

/// Execution mode for liquidation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecutionMode {
    /// Use flash loan to borrow debt, liquidate, swap collateral, repay.
    /// No capital required, pays flash loan fee (~0.09% for AAVE).
    #[default]
    FlashLoan,

    /// Direct execution without flash loan.
    /// Requires bot to have debt token balance.
    Direct,
}

/// Pre-built liquidation strategy.
#[derive(Debug, Clone)]
pub struct LiquidationStrategy {
    /// Execution mode
    pub mode: ExecutionMode,
    /// Flash loan provider (if using flash loan mode)
    pub flash_provider: Option<FlashLoanProvider>,
    /// Encoded instructions
    pub instructions: Bytes,
    /// Expected profit (for validation)
    pub expected_profit: U256,
    /// Minimum profit threshold
    pub min_profit: U256,
}

/// Build a flash loan liquidation strategy.
///
/// Flow: Flash loan → Liquidate → Swap collateral → Repay → Keep profit
pub fn build_flash_loan_strategy(
    flash_provider: FlashLoanProvider,
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
    let instructions = InstructionBuilder::new()
        // 1. Flash loan the debt token
        .flash_loan(flash_provider, vec![debt], vec![debt_amount])
        // 2. Liquidate the position (use borrowed debt to repay borrower)
        .liquidate(pool, user, collateral, debt, debt_amount, min_collateral_out)
        // 3. Swap seized collateral back to debt token (for repayment + profit)
        .swap(
            swap_adapter,
            collateral,
            debt,
            U256::ZERO, // Use full balance
            debt_amount, // At least enough to repay
            swap_data,
        )
        // 4. Profit check (revert if not profitable)
        .profit_check(profit_token, min_profit)
        .build();

    LiquidationStrategy {
        mode: ExecutionMode::FlashLoan,
        flash_provider: Some(flash_provider),
        instructions,
        expected_profit: U256::ZERO, // To be calculated
        min_profit,
    }
}

/// Build a direct liquidation strategy (no flash loan).
///
/// Flow: Swap → Liquidate → Keep collateral
/// Requires: Bot has tokens to swap for debt repayment.
pub fn build_direct_strategy(
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
    let instructions = InstructionBuilder::new()
        // 1. Liquidate the position (must have debt token balance)
        .liquidate(pool, user, collateral, debt, debt_amount, min_collateral_out)
        // 2. Optionally swap collateral if needed
        .swap(
            swap_adapter,
            collateral,
            debt,
            U256::ZERO,
            U256::ZERO,
            swap_data,
        )
        // 3. Profit check
        .profit_check(profit_token, min_profit)
        .build();

    LiquidationStrategy {
        mode: ExecutionMode::Direct,
        flash_provider: None,
        instructions,
        expected_profit: U256::ZERO,
        min_profit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instruction_builder() {
        let instructions = InstructionBuilder::new()
            .flash_loan(FlashLoanProvider::AaveV3, vec![Address::ZERO], vec![U256::from(1000)])
            .liquidate(
                Address::ZERO,
                Address::ZERO,
                Address::ZERO,
                Address::ZERO,
                U256::from(1000),
                U256::ZERO,
            )
            .swap(
                SwapAdapter::UniswapV3,
                Address::ZERO,
                Address::ZERO,
                U256::from(1000),
                U256::from(900),
                Bytes::new(),
            )
            .profit_check(Address::ZERO, U256::from(10))
            .build();

        assert!(!instructions.is_empty());
    }

    #[test]
    fn test_instruction_types() {
        assert_eq!(InstructionType::from_id(0), Some(InstructionType::Noop));
        assert_eq!(InstructionType::from_id(1), Some(InstructionType::Swap));
        assert_eq!(InstructionType::from_id(2), Some(InstructionType::Liquidate));
        assert_eq!(InstructionType::from_id(5), Some(InstructionType::FlashLoan));
        assert_eq!(InstructionType::from_id(99), None);
    }

    #[test]
    fn test_flash_loan_provider() {
        assert_eq!(FlashLoanProvider::from_id(0), Some(FlashLoanProvider::AaveV3));
        assert_eq!(FlashLoanProvider::from_id(1), Some(FlashLoanProvider::Balancer));
        assert_eq!(FlashLoanProvider::from_id(255), Some(FlashLoanProvider::Custom));
        assert_eq!(FlashLoanProvider::from_id(50), None);
    }

    #[test]
    fn test_build_flash_loan_strategy() {
        let strategy = build_flash_loan_strategy(
            FlashLoanProvider::AaveV3,
            Address::ZERO,
            Address::ZERO,
            Address::ZERO,
            Address::ZERO,
            U256::from(1000),
            U256::from(900),
            SwapAdapter::UniswapV3,
            Bytes::new(),
            Address::ZERO,
            U256::from(10),
        );

        assert_eq!(strategy.mode, ExecutionMode::FlashLoan);
        assert_eq!(strategy.flash_provider, Some(FlashLoanProvider::AaveV3));
        assert!(!strategy.instructions.is_empty());
    }

    #[test]
    fn test_build_direct_strategy() {
        let strategy = build_direct_strategy(
            Address::ZERO,
            Address::ZERO,
            Address::ZERO,
            Address::ZERO,
            U256::from(1000),
            U256::from(900),
            SwapAdapter::UniswapV3,
            Bytes::new(),
            Address::ZERO,
            U256::from(10),
        );

        assert_eq!(strategy.mode, ExecutionMode::Direct);
        assert_eq!(strategy.flash_provider, None);
        assert!(!strategy.instructions.is_empty());
    }
}
