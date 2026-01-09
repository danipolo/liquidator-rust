// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test, console} from "forge-std/Test.sol";
import {LiquidSwapAdapter} from "../../../src/adapters/LiquidSwapAdapter.sol";
import {ILiquidSwap} from "../../../src/interfaces/ILiquidSwap.sol";
import {SwapDataDecoder} from "../../../src/libraries/SwapDataDecoder.sol";
import {MockERC20} from "../../utils/MockContracts.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";

/// @title MockLiquidSwapRouter
/// @notice Mock router for testing LiquidSwapAdapter
contract MockLiquidSwapRouter is ILiquidSwap {
    uint256 public swapRate = 1e18;
    bool public shouldFail;

    function setSwapRate(uint256 rate) external {
        swapRate = rate;
    }

    function setShouldFail(bool fail) external {
        shouldFail = fail;
    }

    function executeMultiHopSwap(
        address[] calldata tokens,
        uint256 amountIn,
        uint256 minAmountOut,
        Swap[][] calldata /* hops */
    ) external payable override returns (uint256 totalAmountOut) {
        require(!shouldFail, "swap failed");

        // Get input and output tokens
        address tokenIn = tokens[0];
        address tokenOut = tokens[tokens.length - 1];

        // Transfer input tokens from caller
        IERC20(tokenIn).transferFrom(msg.sender, address(this), amountIn);

        // Calculate output
        totalAmountOut = (amountIn * swapRate) / 1e18;

        require(totalAmountOut >= minAmountOut, "insufficient output");

        // Mint output tokens to this contract (router behavior)
        MockERC20(tokenOut).mint(address(this), totalAmountOut);

        // Transfer output tokens to caller
        IERC20(tokenOut).transfer(msg.sender, totalAmountOut);

        return totalAmountOut;
    }
}

contract LiquidSwapAdapterTest is Test {
    LiquidSwapAdapter public adapter;
    MockLiquidSwapRouter public router;
    MockERC20 public tokenIn;
    MockERC20 public tokenOut;

    address public caller;

    function setUp() external {
        caller = makeAddr("caller");

        // Deploy mock contracts
        router = new MockLiquidSwapRouter();
        tokenIn = new MockERC20("Token In", "TIN", 18);
        tokenOut = new MockERC20("Token Out", "TOUT", 18);

        // Deploy adapter
        adapter = new LiquidSwapAdapter(address(router));

        // Mint tokens to caller
        tokenIn.mint(caller, 1000 ether);
    }

    function test_Swap_Success() external {
        uint256 amountIn = 100 ether;
        uint256 minAmountOut = 90 ether;

        // Create swap data
        address[] memory tokens = new address[](2);
        tokens[0] = address(tokenIn);
        tokens[1] = address(tokenOut);

        ILiquidSwap.Swap[][] memory hops = new ILiquidSwap.Swap[][](1);
        hops[0] = new ILiquidSwap.Swap[](1);
        hops[0][0] = ILiquidSwap.Swap({
            tokenIn: address(tokenIn),
            tokenOut: address(tokenOut),
            routerIndex: 1,
            fee: 3000,
            amountIn: amountIn,
            stable: false
        });

        bytes memory swapData = SwapDataDecoder.encodeLiquidSwapData(tokens, hops);

        // Transfer tokens to adapter
        vm.startPrank(caller);
        tokenIn.transfer(address(adapter), amountIn);

        // Execute swap
        uint256 amountOut = adapter.swap(address(tokenIn), address(tokenOut), amountIn, minAmountOut, swapData);
        vm.stopPrank();

        // Verify output
        assertEq(amountOut, 100 ether);
        assertEq(tokenOut.balanceOf(caller), 100 ether);
    }

    function test_Swap_WithDifferentRate() external {
        // Set 110% swap rate
        router.setSwapRate(1.1e18);

        uint256 amountIn = 100 ether;
        uint256 minAmountOut = 100 ether;

        // Create swap data
        address[] memory tokens = new address[](2);
        tokens[0] = address(tokenIn);
        tokens[1] = address(tokenOut);

        ILiquidSwap.Swap[][] memory hops = new ILiquidSwap.Swap[][](1);
        hops[0] = new ILiquidSwap.Swap[](1);
        hops[0][0] = ILiquidSwap.Swap({
            tokenIn: address(tokenIn),
            tokenOut: address(tokenOut),
            routerIndex: 1,
            fee: 3000,
            amountIn: amountIn,
            stable: false
        });

        bytes memory swapData = SwapDataDecoder.encodeLiquidSwapData(tokens, hops);

        // Transfer tokens to adapter
        vm.startPrank(caller);
        tokenIn.transfer(address(adapter), amountIn);

        // Execute swap
        uint256 amountOut = adapter.swap(address(tokenIn), address(tokenOut), amountIn, minAmountOut, swapData);
        vm.stopPrank();

        // Verify output (110% of input)
        assertEq(amountOut, 110 ether);
    }

    function test_Swap_AdjustsHopAmounts() external {
        uint256 amountIn = 100 ether;
        uint256 minAmountOut = 90 ether;

        // Create swap data with different amountIn in hops
        address[] memory tokens = new address[](2);
        tokens[0] = address(tokenIn);
        tokens[1] = address(tokenOut);

        ILiquidSwap.Swap[][] memory hops = new ILiquidSwap.Swap[][](1);
        hops[0] = new ILiquidSwap.Swap[](1);
        hops[0][0] = ILiquidSwap.Swap({
            tokenIn: address(tokenIn),
            tokenOut: address(tokenOut),
            routerIndex: 1,
            fee: 3000,
            amountIn: 110 ether, // Different from actual amountIn
            stable: false
        });

        bytes memory swapData = SwapDataDecoder.encodeLiquidSwapData(tokens, hops);

        // Transfer tokens to adapter
        vm.startPrank(caller);
        tokenIn.transfer(address(adapter), amountIn);

        // Execute swap - adapter should adjust hop amounts
        uint256 amountOut = adapter.swap(address(tokenIn), address(tokenOut), amountIn, minAmountOut, swapData);
        vm.stopPrank();

        // Verify swap succeeded
        assertEq(amountOut, 100 ether);
    }

    function test_Swap_RevertWhen_InsufficientOutput() external {
        // Set 80% swap rate
        router.setSwapRate(0.8e18);

        uint256 amountIn = 100 ether;
        uint256 minAmountOut = 90 ether; // Expecting 90, but will get 80

        // Create swap data
        address[] memory tokens = new address[](2);
        tokens[0] = address(tokenIn);
        tokens[1] = address(tokenOut);

        ILiquidSwap.Swap[][] memory hops = new ILiquidSwap.Swap[][](1);
        hops[0] = new ILiquidSwap.Swap[](1);
        hops[0][0] = ILiquidSwap.Swap({
            tokenIn: address(tokenIn),
            tokenOut: address(tokenOut),
            routerIndex: 1,
            fee: 3000,
            amountIn: amountIn,
            stable: false
        });

        bytes memory swapData = SwapDataDecoder.encodeLiquidSwapData(tokens, hops);

        // Transfer tokens to adapter
        vm.startPrank(caller);
        tokenIn.transfer(address(adapter), amountIn);

        // Execute swap - should revert
        vm.expectRevert("insufficient output");
        adapter.swap(address(tokenIn), address(tokenOut), amountIn, minAmountOut, swapData);
        vm.stopPrank();
    }

    function test_Router_ReturnsCorrectAddress() external view {
        assertEq(address(adapter.router()), address(router));
    }
}
