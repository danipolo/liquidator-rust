// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {ISwapAdapter} from "../interfaces/ISwapAdapter.sol";
import {ILiquidSwap} from "../interfaces/ILiquidSwap.sol";
import {SwapDataDecoder} from "../libraries/SwapDataDecoder.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";

/// @title LiquidSwapAdapter
/// @notice Adapter for LiquidSwap DEX on HyperLiquid
/// @dev Wraps the ILiquidSwap interface to implement ISwapAdapter
contract LiquidSwapAdapter is ISwapAdapter {
    using SafeERC20 for IERC20;

    /// @notice The LiquidSwap router address
    ILiquidSwap public immutable router;

    /// @dev Thrown when output amount is less than minimum
    error InsufficientOutput(uint256 amountOut, uint256 minAmountOut);

    /// @notice Constructs the adapter with the LiquidSwap router address
    /// @param _router The LiquidSwap router contract address
    constructor(address _router) {
        router = ILiquidSwap(_router);
    }

    /// @inheritdoc ISwapAdapter
    /// @notice Executes a swap through LiquidSwap
    /// @dev Decodes the data parameter as LiquidSwapData containing tokens and hops
    function swap(
        address tokenIn,
        address tokenOut,
        uint256 amountIn,
        uint256 minAmountOut,
        bytes calldata data
    ) external override returns (uint256 amountOut) {
        // Decode the LiquidSwap-specific data
        (address[] memory tokens, ILiquidSwap.Swap[][] memory hops) = SwapDataDecoder.decodeLiquidSwapData(data);

        // Adjust hop amounts if actual balance differs from expected
        // This handles cases where collateral received varies slightly
        uint256 inputAmountFromHops = 0;
        for (uint256 i = 0; i < hops[0].length; i++) {
            inputAmountFromHops += hops[0][i].amountIn;
        }

        if (inputAmountFromHops > amountIn) {
            uint256 diff = inputAmountFromHops - amountIn;
            hops[0][0].amountIn -= diff;
        } else if (amountIn > inputAmountFromHops) {
            uint256 diff = amountIn - inputAmountFromHops;
            hops[0][0].amountIn += diff;
        }

        // Approve router to spend tokens
        IERC20(tokenIn).forceApprove(address(router), amountIn);

        // Execute the multi-hop swap
        amountOut = router.executeMultiHopSwap(tokens, amountIn, minAmountOut, hops);

        if (amountOut < minAmountOut) {
            revert InsufficientOutput(amountOut, minAmountOut);
        }

        // Transfer output tokens back to caller
        IERC20(tokenOut).safeTransfer(msg.sender, amountOut);

        return amountOut;
    }
}
