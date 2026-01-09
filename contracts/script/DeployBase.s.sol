// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {DeployBase} from "./Deploy.s.sol";

/// @title DeployBaseChain
/// @notice Deployment script for Base chain
/// @dev Run with: forge script script/DeployBase.s.sol --rpc-url $BASE_RPC_URL --broadcast
contract DeployBaseChain is DeployBase {
    // Base mainnet addresses
    address constant POOL = 0xA238Dd80C259a72e81d7e4664a9801593F98d1c5;
    address constant UNISWAP_FACTORY = 0x33128a8fC17869897dcE68Ed026d694621f6FDfD;
    address constant WETH = 0x4200000000000000000000000000000000000006;
    address constant UNISWAP_ROUTER = 0x2626664c2603336E57B271c5C0b26F421741e481;

    function run() external {
        _deploy(
            POOL,
            UNISWAP_FACTORY,
            WETH,
            address(0), // No LiquidSwap on Base
            UNISWAP_ROUTER
        );
    }
}
