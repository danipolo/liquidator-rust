// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Script, console} from "forge-std/Script.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {UniswapV3Adapter} from "../src/adapters/UniswapV3Adapter.sol";
import {SwapDataDecoder} from "../src/libraries/SwapDataDecoder.sol";
import {IWETH} from "../src/interfaces/IWETH.sol";

/// @title TestSwap
/// @notice Script to test a real swap using the deployed UniswapV3Adapter
contract TestSwap is Script {
    // Arbitrum mainnet addresses
    address constant WETH = 0x82aF49447D8a07e3bd95BD0d56f35241523fBab1;
    address constant USDC = 0xaf88d065e77c8cC2239327C5EDb3A432268e5831;

    // Deployed adapter address
    address constant UNISWAP_ADAPTER = 0x7845144188DEd8667Dcb6500E44A55e50c264589;

    function run() external {
        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");
        address deployer = vm.addr(deployerPrivateKey);

        console.log("Testing swap with deployer:", deployer);
        console.log("ETH balance:", deployer.balance / 1e15, "milliETH");

        uint256 amountIn = 0.001 ether; // Swap 0.001 WETH
        uint256 minAmountOut = 1e6; // Expect at least 1 USDC

        // Check if we have enough ETH (need extra for gas)
        require(deployer.balance >= amountIn + 0.0005 ether, "Not enough ETH");

        vm.startBroadcast(deployerPrivateKey);

        // 1. Wrap ETH to WETH
        IWETH(WETH).deposit{value: amountIn}();
        console.log("Wrapped", amountIn / 1e15, "milliETH to WETH");

        // 2. Transfer WETH to adapter
        IERC20(WETH).transfer(UNISWAP_ADAPTER, amountIn);
        console.log("Transferred WETH to adapter");

        // 3. Create swap data (single-hop, 0.05% fee tier)
        bytes memory swapData = SwapDataDecoder.encodeUniswapV3Data(false, abi.encode(uint24(500)));

        // 4. Execute swap
        uint256 usdcBefore = IERC20(USDC).balanceOf(deployer);

        UniswapV3Adapter adapter = UniswapV3Adapter(UNISWAP_ADAPTER);
        uint256 amountOut = adapter.swap(WETH, USDC, amountIn, minAmountOut, swapData);

        uint256 usdcAfter = IERC20(USDC).balanceOf(deployer);

        vm.stopBroadcast();

        console.log("Swap successful!");
        console.log("  Amount in:", amountIn / 1e15, "milliWETH");
        console.log("  Amount out:", amountOut / 1e6, "USDC");
        console.log("  USDC received:", (usdcAfter - usdcBefore) / 1e6, "USDC");
    }
}
