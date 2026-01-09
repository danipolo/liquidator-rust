// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test, console} from "forge-std/Test.sol";
import {Liquidator} from "../../src/Liquidator.sol";
import {UniswapV3Adapter} from "../../src/adapters/UniswapV3Adapter.sol";
import {SwapDataDecoder} from "../../src/libraries/SwapDataDecoder.sol";

interface IPool {
    function getUserAccountData(address user) external view returns (
        uint256 totalCollateralBase,
        uint256 totalDebtBase,
        uint256 availableBorrowsBase,
        uint256 currentLiquidationThreshold,
        uint256 ltv,
        uint256 healthFactor
    );

    function supply(address asset, uint256 amount, address onBehalfOf, uint16 referralCode) external;
    function borrow(address asset, uint256 amount, uint256 interestRateMode, uint16 referralCode, address onBehalfOf) external;
    function getReserveData(address asset) external view returns (ReserveData memory);

    struct ReserveData {
        uint256 configuration;
        uint128 liquidityIndex;
        uint128 currentLiquidityRate;
        uint128 variableBorrowIndex;
        uint128 currentVariableBorrowRate;
        uint128 currentStableBorrowRate;
        uint40 lastUpdateTimestamp;
        uint16 id;
        address aTokenAddress;
        address stableDebtTokenAddress;
        address variableDebtTokenAddress;
        address interestRateStrategyAddress;
        uint128 accruedToTreasury;
        uint128 unbacked;
        uint128 isolationModeTotalDebt;
    }
}

interface IERC20 {
    function balanceOf(address) external view returns (uint256);
    function approve(address, uint256) external returns (bool);
}

interface IWETH {
    function deposit() external payable;
    function approve(address, uint256) external returns (bool);
    function balanceOf(address) external view returns (uint256);
}

interface IChainlinkAggregator {
    function latestAnswer() external view returns (int256);
}

/// @title LiquidatorE2ETest
/// @notice End-to-end test of the Liquidator using Uniswap V3 flash swaps
contract LiquidatorE2ETest is Test {
    // AAVE V3 Pool on Arbitrum
    address constant AAVE_POOL = 0x794a61358D6845594F94dc1DB02A252b5b4814aD;

    // Uniswap V3 on Arbitrum
    address constant UNISWAP_FACTORY = 0x1F98431c8aD98523631AE4a59f267346ea31F984;
    address constant UNISWAP_ROUTER = 0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45;

    // Assets
    address constant WETH = 0x82aF49447D8a07e3bd95BD0d56f35241523fBab1;
    address constant USDC = 0xaf88d065e77c8cC2239327C5EDb3A432268e5831;

    // Chainlink WETH/USD price feed on Arbitrum
    address constant WETH_USD_FEED = 0x639Fe6ab55C921f74e7fac1ee960C0B6293ba612;

    bool public forkEnabled;
    Liquidator public liquidator;
    UniswapV3Adapter public adapter;

    address public victim;

    function setUp() external {
        try vm.envString("ARBITRUM_RPC_URL") returns (string memory rpcUrl) {
            if (bytes(rpcUrl).length > 0) {
                vm.createSelectFork(rpcUrl);
                forkEnabled = true;

                // Deploy Liquidator and Adapter
                liquidator = new Liquidator(AAVE_POOL, UNISWAP_FACTORY, WETH);
                adapter = new UniswapV3Adapter(UNISWAP_ROUTER);

                // Register UniswapV3 adapter (type 1)
                liquidator.setAdapter(1, address(adapter));

                victim = makeAddr("victim");
            }
        } catch {
            forkEnabled = false;
        }
    }

    modifier onlyFork() {
        if (!forkEnabled) return;
        _;
    }

    /// @notice Test creating an underwater position and liquidating with flash swap
    function testFork_FlashLiquidation() external onlyFork {
        console.log("=== Liquidator E2E Test ===");
        console.log("Liquidator:", address(liquidator));
        console.log("Adapter:", address(adapter));

        // Give victim some ETH
        vm.deal(victim, 10 ether);

        vm.startPrank(victim);

        // Wrap ETH to WETH
        IWETH(WETH).deposit{value: 5 ether}();
        console.log("Victim WETH:", IWETH(WETH).balanceOf(victim) / 1e18, "WETH");

        // Approve and supply WETH
        IWETH(WETH).approve(AAVE_POOL, type(uint256).max);
        IPool(AAVE_POOL).supply(WETH, 5 ether, victim, 0);
        console.log("Supplied 5 WETH as collateral");

        // Get max borrow
        (uint256 collateral, , uint256 availableBorrow, , , ) = IPool(AAVE_POOL).getUserAccountData(victim);
        console.log("Collateral (USD):", collateral / 1e8);
        console.log("Available borrow (USD):", availableBorrow / 1e8);

        // Borrow 95% of max USDC
        uint256 borrowAmount = (availableBorrow * 95 / 100) * 1e6 / 1e8;
        IPool(AAVE_POOL).borrow(USDC, borrowAmount, 2, 0, victim);
        console.log("Borrowed USDC:", borrowAmount / 1e6);

        (, uint256 debt, , , , uint256 hfAfterBorrow) = IPool(AAVE_POOL).getUserAccountData(victim);
        console.log("Debt (USD):", debt / 1e8);
        console.log("HF after borrow:", hfAfterBorrow / 1e16, "%");

        vm.stopPrank();

        // Crash ETH price 50%
        console.log("--- Crashing ETH price 50% ---");
        int256 currentPrice = IChainlinkAggregator(WETH_USD_FEED).latestAnswer();
        console.log("Current ETH price:", uint256(currentPrice) / 1e8, "USD");

        vm.mockCall(
            WETH_USD_FEED,
            abi.encodeWithSelector(IChainlinkAggregator.latestAnswer.selector),
            abi.encode(currentPrice / 2)
        );

        (, , , , , uint256 hfAfterCrash) = IPool(AAVE_POOL).getUserAccountData(victim);
        console.log("HF after crash:", hfAfterCrash / 1e16, "%");
        console.log(">>> POSITION IS LIQUIDATABLE <<<");

        assertLt(hfAfterCrash, 1e18, "Position should be underwater");

        // Execute liquidation with Liquidator
        console.log("--- Executing Flash Liquidation ---");

        IPool.ReserveData memory reserveData = IPool(AAVE_POOL).getReserveData(USDC);
        uint256 debtBalance = IERC20(reserveData.variableDebtTokenAddress).balanceOf(victim);
        uint256 debtToCover = debtBalance / 2;
        console.log("Debt to cover:", debtToCover / 1e6, "USDC");

        // IMPORTANT: Use DIFFERENT pool for swap than for flash!
        // Flash from 0.05% pool (default), swap through 0.3% pool to avoid LOK reentrancy error
        bytes memory uniswapData = SwapDataDecoder.encodeUniswapV3Data(
            false,
            abi.encode(uint24(3000)) // 0.3% fee pool for swap
        );
        bytes memory swapData = SwapDataDecoder.encodeWrappedSwapData(1, uniswapData);

        uint256 usdcBefore = IERC20(USDC).balanceOf(address(this));

        // Execute using standard ILiquidator interface (6 params, uses defaultFlashPoolFee=500)
        uint256 profit = liquidator.liquidate(
            victim,
            WETH,
            USDC,
            debtToCover,
            0, // no min for testing
            swapData
        );

        (, uint256 debtAfter, , , , uint256 hfAfter) = IPool(AAVE_POOL).getUserAccountData(victim);
        uint256 usdcAfter = IERC20(USDC).balanceOf(address(this));

        console.log("---");
        console.log("Debt after (USD):", debtAfter / 1e8);
        console.log("HF after:", hfAfter / 1e16, "%");
        console.log("Profit (USDC):", profit / 1e6);
        console.log("Contract balance change:", (usdcAfter - usdcBefore) / 1e6, "USDC");

        // Verify liquidation succeeded
        assertLt(debtAfter, debt, "Debt should be reduced");
        assertGt(profit, 0, "Should have profit");

        console.log("=== FLASH LIQUIDATION SUCCESSFUL ===");
    }

    /// @notice Replay the real liquidation from block 418581414
    function testFork_ReplayRealLiquidation() external onlyFork {
        console.log("=== Replaying Real Liquidation ===");

        // Fork at block before the real liquidation
        vm.rollFork(418581414);

        // Deploy fresh contracts at this block
        liquidator = new Liquidator(AAVE_POOL, UNISWAP_FACTORY, WETH);
        adapter = new UniswapV3Adapter(UNISWAP_ROUTER);
        liquidator.setAdapter(1, address(adapter));

        address liquidatedUser = 0x308A31d418f62711D5D71d71fDBFcd74968883F8;

        // Check position
        (, uint256 debtBefore, , , , uint256 hfBefore) = IPool(AAVE_POOL).getUserAccountData(liquidatedUser);
        console.log("User:", liquidatedUser);
        console.log("Debt (USD):", debtBefore / 1e8);
        console.log("HF:", hfBefore / 1e16, "%");

        assertLt(hfBefore, 1e18, "Position should be underwater");

        // The real tx covered ~37,275 USDC
        uint256 debtToCover = 37275447749;

        // Use different pool for swap than flash to avoid LOK
        bytes memory uniswapData = SwapDataDecoder.encodeUniswapV3Data(
            false,
            abi.encode(uint24(3000)) // 0.3% pool for swap
        );
        bytes memory swapData = SwapDataDecoder.encodeWrappedSwapData(1, uniswapData);

        // Execute with explicit fee using liquidateWithFee
        uint256 profit = liquidator.liquidateWithFee(
            liquidatedUser,
            WETH,
            USDC,
            debtToCover,
            0,
            swapData,
            500 // 0.05% pool for flash
        );

        (, uint256 debtAfter, , , , uint256 hfAfter) = IPool(AAVE_POOL).getUserAccountData(liquidatedUser);

        console.log("---");
        console.log("Debt after (USD):", debtAfter / 1e8);
        console.log("HF after:", hfAfter / 1e16, "%");
        console.log("Profit (USDC):", profit / 1e6);

        assertGt(hfAfter, hfBefore, "HF should improve");
        console.log("=== REPLAY SUCCESSFUL ===");
    }

    function test_Placeholder() external pure {
        assertTrue(true);
    }
}
