// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title ISwapRouter
/// @notice Minimal interface for Uniswap V3 SwapRouter02
/// @dev Used by UniswapV3Adapter for both single and multi-hop swaps
interface ISwapRouter {
    /// @notice Parameters for single-hop exact input swap
    struct ExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        uint24 fee;
        address recipient;
        uint256 amountIn;
        uint256 amountOutMinimum;
        uint160 sqrtPriceLimitX96;
    }

    /// @notice Executes a single-hop exact input swap
    /// @param params The swap parameters
    /// @return amountOut The amount of output tokens received
    function exactInputSingle(ExactInputSingleParams calldata params) external payable returns (uint256 amountOut);

    /// @notice Parameters for multi-hop exact input swap
    struct ExactInputParams {
        bytes path;
        address recipient;
        uint256 amountIn;
        uint256 amountOutMinimum;
    }

    /// @notice Executes a multi-hop exact input swap
    /// @param params The swap parameters with encoded path
    /// @return amountOut The amount of output tokens received
    function exactInput(ExactInputParams calldata params) external payable returns (uint256 amountOut);
}
