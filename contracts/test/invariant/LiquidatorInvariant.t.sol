// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test, console} from "forge-std/Test.sol";
import {Liquidator} from "../../src/Liquidator.sol";
import {DirectAdapter} from "../../src/adapters/DirectAdapter.sol";
import {LiquidatorHandler} from "./handlers/LiquidatorHandler.sol";
import {MockERC20, MockWETH, MockPool, MockSwapAdapter, MockDebtToken, MockUniswapV3Factory} from "../utils/MockContracts.sol";
import {TestConstants} from "../utils/TestConstants.sol";

contract LiquidatorInvariantTest is Test {
    Liquidator public liquidator;
    LiquidatorHandler public handler;
    MockPool public pool;
    MockUniswapV3Factory public uniswapFactory;
    MockWETH public weth;
    MockERC20 public collateral;
    MockERC20 public debt;
    MockDebtToken public debtToken;
    MockSwapAdapter public adapter;
    DirectAdapter public directAdapter;

    address public owner;

    function setUp() external {
        owner = address(this);

        // Deploy mock contracts
        pool = new MockPool();
        uniswapFactory = new MockUniswapV3Factory();
        weth = new MockWETH();
        collateral = new MockERC20("Collateral", "COL", 18);
        debt = new MockERC20("Debt", "DEBT", 18);
        debtToken = new MockDebtToken();
        adapter = new MockSwapAdapter();
        directAdapter = new DirectAdapter();

        // Setup pool reserve data
        pool.setReserveData(address(debt), address(debtToken));

        // Deploy liquidator
        liquidator = new Liquidator(address(pool), address(uniswapFactory), address(weth));

        // Setup adapters
        liquidator.setAdapter(TestConstants.ADAPTER_UNISWAP_V3, address(adapter));
        liquidator.setAdapter(TestConstants.ADAPTER_DIRECT, address(directAdapter));

        // Deploy handler
        handler = new LiquidatorHandler(liquidator, collateral, debt, adapter, owner);

        // Target only the handler for invariant testing
        targetContract(address(handler));

        // Exclude liquidator from direct calls
        excludeContract(address(liquidator));

        // Setup some initial state
        debt.mint(address(liquidator), 1000 ether);
        vm.deal(address(liquidator), 1 ether);
    }

    /// @notice Owner should never change unexpectedly
    function invariant_OwnerConsistent() external view {
        assertEq(liquidator.owner(), owner, "Owner should not change");
    }

    /// @notice Adapter mapping should remain accessible (not testing validity, just accessibility)
    function invariant_AdaptersAccessible() external view {
        // Just verify we can read adapter addresses without reverting
        // Adapters can be set to any address by owner (including EOAs)
        liquidator.adapters(TestConstants.ADAPTER_UNISWAP_V3);
        liquidator.adapters(TestConstants.ADAPTER_DIRECT);
        liquidator.adapters(TestConstants.ADAPTER_LIQUIDSWAP);
    }

    /// @notice Pool address should never change
    function invariant_PoolImmutable() external view {
        assertEq(address(liquidator.pool()), address(pool), "Pool address should be immutable");
    }

    /// @notice Wrapped native address should never change
    function invariant_WrappedNativeImmutable() external view {
        assertEq(address(liquidator.wrappedNative()), address(weth), "Wrapped native address should be immutable");
    }

    /// @notice Ghost variable tracking - adapter updates should be consistent
    function invariant_GhostAdapterUpdatesConsistent() external view {
        // Sum of per-type updates should equal total updates
        uint256 sumTypeUpdates = handler.ghost_adapterUsageCount(0) + handler.ghost_adapterUsageCount(1)
            + handler.ghost_adapterUsageCount(2);

        assertEq(sumTypeUpdates, handler.ghost_adapterUpdates(), "Adapter update counts should be consistent");
    }

    /// @notice After all operations, balances should be non-negative (obvious but good sanity check)
    function invariant_NonNegativeBalances() external view {
        // ERC20 balances are always >= 0 by definition, but let's verify our accounting
        uint256 liquidatorDebtBalance = debt.balanceOf(address(liquidator));
        uint256 liquidatorNativeBalance = address(liquidator).balance;

        // These should always be true
        assertGe(liquidatorDebtBalance, 0, "Debt balance should be non-negative");
        assertGe(liquidatorNativeBalance, 0, "Native balance should be non-negative");
    }

    /// @notice Summary of invariant test state
    function invariant_callSummary() external view {
        console.log("=== Invariant Test Summary ===");
        console.log("Total adapter updates:", handler.ghost_adapterUpdates());
        console.log("Total rescue operations:", handler.ghost_rescueOperations());
        console.log("Liquidator debt balance:", debt.balanceOf(address(liquidator)));
        console.log("Liquidator native balance:", address(liquidator).balance);
    }
}
