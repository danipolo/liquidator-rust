// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Script, console} from "forge-std/Script.sol";
import {Liquidator} from "../src/Liquidator.sol";
import {LiquidSwapAdapter} from "../src/adapters/LiquidSwapAdapter.sol";
import {UniswapV3Adapter} from "../src/adapters/UniswapV3Adapter.sol";
import {DirectAdapter} from "../src/adapters/DirectAdapter.sol";

/// @title DeployBase
/// @notice Base deployment script with common deployment logic
/// @dev Chain-specific scripts inherit from this and provide addresses
///
/// Flash Loan Source Selection:
///   - If uniswapFactory != address(0) → Uses Uniswap V3 flash swaps
///   - If uniswapFactory == address(0) → Uses AAVE pool flash loans
///
/// Chains:
///   - Arbitrum, Base, Optimism: Use Uniswap V3 flash (AAVE flash disabled for some assets)
///   - HyperLiquid: Use AAVE flash (no Uniswap V3 on chain)
abstract contract DeployBase is Script {
    // Adapter type identifiers
    uint8 internal constant ADAPTER_LIQUIDSWAP = 0;
    uint8 internal constant ADAPTER_UNISWAP_V3 = 1;
    uint8 internal constant ADAPTER_DIRECT = 2;

    Liquidator public liquidator;
    DirectAdapter public directAdapter;
    LiquidSwapAdapter public liquidSwapAdapter;
    UniswapV3Adapter public uniswapV3Adapter;

    /// @notice Deploys the Liquidator and adapters
    /// @param pool The AAVE V3 Pool address
    /// @param uniswapFactory The Uniswap V3 Factory address (for flash loans)
    /// @param wrappedNative The wrapped native token address (WETH/WHYPE)
    /// @param liquidSwapRouter The LiquidSwap router address (address(0) to skip)
    /// @param uniswapRouter The Uniswap V3 router address (address(0) to skip)
    function _deploy(
        address pool,
        address uniswapFactory,
        address wrappedNative,
        address liquidSwapRouter,
        address uniswapRouter
    ) internal {
        uint256 deployerKey = vm.envUint("PRIVATE_KEY");
        address deployer = vm.addr(deployerKey);

        console.log("Deploying from:", deployer);
        console.log("Pool:", pool);
        console.log("Uniswap Factory:", uniswapFactory);
        console.log("Wrapped Native:", wrappedNative);

        vm.startBroadcast(deployerKey);

        // Deploy core contract
        // Flash source determined by uniswapFactory: address(0) = AAVE, else = Uniswap V3
        liquidator = new Liquidator(pool, uniswapFactory, wrappedNative);
        console.log("Liquidator deployed:", address(liquidator));
        console.log("Flash source:", uniswapFactory != address(0) ? "Uniswap V3" : "AAVE");

        // Deploy DirectAdapter (always deployed)
        directAdapter = new DirectAdapter();
        liquidator.setAdapter(ADAPTER_DIRECT, address(directAdapter));
        console.log("DirectAdapter deployed:", address(directAdapter));

        // Deploy LiquidSwapAdapter if router provided
        if (liquidSwapRouter != address(0)) {
            liquidSwapAdapter = new LiquidSwapAdapter(liquidSwapRouter);
            liquidator.setAdapter(ADAPTER_LIQUIDSWAP, address(liquidSwapAdapter));
            console.log("LiquidSwapAdapter deployed:", address(liquidSwapAdapter));
        }

        // Deploy UniswapV3Adapter if router provided
        if (uniswapRouter != address(0)) {
            uniswapV3Adapter = new UniswapV3Adapter(uniswapRouter);
            liquidator.setAdapter(ADAPTER_UNISWAP_V3, address(uniswapV3Adapter));
            console.log("UniswapV3Adapter deployed:", address(uniswapV3Adapter));
        }

        vm.stopBroadcast();

        // Verify deployment
        require(liquidator.owner() == deployer, "Owner mismatch");
        require(address(liquidator.pool()) == pool, "Pool mismatch");
        require(address(liquidator.wrappedNative()) == wrappedNative, "Wrapped native mismatch");

        console.log("");
        console.log("=== Deployment Complete ===");
        console.log("Owner:", liquidator.owner());
        console.log("Default Flash Pool Fee:", liquidator.defaultFlashPoolFee());
    }
}
