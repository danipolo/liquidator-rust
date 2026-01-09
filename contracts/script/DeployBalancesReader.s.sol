// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Script.sol";
import "../src/BalancesReader.sol";

contract DeployBalancesReaderScript is Script {
    function run() external {
        // Optimism AAVE V3 addresses
        address poolDataProvider = 0x69FA688f1Dc47d4B5d8029D5a35FB7a548310654;
        address oracle = 0xD81eb3728a631871a7eBBaD631b5f424909f0c77;

        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");

        vm.startBroadcast(deployerPrivateKey);

        BalancesReader reader = new BalancesReader(poolDataProvider, oracle);

        vm.stopBroadcast();

        console.log("BalancesReader deployed at:", address(reader));
        console.log("PoolDataProvider:", poolDataProvider);
        console.log("Oracle:", oracle);
    }
}
