// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {ISwapAdapter} from "../interfaces/ISwapAdapter.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";

/// @title DirectAdapter
/// @notice No-op passthrough adapter for when no swap is needed
/// @dev Used when collateral and debt tokens are the same, or when tokens are already in the correct form
contract DirectAdapter is ISwapAdapter {
    using SafeERC20 for IERC20;

    /// @dev Thrown when tokenIn and tokenOut are different (swap would be needed)
    error TokenMismatch(address tokenIn, address tokenOut);

    /// @dev Thrown when output amount is less than minimum
    error InsufficientOutput(uint256 amountOut, uint256 minAmountOut);

    /// @inheritdoc ISwapAdapter
    /// @notice Passes through tokens without swapping
    /// @dev Reverts if tokenIn != tokenOut since no actual swap can occur
    function swap(
        address tokenIn,
        address tokenOut,
        uint256 amountIn,
        uint256 minAmountOut,
        bytes calldata /* data */
    ) external override returns (uint256 amountOut) {
        // Direct adapter only works when no swap is needed
        if (tokenIn != tokenOut) {
            revert TokenMismatch(tokenIn, tokenOut);
        }

        // Transfer tokens from caller to this contract, then back
        // In practice, the Liquidator will handle token transfers
        amountOut = amountIn;

        if (amountOut < minAmountOut) {
            revert InsufficientOutput(amountOut, minAmountOut);
        }

        // Transfer tokens back to caller
        IERC20(tokenOut).safeTransfer(msg.sender, amountOut);

        return amountOut;
    }
}
