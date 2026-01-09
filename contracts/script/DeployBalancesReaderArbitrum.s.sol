// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Script.sol";
import "../src/BalancesReader.sol";

contract DeployBalancesReaderArbitrumScript is Script {
    function run() external {
        // AAVE V3 Arbitrum addresses
        address poolDataProvider = 0x243Aa95cAC2a25651eda86e80bEe66114413c43b;
        address oracle = 0xb56c2F0B653B2e0b10C9b928C8580Ac5Df02C7C7;

        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");

        vm.startBroadcast(deployerPrivateKey);

        BalancesReader reader = new BalancesReader(poolDataProvider, oracle);

        vm.stopBroadcast();

        console.log("BalancesReader deployed at:", address(reader));
        console.log("PoolDataProvider:", poolDataProvider);
        console.log("Oracle:", oracle);
    }
}
