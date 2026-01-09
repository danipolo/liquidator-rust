// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Script, console} from "forge-std/Script.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {Liquidator} from "../src/Liquidator.sol";
import {SwapDataDecoder} from "../src/libraries/SwapDataDecoder.sol";
import {TestConstants} from "../test/utils/TestConstants.sol";

interface IPool {
    function getUserAccountData(address user) external view returns (
        uint256 totalCollateralBase,
        uint256 totalDebtBase,
        uint256 availableBorrowsBase,
        uint256 currentLiquidationThreshold,
        uint256 ltv,
        uint256 healthFactor
    );
}

/// @title TestFlashLoan
/// @notice Script to test flash loan liquidation using the deployed Liquidator
contract TestFlashLoan is Script {
    // Arbitrum mainnet addresses
    address constant AAVE_POOL = 0x794a61358D6845594F94dc1DB02A252b5b4814aD;
    address constant WETH = 0x82aF49447D8a07e3bd95BD0d56f35241523fBab1;
    address constant USDC = 0xaf88d065e77c8cC2239327C5EDb3A432268e5831;

    // Deployed Liquidator address
    address constant LIQUIDATOR = 0x1EeB15C62ABCbaA8F57703BADd39Ae14Ab5adEd6;

    function run() external {
        console.log("=== Flash Loan Liquidation Test ===");
        console.log("Liquidator:", LIQUIDATOR);
        console.log("");
        console.log("To liquidate a position:");
        console.log("1. Find underwater position (health factor < 1.0)");
        console.log("2. Run: forge script TestFlashLoan --sig 'liquidate(address,address,address)' <user> <collateral> <debt>");
        console.log("");
        console.log("To check a position:");
        console.log("Run: forge script TestFlashLoan --sig 'check(address)' <user>");
    }

    /// @notice Check a user's position health
    function check(address user) external view {
        (,,,,,uint256 hf) = IPool(AAVE_POOL).getUserAccountData(user);
        console.log("User:", user);
        console.log("Health Factor:", hf);
        console.log("Liquidatable:", hf < 1e18 ? "YES" : "NO");
    }

    /// @notice Execute a liquidation
    function liquidate(address user, address collateral, address debt) external {
        uint256 pk = vm.envUint("PRIVATE_KEY");

        // Check health factor first
        (,,,,,uint256 hf) = IPool(AAVE_POOL).getUserAccountData(user);
        console.log("Health Factor:", hf);
        require(hf < 1e18, "Position is healthy, cannot liquidate");

        // Create swap data: collateral -> debt via Uniswap V3 (0.05% fee)
        bytes memory uniData = SwapDataDecoder.encodeUniswapV3Data(false, abi.encode(uint24(500)));
        bytes memory swapData = SwapDataDecoder.encodeWrappedSwapData(TestConstants.ADAPTER_UNISWAP_V3, uniData);

        vm.startBroadcast(pk);

        uint256 profit = Liquidator(payable(LIQUIDATOR)).liquidate(
            user,
            collateral,
            debt,
            type(uint256).max, // 50% of debt
            0,                  // minAmountOut (set properly in production!)
            swapData
        );

        vm.stopBroadcast();

        console.log("Liquidation successful! Profit:", profit);
    }
}
