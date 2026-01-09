// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {ISwapAdapter} from "../interfaces/ISwapAdapter.sol";
import {ISwapRouter} from "../interfaces/ISwapRouter.sol";
import {SwapDataDecoder} from "../libraries/SwapDataDecoder.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";

/// @title UniswapV3Adapter
/// @notice Adapter for Uniswap V3 SwapRouter02
/// @dev Supports both single-hop and multi-hop swaps on Arbitrum, Base, Optimism
contract UniswapV3Adapter is ISwapAdapter {
    using SafeERC20 for IERC20;

    /// @notice The Uniswap V3 SwapRouter02 address
    ISwapRouter public immutable router;

    /// @dev Thrown when output amount is less than minimum
    error InsufficientOutput(uint256 amountOut, uint256 minAmountOut);

    /// @notice Constructs the adapter with the SwapRouter02 address
    /// @param _router The Uniswap V3 SwapRouter02 contract address
    constructor(address _router) {
        router = ISwapRouter(_router);
    }

    /// @inheritdoc ISwapAdapter
    /// @notice Executes a swap through Uniswap V3
    /// @dev Decodes data as UniswapV3SwapData to determine single or multi-hop
    function swap(
        address tokenIn,
        address tokenOut,
        uint256 amountIn,
        uint256 minAmountOut,
        bytes calldata data
    ) external override returns (uint256 amountOut) {
        // Decode the UniswapV3-specific data
        (bool isMultiHop, bytes memory pathOrFee) = SwapDataDecoder.decodeUniswapV3Data(data);

        // Approve router to spend tokens
        IERC20(tokenIn).forceApprove(address(router), amountIn);

        if (isMultiHop) {
            // Multi-hop swap using packed path
            amountOut = router.exactInput(
                ISwapRouter.ExactInputParams({
                    path: pathOrFee,
                    recipient: address(this),
                    amountIn: amountIn,
                    amountOutMinimum: minAmountOut
                })
            );
        } else {
            // Single-hop swap using fee tier
            uint24 fee = abi.decode(pathOrFee, (uint24));

            amountOut = router.exactInputSingle(
                ISwapRouter.ExactInputSingleParams({
                    tokenIn: tokenIn,
                    tokenOut: tokenOut,
                    fee: fee,
                    recipient: address(this),
                    amountIn: amountIn,
                    amountOutMinimum: minAmountOut,
                    sqrtPriceLimitX96: 0 // No price limit
                })
            );
        }

        if (amountOut < minAmountOut) {
            revert InsufficientOutput(amountOut, minAmountOut);
        }

        // Transfer output tokens back to caller
        IERC20(tokenOut).safeTransfer(msg.sender, amountOut);

        return amountOut;
    }
}
