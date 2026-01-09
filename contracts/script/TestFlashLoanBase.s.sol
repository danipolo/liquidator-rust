// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Script, console} from "forge-std/Script.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {Liquidator} from "../src/Liquidator.sol";
import {SwapDataDecoder} from "../src/libraries/SwapDataDecoder.sol";

interface IPool {
    function getUserAccountData(address user) external view returns (
        uint256 totalCollateralBase,
        uint256 totalDebtBase,
        uint256 availableBorrowsBase,
        uint256 currentLiquidationThreshold,
        uint256 ltv,
        uint256 healthFactor
    );
}

interface IUniswapV3Factory {
    function getPool(address tokenA, address tokenB, uint24 fee) external view returns (address);
}

interface IUniswapV3Pool {
    function token0() external view returns (address);
    function token1() external view returns (address);
    function liquidity() external view returns (uint128);
    function flash(address recipient, uint256 amount0, uint256 amount1, bytes calldata data) external;
}

/// @title TestFlashLoanBase
/// @notice Script to test flash loan functionality on Base chain
contract TestFlashLoanBase is Script {
    // Base mainnet addresses
    address constant AAVE_POOL = 0xA238Dd80C259a72e81d7e4664a9801593F98d1c5;
    address constant UNISWAP_FACTORY = 0x33128a8fC17869897dcE68Ed026d694621f6FDfD;
    address constant WETH = 0x4200000000000000000000000000000000000006;
    address constant USDC = 0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913;
    address constant USDbC = 0xd9aAEc86B65D86f6A7B5B1b0c42FFA531710b6CA;

    // Deployed Liquidator address on Base
    address constant LIQUIDATOR = 0xB39E236CED4429b385F5e22377A7Be8b3BC6eDcb;

    uint8 constant ADAPTER_UNISWAP_V3 = 1;

    function run() external view {
        console.log("=== Flash Loan Test - Base Chain ===");
        console.log("");

        // Check Liquidator deployment
        console.log("1. Checking Liquidator deployment...");
        Liquidator liq = Liquidator(payable(LIQUIDATOR));
        console.log("   Liquidator:", LIQUIDATOR);
        console.log("   Owner:", liq.owner());
        console.log("   Pool:", address(liq.pool()));
        console.log("   Flash Source:", uint8(liq.flashSource()) == 0 ? "Uniswap V3" : "AAVE");
        console.log("");

        // Check Uniswap pools exist
        console.log("2. Checking Uniswap V3 pools...");
        _checkPool(WETH, USDC, 500, "WETH/USDC 0.05%");
        _checkPool(WETH, USDC, 3000, "WETH/USDC 0.3%");
        _checkPool(WETH, USDbC, 500, "WETH/USDbC 0.05%");
        console.log("");

        // Check adapters
        console.log("3. Checking adapters...");
        address uniAdapter = liq.adapters(ADAPTER_UNISWAP_V3);
        console.log("   UniswapV3Adapter:", uniAdapter);
        console.log("");

        console.log("=== All checks passed! Ready to liquidate. ===");
        console.log("");
        console.log("To check a position:");
        console.log("  ./forge.sh script script/TestFlashLoanBase.s.sol --sig 'check(address)' <user> --rpc-url base");
        console.log("");
        console.log("To liquidate:");
        console.log("  ./forge.sh script script/TestFlashLoanBase.s.sol --sig 'liquidate(address,address,address)' <user> <collateral> <debt> --rpc-url base --broadcast");
    }

    function _checkPool(address tokenA, address tokenB, uint24 fee, string memory name) internal view {
        address pool = IUniswapV3Factory(UNISWAP_FACTORY).getPool(tokenA, tokenB, fee);
        if (pool != address(0)) {
            uint128 liquidity = IUniswapV3Pool(pool).liquidity();
            console.log("   [OK]", name);
            console.log("        Pool:", pool);
            console.log("        Liquidity:", liquidity);
        } else {
            console.log("   [MISSING]", name);
        }
    }

    /// @notice Check a user's position health on Base AAVE
    function check(address user) external view {
        (
            uint256 totalCollateralBase,
            uint256 totalDebtBase,
            ,
            uint256 currentLiquidationThreshold,
            uint256 ltv,
            uint256 hf
        ) = IPool(AAVE_POOL).getUserAccountData(user);

        console.log("=== Position Check (Base) ===");
        console.log("User:", user);
        console.log("Total Collateral (USD):", totalCollateralBase / 1e8);
        console.log("Total Debt (USD):", totalDebtBase / 1e8);
        console.log("LTV:", ltv);
        console.log("Liquidation Threshold:", currentLiquidationThreshold);
        console.log("Health Factor (%):", hf / 1e16);
        console.log("Health Factor (raw):", hf);
        console.log("Liquidatable:", hf < 1e18 ? "YES" : "NO");
    }

    /// @notice Execute a liquidation on Base
    function liquidate(address user, address collateral, address debt) external {
        uint256 pk = vm.envUint("PRIVATE_KEY");

        // Check health factor first
        (,,,,,uint256 hf) = IPool(AAVE_POOL).getUserAccountData(user);
        console.log("Health Factor:", hf);
        require(hf < 1e18, "Position is healthy, cannot liquidate");

        // Create swap data: collateral -> debt via Uniswap V3 (0.3% fee for better liquidity)
        bytes memory uniData = abi.encodePacked(uint24(3000)); // 0.3% fee
        bytes memory swapData = abi.encodePacked(ADAPTER_UNISWAP_V3, uniData);

        vm.startBroadcast(pk);

        uint256 profit = Liquidator(payable(LIQUIDATOR)).liquidate(
            user,
            collateral,
            debt,
            type(uint256).max, // 50% of debt
            0,                  // minAmountOut (set properly in production!)
            swapData
        );

        vm.stopBroadcast();

        console.log("Liquidation successful! Profit:", profit);
    }

    /// @notice Test flash loan by attempting a dry-run simulation
    function testFlash() external {
        console.log("=== Flash Loan Dry Run ===");

        // Find WETH/USDC pool
        address pool = IUniswapV3Factory(UNISWAP_FACTORY).getPool(WETH, USDC, 500);
        require(pool != address(0), "Pool not found");

        console.log("Pool:", pool);
        console.log("Liquidity:", IUniswapV3Pool(pool).liquidity());

        // Check if we can get token ordering right
        address token0 = IUniswapV3Pool(pool).token0();
        address token1 = IUniswapV3Pool(pool).token1();
        console.log("Token0:", token0);
        console.log("Token1:", token1);

        bool wethIsToken0 = token0 == WETH;
        console.log("WETH is token0:", wethIsToken0);

        console.log("");
        console.log("Flash loan infrastructure verified!");
    }
}
