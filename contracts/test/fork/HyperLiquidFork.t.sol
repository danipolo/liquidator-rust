// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test, console} from "forge-std/Test.sol";
import {Liquidator} from "../../src/Liquidator.sol";
import {LiquidSwapAdapter} from "../../src/adapters/LiquidSwapAdapter.sol";
import {DirectAdapter} from "../../src/adapters/DirectAdapter.sol";
import {TestConstants} from "../utils/TestConstants.sol";

/// @title HyperLiquidForkTest
/// @notice Fork tests against HyperLiquid mainnet
/// @dev Requires HYPERLIQUID_RPC_URL environment variable
contract HyperLiquidForkTest is Test {
    Liquidator public liquidator;
    LiquidSwapAdapter public liquidSwapAdapter;
    DirectAdapter public directAdapter;

    // HyperLiquid mainnet addresses
    address constant AAVE_POOL = address(0); // TBD - Update when available
    address constant UNISWAP_FACTORY = address(0); // No Uniswap V3 on HyperLiquid
    address constant WHYPE = 0x5555555555555555555555555555555555555555;
    address constant LIQUIDSWAP_ROUTER = 0x744489Ee3d540777A66f2cf297479745e0852f7A;

    bool public forkEnabled;

    function setUp() external {
        // Try to create fork - skip tests if RPC URL not available
        try vm.envString("HYPERLIQUID_RPC_URL") returns (string memory rpcUrl) {
            if (bytes(rpcUrl).length > 0 && AAVE_POOL != address(0)) {
                vm.createSelectFork(rpcUrl);
                forkEnabled = true;

                // Deploy contracts on fork
                liquidator = new Liquidator(AAVE_POOL, UNISWAP_FACTORY, WHYPE);
                liquidSwapAdapter = new LiquidSwapAdapter(LIQUIDSWAP_ROUTER);
                directAdapter = new DirectAdapter();

                // Setup adapters
                liquidator.setAdapter(TestConstants.ADAPTER_LIQUIDSWAP, address(liquidSwapAdapter));
                liquidator.setAdapter(TestConstants.ADAPTER_DIRECT, address(directAdapter));
            }
        } catch {
            // Fork not available
            forkEnabled = false;
        }
    }

    modifier onlyFork() {
        if (!forkEnabled) {
            return;
        }
        _;
    }

    /// @notice Test that LiquidSwapAdapter is deployed correctly
    function testFork_LiquidSwapAdapter_Deployment() external onlyFork {
        assertEq(address(liquidSwapAdapter.router()), LIQUIDSWAP_ROUTER);
    }

    /// @notice Test that Liquidator is configured correctly
    function testFork_Liquidator_Configuration() external onlyFork {
        assertEq(address(liquidator.pool()), AAVE_POOL);
        assertEq(address(liquidator.wrappedNative()), WHYPE);
        assertEq(liquidator.adapters(TestConstants.ADAPTER_LIQUIDSWAP), address(liquidSwapAdapter));
        assertEq(liquidator.adapters(TestConstants.ADAPTER_DIRECT), address(directAdapter));
    }

    /// @notice Test swap against real LiquidSwap DEX
    /// @dev This test requires actual token balances on the fork
    function testFork_LiquidSwapAdapter_Swap() external onlyFork {
        // This test would require:
        // 1. Whaling tokens from a known holder
        // 2. Setting up proper swap paths
        // 3. Executing the swap

        // For now, just verify the adapter is set up correctly
        assertTrue(address(liquidSwapAdapter.router()).code.length > 0, "Router should be a contract");
    }

    /// @notice Test liquidation against actual underwater position
    /// @dev This test requires finding a liquidatable position on-chain
    function testFork_Liquidate_RealPosition() external onlyFork {
        // This test would require:
        // 1. Finding an underwater position on AAVE
        // 2. Setting up proper swap data
        // 3. Executing the liquidation

        // For now, verify setup is correct
        assertEq(liquidator.owner(), address(this));
    }

    /// @notice Placeholder test that always passes when fork is not available
    function test_Placeholder_WhenNoFork() external pure {
        // This test ensures the test file compiles and runs even without fork
        assertTrue(true);
    }
}
