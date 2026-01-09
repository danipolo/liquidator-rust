// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {DeployBase} from "./Deploy.s.sol";

/// @title DeployArbitrum
/// @notice Deployment script for Arbitrum chain
/// @dev Run with: forge script script/DeployArbitrum.s.sol --rpc-url $ARBITRUM_RPC_URL --broadcast
contract DeployArbitrum is DeployBase {
    // Arbitrum mainnet addresses
    address constant POOL = 0x794a61358D6845594F94dc1DB02A252b5b4814aD;
    address constant UNISWAP_FACTORY = 0x1F98431c8aD98523631AE4a59f267346ea31F984;
    address constant WETH = 0x82aF49447D8a07e3bd95BD0d56f35241523fBab1;
    address constant UNISWAP_ROUTER = 0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45;

    function run() external {
        _deploy(
            POOL,
            UNISWAP_FACTORY,
            WETH,
            address(0), // No LiquidSwap on Arbitrum
            UNISWAP_ROUTER
        );
    }
}
