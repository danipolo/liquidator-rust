// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {DeployBase} from "./Deploy.s.sol";

/// @title DeployOptimism
/// @notice Deployment script for Optimism chain
/// @dev Run with: forge script script/DeployOptimism.s.sol --rpc-url $OPTIMISM_RPC_URL --broadcast
contract DeployOptimism is DeployBase {
    // Optimism mainnet addresses
    address constant POOL = 0x794a61358D6845594F94dc1DB02A252b5b4814aD;
    address constant UNISWAP_FACTORY = 0x1F98431c8aD98523631AE4a59f267346ea31F984;
    address constant WETH = 0x4200000000000000000000000000000000000006;
    address constant UNISWAP_ROUTER = 0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45;

    function run() external {
        _deploy(
            POOL,
            UNISWAP_FACTORY,
            WETH,
            address(0), // No LiquidSwap on Optimism
            UNISWAP_ROUTER
        );
    }
}
