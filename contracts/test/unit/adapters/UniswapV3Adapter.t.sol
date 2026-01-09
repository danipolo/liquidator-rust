// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test, console} from "forge-std/Test.sol";
import {UniswapV3Adapter} from "../../../src/adapters/UniswapV3Adapter.sol";
import {ISwapRouter} from "../../../src/interfaces/ISwapRouter.sol";
import {SwapDataDecoder} from "../../../src/libraries/SwapDataDecoder.sol";
import {MockERC20} from "../../utils/MockContracts.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";

/// @title MockUniswapRouter
/// @notice Mock SwapRouter02 for testing UniswapV3Adapter
contract MockUniswapRouter is ISwapRouter {
    uint256 public swapRate = 1e18;
    bool public shouldFail;

    function setSwapRate(uint256 rate) external {
        swapRate = rate;
    }

    function setShouldFail(bool fail) external {
        shouldFail = fail;
    }

    function exactInputSingle(ExactInputSingleParams calldata params)
        external
        payable
        override
        returns (uint256 amountOut)
    {
        require(!shouldFail, "swap failed");

        // Transfer input tokens from caller
        IERC20(params.tokenIn).transferFrom(msg.sender, address(this), params.amountIn);

        // Calculate output
        amountOut = (params.amountIn * swapRate) / 1e18;

        require(amountOut >= params.amountOutMinimum, "insufficient output");

        // Mint output tokens
        MockERC20(params.tokenOut).mint(params.recipient, amountOut);

        return amountOut;
    }

    function exactInput(ExactInputParams calldata params) external payable override returns (uint256 amountOut) {
        require(!shouldFail, "swap failed");

        // Decode path to get tokenIn and tokenOut
        // Path format: tokenIn (20 bytes) + fee (3 bytes) + tokenOut (20 bytes) [+ fee + token...]
        bytes memory path = params.path;
        address tokenIn;
        address tokenOut;

        // Extract tokenIn from first 20 bytes
        assembly {
            tokenIn := mload(add(path, 20))
        }

        // Extract tokenOut from last 20 bytes
        uint256 pathLen = path.length;
        assembly {
            tokenOut := mload(add(path, pathLen))
        }

        // Transfer input tokens from caller
        IERC20(tokenIn).transferFrom(msg.sender, address(this), params.amountIn);

        // Calculate output
        amountOut = (params.amountIn * swapRate) / 1e18;

        require(amountOut >= params.amountOutMinimum, "insufficient output");

        // Mint output tokens
        MockERC20(tokenOut).mint(params.recipient, amountOut);

        return amountOut;
    }
}

contract UniswapV3AdapterTest is Test {
    UniswapV3Adapter public adapter;
    MockUniswapRouter public router;
    MockERC20 public tokenIn;
    MockERC20 public tokenOut;
    MockERC20 public tokenMid;

    address public caller;

    function setUp() external {
        caller = makeAddr("caller");

        // Deploy mock contracts
        router = new MockUniswapRouter();
        tokenIn = new MockERC20("Token In", "TIN", 18);
        tokenOut = new MockERC20("Token Out", "TOUT", 18);
        tokenMid = new MockERC20("Token Mid", "TMID", 18);

        // Deploy adapter
        adapter = new UniswapV3Adapter(address(router));

        // Mint tokens to caller
        tokenIn.mint(caller, 1000 ether);
    }

    function test_Swap_SingleHop_Success() external {
        uint256 amountIn = 100 ether;
        uint256 minAmountOut = 90 ether;

        // Create single-hop swap data
        bytes memory swapData = SwapDataDecoder.encodeUniswapV3Data(false, abi.encode(uint24(3000)));

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

    function test_Swap_SingleHop_WithDifferentRate() external {
        // Set 110% swap rate
        router.setSwapRate(1.1e18);

        uint256 amountIn = 100 ether;
        uint256 minAmountOut = 100 ether;

        // Create single-hop swap data
        bytes memory swapData = SwapDataDecoder.encodeUniswapV3Data(false, abi.encode(uint24(3000)));

        // Transfer tokens to adapter
        vm.startPrank(caller);
        tokenIn.transfer(address(adapter), amountIn);

        // Execute swap
        uint256 amountOut = adapter.swap(address(tokenIn), address(tokenOut), amountIn, minAmountOut, swapData);
        vm.stopPrank();

        // Verify output (110% of input)
        assertEq(amountOut, 110 ether);
    }

    function test_Swap_MultiHop_Success() external {
        uint256 amountIn = 100 ether;
        uint256 minAmountOut = 90 ether;

        // Create multi-hop path: tokenIn -> tokenMid -> tokenOut
        // Path encoding: token (20 bytes) + fee (3 bytes) + token (20 bytes) + fee (3 bytes) + token (20 bytes)
        bytes memory path =
            abi.encodePacked(address(tokenIn), uint24(3000), address(tokenMid), uint24(500), address(tokenOut));

        // Create multi-hop swap data
        bytes memory swapData = SwapDataDecoder.encodeUniswapV3Data(true, path);

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

    function test_Swap_SingleHop_RevertWhen_InsufficientOutput() external {
        // Set 80% swap rate
        router.setSwapRate(0.8e18);

        uint256 amountIn = 100 ether;
        uint256 minAmountOut = 90 ether;

        // Create single-hop swap data
        bytes memory swapData = SwapDataDecoder.encodeUniswapV3Data(false, abi.encode(uint24(3000)));

        // Transfer tokens to adapter
        vm.startPrank(caller);
        tokenIn.transfer(address(adapter), amountIn);

        // Execute swap - should revert
        vm.expectRevert("insufficient output");
        adapter.swap(address(tokenIn), address(tokenOut), amountIn, minAmountOut, swapData);
        vm.stopPrank();
    }

    function test_Swap_DifferentFeeTiers() external {
        uint256 amountIn = 100 ether;
        uint256 minAmountOut = 90 ether;

        // Test different fee tiers
        uint24[4] memory feeTiers = [uint24(100), uint24(500), uint24(3000), uint24(10000)];

        for (uint256 i = 0; i < feeTiers.length; i++) {
            // Reset token balances
            tokenIn.mint(caller, amountIn);

            // Create swap data with different fee tier
            bytes memory swapData = SwapDataDecoder.encodeUniswapV3Data(false, abi.encode(feeTiers[i]));

            // Transfer tokens to adapter
            vm.startPrank(caller);
            tokenIn.transfer(address(adapter), amountIn);

            // Execute swap
            uint256 amountOut = adapter.swap(address(tokenIn), address(tokenOut), amountIn, minAmountOut, swapData);
            vm.stopPrank();

            // Verify output
            assertEq(amountOut, 100 ether);
        }
    }

    function test_Router_ReturnsCorrectAddress() external view {
        assertEq(address(adapter.router()), address(router));
    }
}
