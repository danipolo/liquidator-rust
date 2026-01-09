// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {DeployBase} from "./Deploy.s.sol";

/// @title DeployHyperLiquid
/// @notice Deployment script for HyperLiquid chain (HyperLend/AAVE fork)
/// @dev Run with: forge script script/DeployHyperLiquid.s.sol --rpc-url $HYPERLIQUID_RPC_URL --broadcast
/// @dev Uses AAVE flash loans since Uniswap V3 is not available on HyperLiquid
contract DeployHyperLiquid is DeployBase {
    // HyperLiquid mainnet addresses (HyperLend - AAVE V3 fork)
    address constant POOL = 0x00A89d7a5A02160f20150EbEA7a2b5E4879A1A8b;
    address constant WHYPE = 0x5555555555555555555555555555555555555555;
    address constant LIQUIDSWAP_ROUTER = 0x744489Ee3d540777A66f2cf297479745e0852f7A;

    // No Uniswap V3 on HyperLiquid → Contract will use AAVE flash loans
    address constant UNISWAP_FACTORY = address(0);

    function run() external {
        _deploy(
            POOL,
            UNISWAP_FACTORY, // address(0) → Uses AAVE flash loans
            WHYPE,
            LIQUIDSWAP_ROUTER, // LiquidSwap for swaps
            address(0) // No Uniswap router needed
        );
    }
}
