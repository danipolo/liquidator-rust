//! Contract bindings generated from Foundry JSON artifacts.
//!
//! These bindings are loaded directly from the compiled Solidity contracts,
//! ensuring the Rust types always match the deployed contract ABIs.
//!
//! # Usage
//!
//! ```rust,ignore
//! use liquidator_chain::contracts::bindings::{ILiquidator, ISwapAdapter};
//!
//! // Create contract instance for RPC calls
//! let liquidator = ILiquidator::new(address, provider);
//! let profit = liquidator.liquidate(user, collateral, debt, amount, min_out, swap_data).call().await?;
//! ```
//!
//! # Regenerating Bindings
//!
//! Bindings are automatically regenerated when you compile. To update:
//! 1. Make changes to Solidity contracts in `contracts/src/`
//! 2. Run `forge build` in the `contracts/` directory
//! 3. Rebuild the Rust project with `cargo build`

use alloy::sol;

// ============================================================================
// Core Liquidator Contract
// ============================================================================

// Interface only (for calling deployed contracts)
sol!(
    #[sol(rpc)]
    #[derive(Debug)]
    ILiquidator,
    "../../contracts/out/ILiquidator.sol/ILiquidator.json"
);

// ============================================================================
// Swap Adapters
// ============================================================================

sol!(
    #[sol(rpc)]
    #[derive(Debug)]
    ISwapAdapter,
    "../../contracts/out/ISwapAdapter.sol/ISwapAdapter.json"
);

// ============================================================================
// External Protocol Interfaces
// ============================================================================

sol!(
    #[sol(rpc)]
    #[derive(Debug)]
    IPool,
    "../../contracts/out/IPool.sol/IPool.json"
);

sol!(
    #[sol(rpc)]
    #[derive(Debug)]
    ILiquidSwap,
    "../../contracts/out/ILiquidSwap.sol/ILiquidSwap.json"
);

sol!(
    #[sol(rpc)]
    #[derive(Debug)]
    ISwapRouter,
    "../../contracts/out/ISwapRouter.sol/ISwapRouter.json"
);

sol!(
    #[sol(rpc)]
    #[derive(Debug)]
    IWETH,
    "../../contracts/out/IWETH.sol/IWETH.json"
);

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::{Address, Bytes, U256};
    use alloy::sol_types::SolCall;

    #[test]
    fn test_iliquidator_bindings() {
        // Verify the generated types compile and have expected methods
        let call = ILiquidator::liquidateCall {
            user: Address::ZERO,
            collateral: Address::ZERO,
            debt: Address::ZERO,
            debtAmount: U256::ZERO,
            minAmountOut: U256::ZERO,
            swapData: Bytes::new(),
        };

        // Encode to verify ABI encoding works
        let encoded = call.abi_encode();
        assert!(!encoded.is_empty());
    }

    #[test]
    fn test_iswap_adapter_bindings() {
        let call = ISwapAdapter::swapCall {
            tokenIn: Address::ZERO,
            tokenOut: Address::ZERO,
            amountIn: U256::ZERO,
            minAmountOut: U256::ZERO,
            data: Bytes::new(),
        };

        let encoded = call.abi_encode();
        assert!(!encoded.is_empty());
    }

    #[test]
    fn test_selector_matches_contract() {
        // Verify our bindings produce the expected selector from the compiled contract
        // Expected: "liquidate(address,address,address,uint256,uint256,bytes)": "f3cf6097"
        let selector = ILiquidator::liquidateCall::SELECTOR;
        assert_eq!(
            hex::encode(selector),
            "f3cf6097",
            "Liquidate selector mismatch! Bot will send incorrect calldata."
        );

        // rescueTokens(address,uint256,bool,address): c25fac10
        let rescue_selector = ILiquidator::rescueTokensCall::SELECTOR;
        assert_eq!(
            hex::encode(rescue_selector),
            "c25fac10",
            "RescueTokens selector mismatch!"
        );

        // setAdapter(uint8,address): 5d86123c
        let set_adapter_selector = ILiquidator::setAdapterCall::SELECTOR;
        assert_eq!(
            hex::encode(set_adapter_selector),
            "5d86123c",
            "SetAdapter selector mismatch!"
        );
    }

    #[test]
    fn test_bindings_match_inline_definitions() {
        // Verify that bindings from JSON produce identical encoding as inline sol! definitions
        use crate::contracts::aave_v3;

        let user = Address::ZERO;
        let collateral = Address::ZERO;
        let debt = Address::ZERO;
        let amount = U256::from(1000);
        let min_out = U256::from(900);
        let swap_data = Bytes::from(vec![1, 2, 3, 4]);

        // Encode using inline definition (aave_v3.rs)
        let inline_encoded = aave_v3::encode_liquidation(
            user, collateral, debt, amount, min_out, swap_data.clone()
        );

        // Encode using JSON bindings
        let call = ILiquidator::liquidateCall {
            user,
            collateral,
            debt,
            debtAmount: amount,
            minAmountOut: min_out,
            swapData: swap_data,
        };
        let bindings_encoded = Bytes::from(call.abi_encode());

        assert_eq!(
            inline_encoded, bindings_encoded,
            "Encoding mismatch between inline sol! and JSON bindings! \
             This would cause transaction failures."
        );
    }
}
