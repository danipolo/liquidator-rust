// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Script.sol";
import "../src/BalancesReader.sol";

contract DeployBalancesReaderBaseScript is Script {
    function run() external {
        // AAVE V3 Base addresses
        address poolDataProvider = 0x0F43731EB8d45A581f4a36DD74F5f358bc90C73A;
        address oracle = 0x2Cc0Fc26eD4563A5ce5e8bdcfe1A2878676Ae156;

        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");

        vm.startBroadcast(deployerPrivateKey);

        BalancesReader reader = new BalancesReader(poolDataProvider, oracle);

        vm.stopBroadcast();

        console.log("BalancesReader deployed at:", address(reader));
        console.log("PoolDataProvider:", poolDataProvider);
        console.log("Oracle:", oracle);
    }
}
