// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title ILiquidSwap
/// @notice Interface for LiquidSwap DEX on HyperLiquid
/// @dev Used by LiquidSwapAdapter to execute multi-hop swaps
interface ILiquidSwap {
    /// @notice Swap allocation structure for multi-hop routing
    /// @param tokenIn Input token address
    /// @param tokenOut Output token address
    /// @param routerIndex Router to use: 1=KittenSwap, 2=HyperSwapV2, 3=HyperSwapV3, 4=Laminar, 5=KittenSwapV3
    /// @param fee Fee tier (only used for HyperSwapV3 and Laminar)
    /// @param amountIn Input amount for exact input swaps, or output amount for exact output
    /// @param stable Whether the pool is stable (only used for KittenSwap)
    struct Swap {
        address tokenIn;
        address tokenOut;
        uint8 routerIndex;
        uint24 fee;
        uint256 amountIn;
        bool stable;
    }

    /// @notice Executes a multi-hop swap through multiple DEX routers
    /// @param tokens Array of token addresses in the swap path
    /// @param amountIn Total input amount
    /// @param minAmountOut Minimum output amount (slippage protection)
    /// @param hops Array of swap allocations per hop
    /// @return totalAmountOut The total output amount received
    function executeMultiHopSwap(
        address[] calldata tokens,
        uint256 amountIn,
        uint256 minAmountOut,
        Swap[][] calldata hops
    ) external payable returns (uint256 totalAmountOut);
}
