// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Script.sol";
import "../src/BalancesReader.sol";

contract DeployBalancesReaderHyperliquidScript is Script {
    function run() external {
        // HyperLend (on HyperLiquid EVM) addresses
        address poolDataProvider = 0x5481bf8d3946E6A3168640c1D7523eB59F055a29;
        address oracle = 0xC9Fb4fbE842d57EAc1dF3e641a281827493A630e;

        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");

        vm.startBroadcast(deployerPrivateKey);

        BalancesReader reader = new BalancesReader(poolDataProvider, oracle);

        vm.stopBroadcast();

        console.log("BalancesReader deployed at:", address(reader));
        console.log("PoolDataProvider:", poolDataProvider);
        console.log("Oracle:", oracle);
    }
}
