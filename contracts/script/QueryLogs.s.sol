// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Script, console} from "forge-std/Script.sol";

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

/// @title QueryLogs
/// @notice Use Foundry fork to find liquidation events
contract QueryLogs is Script {
    address constant AAVE_POOL = 0x794a61358D6845594F94dc1DB02A252b5b4814aD;

    // LiquidationCall event signature
    bytes32 constant LIQUIDATION_TOPIC = 0xe413a321e8681d831f4dbccbca790d2952b56f977908e45be37335533e005286;

    function run() external {
        // Get current block
        uint256 currentBlock = block.number;
        console.log("Current block:", currentBlock);

        // Query logs using vm.getBlockTimestamp and rolling back
        // We'll check the last 1000 blocks for liquidation events

        console.log("Searching for LiquidationCall events...");
        console.log("Pool address:", AAVE_POOL);
        console.log("Event topic:", vm.toString(LIQUIDATION_TOPIC));

        // Use eth_getLogs via vm.rpc if available, otherwise show manual query
        console.log("");
        console.log("To query manually, use:");
        console.log("cast logs --from-block", currentBlock - 100000);
        console.log("  --to-block", currentBlock);
        console.log("  --address", AAVE_POOL);
        console.log("  0xe413a321e8681d831f4dbccbca790d2952b56f977908e45be37335533e005286");
    }

    /// @notice Roll back to a specific block and check positions
    function checkAtBlock(uint256 blockNum) external {
        vm.rollFork(blockNum);
        console.log("Rolled to block:", block.number);
    }
}
