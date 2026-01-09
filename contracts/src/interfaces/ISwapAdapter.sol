// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title ISwapAdapter
/// @notice Interface for pluggable DEX adapters
/// @dev All adapters must implement this interface for compatibility with Liquidator
interface ISwapAdapter {
    /// @notice Executes a swap from tokenIn to tokenOut
    /// @dev Tokens must be approved before calling. Adapter handles routing logic.
    /// @param tokenIn Input token address
    /// @param tokenOut Output token address
    /// @param amountIn Amount of input tokens
    /// @param minAmountOut Minimum output tokens (slippage protection)
    /// @param data Adapter-specific encoded parameters
    /// @return amountOut Actual output amount received
    function swap(
        address tokenIn,
        address tokenOut,
        uint256 amountIn,
        uint256 minAmountOut,
        bytes calldata data
    ) external returns (uint256 amountOut);
}
