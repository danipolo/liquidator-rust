// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {DeployBase} from "./Deploy.s.sol";

/// @title DeployEthereum
/// @notice Deployment script for Ethereum mainnet
/// @dev Run with: forge script script/DeployEthereum.s.sol --rpc-url $ETHEREUM_RPC_URL --broadcast
contract DeployEthereum is DeployBase {
    // Ethereum mainnet addresses
    address constant POOL = 0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2;
    address constant UNISWAP_FACTORY = 0x1F98431c8aD98523631AE4a59f267346ea31F984;
    address constant WETH = 0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2;
    address constant UNISWAP_ROUTER = 0xE592427A0AEce92De3Edee1F18E0157C05861564;

    function run() external {
        _deploy(
            POOL,
            UNISWAP_FACTORY,
            WETH,
            address(0), // No LiquidSwap on Ethereum
            UNISWAP_ROUTER
        );
    }
}
