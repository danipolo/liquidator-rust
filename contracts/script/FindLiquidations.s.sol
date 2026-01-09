// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Script, console} from "forge-std/Script.sol";

interface IPool {
    event LiquidationCall(
        address indexed collateralAsset,
        address indexed debtAsset,
        address indexed user,
        uint256 debtToCover,
        uint256 liquidatedCollateralAmount,
        address liquidator,
        bool receiveAToken
    );

    function getUserAccountData(address user) external view returns (
        uint256 totalCollateralBase,
        uint256 totalDebtBase,
        uint256 availableBorrowsBase,
        uint256 currentLiquidationThreshold,
        uint256 ltv,
        uint256 healthFactor
    );
}

/// @title FindLiquidations
/// @notice Script to find liquidatable positions or recent liquidation events
contract FindLiquidations is Script {
    address constant AAVE_POOL = 0x794a61358D6845594F94dc1DB02A252b5b4814aD;

    // Some known large position holders on Arbitrum AAVE (from public data)
    address[] public testUsers;

    function run() external view {
        console.log("=== Checking Known Positions ===");
        console.log("AAVE Pool:", AAVE_POOL);
        console.log("");

        // Add some test addresses - these are public from AAVE governance/explorers
        address[5] memory users = [
            0x1f79618e870fd5b5C3320106cb368125723B6245, // Your address
            0x489ee077994B6658eAfA855C308275EAd8097C4A, // Known whale
            0x0000000000000000000000000000000000000001, // Test
            0x0000000000000000000000000000000000000002, // Test
            0x0000000000000000000000000000000000000003  // Test
        ];

        IPool pool = IPool(AAVE_POOL);

        for (uint i = 0; i < users.length; i++) {
            if (users[i] == address(0)) continue;

            try pool.getUserAccountData(users[i]) returns (
                uint256 collateral,
                uint256 debt,
                uint256,
                uint256,
                uint256,
                uint256 hf
            ) {
                if (debt > 0) {
                    console.log("---");
                    console.log("User:", users[i]);
                    console.log("Collateral (USD):", collateral / 1e8);
                    console.log("Debt (USD):", debt / 1e8);
                    console.log("Health Factor:", hf / 1e16, "%");
                    if (hf < 1e18) {
                        console.log(">>> LIQUIDATABLE! <<<");
                    }
                }
            } catch {
                // Skip invalid addresses
            }
        }
    }

    /// @notice Check specific user
    function checkUser(address user) external view {
        IPool pool = IPool(AAVE_POOL);
        (uint256 collateral, uint256 debt,,,, uint256 hf) = pool.getUserAccountData(user);

        console.log("User:", user);
        console.log("Collateral (USD):", collateral / 1e8);
        console.log("Debt (USD):", debt / 1e8);
        console.log("Health Factor:", hf);
        console.log("Health Factor %:", hf / 1e16);
        console.log("Liquidatable:", hf < 1e18 ? "YES" : "NO");
    }
}
