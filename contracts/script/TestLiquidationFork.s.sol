// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Script, console} from "forge-std/Script.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {Liquidator} from "../src/Liquidator.sol";

interface IPool {
    function getUserAccountData(address user) external view returns (
        uint256 totalCollateralBase,
        uint256 totalDebtBase,
        uint256 availableBorrowsBase,
        uint256 currentLiquidationThreshold,
        uint256 ltv,
        uint256 healthFactor
    );

    function getReserveData(address asset) external view returns (
        uint256 configuration,
        uint128 liquidityIndex,
        uint128 currentLiquidityRate,
        uint128 variableBorrowIndex,
        uint128 currentVariableBorrowRate,
        uint128 currentStableBorrowRate,
        uint40 lastUpdateTimestamp,
        uint16 id,
        address aTokenAddress,
        address stableDebtTokenAddress,
        address variableDebtTokenAddress,
        address interestRateStrategyAddress,
        uint128 accruedToTreasury,
        uint128 unbacked,
        uint128 isolationModeTotalDebt
    );
}

interface IAaveOracle {
    function getAssetPrice(address asset) external view returns (uint256);
    function setAssetSources(address[] calldata assets, address[] calldata sources) external;
}

interface IPoolAddressesProvider {
    function getPriceOracle() external view returns (address);
    function getACLAdmin() external view returns (address);
}

/// @title TestLiquidationFork
/// @notice Fork test to verify liquidation flow works correctly on Base
contract TestLiquidationFork is Script {
    // Base mainnet addresses
    address constant AAVE_POOL = 0xA238Dd80C259a72e81d7e4664a9801593F98d1c5;
    address constant POOL_ADDRESSES_PROVIDER = 0xe20fCBdBfFC4Dd138cE8b2E6FBb6CB49777ad64D;
    address constant WETH = 0x4200000000000000000000000000000000000006;
    address constant USDC = 0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913;

    // Deployed Liquidator
    address constant LIQUIDATOR = 0xB39E236CED4429b385F5e22377A7Be8b3BC6eDcb;
    address constant OWNER = 0x1f79618e870fd5b5C3320106cb368125723B6245;

    // Test position - WETH collateral, WETH borrow, HF ~1.107
    address constant TEST_USER = 0xafc12216b7bFd3C63490E608FC2e984C82054C4b;

    uint8 constant ADAPTER_UNISWAP_V3 = 1;
    uint8 constant ADAPTER_DIRECT = 2;

    function run() external {
        console.log("=== Fork Liquidation Test - Base ===");
        console.log("");

        // Get current state
        IPool pool = IPool(AAVE_POOL);
        (
            uint256 totalCollateral,
            uint256 totalDebt,
            ,
            uint256 liqThreshold,
            ,
            uint256 hf
        ) = pool.getUserAccountData(TEST_USER);

        console.log("Target user:", TEST_USER);
        console.log("Collateral (USD):", totalCollateral / 1e8);
        console.log("Debt (USD):", totalDebt / 1e8);
        console.log("Liq Threshold:", liqThreshold);
        console.log("Health Factor:", hf / 1e16, "%");
        console.log("Liquidatable:", hf < 1e18 ? "YES" : "NO");
        console.log("");

        if (hf >= 1e18) {
            console.log("Position is healthy. Running fork simulation with price manipulation...");
            _runForkSimulation();
        } else {
            console.log("Position is liquidatable! You can run a real liquidation.");
        }
    }

    function _runForkSimulation() internal {
        // Get oracle
        address oracle = IPoolAddressesProvider(POOL_ADDRESSES_PROVIDER).getPriceOracle();
        console.log("Oracle:", oracle);

        uint256 wethPrice = IAaveOracle(oracle).getAssetPrice(WETH);
        console.log("Current WETH price:", wethPrice);

        // Calculate price drop needed to make HF < 1
        // HF = (collateral * liqThreshold) / debt
        // We need to drop collateral value so HF < 1
        // For this WETH/WETH position, dropping WETH price affects both equally
        // So we need a different approach - we'll use a WETH/USDC position instead

        console.log("");
        console.log("Note: The test user has WETH collateral and WETH debt.");
        console.log("Price changes affect both equally, making liquidation impossible.");
        console.log("");
        console.log("Switching to user with WETH collateral and USDC debt...");

        _testWithUsdcDebt();
    }

    function _testWithUsdcDebt() internal {
        // User with WETH+USDC collateral, USDC debt - HF 1.174
        address testUser2 = 0xAe54f3c2b44cA6842d3D1e1Cf3f4039C64e5Bb45;

        IPool pool = IPool(AAVE_POOL);
        (
            uint256 totalCollateral,
            uint256 totalDebt,
            ,
            ,
            ,
            uint256 hf
        ) = pool.getUserAccountData(testUser2);

        console.log("");
        console.log("=== Test User 2 ===");
        console.log("Address:", testUser2);
        console.log("Collateral (USD):", totalCollateral / 1e8);
        console.log("Debt (USD):", totalDebt / 1e8);
        console.log("Health Factor:", hf / 1e16, "%");

        // Get oracle and simulate price drop
        address oracle = IPoolAddressesProvider(POOL_ADDRESSES_PROVIDER).getPriceOracle();
        uint256 wethPrice = IAaveOracle(oracle).getAssetPrice(WETH);

        // Calculate new price to make HF = 0.95
        // Current HF = 1.174
        // Need to reduce collateral value by ~20%
        uint256 newWethPrice = (wethPrice * 80) / 100;

        console.log("");
        console.log("Simulating 20% WETH price drop...");
        console.log("Current WETH price:", wethPrice);
        console.log("New WETH price:", newWethPrice);

        // In a real fork test, we would use vm.store to manipulate the oracle
        // For now, let's just show what the test would look like

        console.log("");
        console.log("=== To run full fork test, use: ===");
        console.log("forge test --fork-url $BASE_RPC_URL --match-test testLiquidation -vvv");
    }

    /// @notice Simulate a direct liquidation (same collateral/debt token)
    function testDirectLiquidation() external {
        console.log("=== Direct Liquidation Test (WETH -> WETH) ===");

        // For WETH/WETH positions, we use the DirectAdapter (no swap needed)
        // This tests the flash loan and liquidation without swap complexity

        // First check if there are any liquidatable WETH/WETH positions
        IPool pool = IPool(AAVE_POOL);
        (,,,,,uint256 hf) = pool.getUserAccountData(TEST_USER);

        console.log("Test user HF:", hf / 1e16, "%");

        if (hf < 1e18) {
            console.log("Position is liquidatable!");
            console.log("");
            console.log("To execute:");
            console.log("./forge.sh script script/TestLiquidationFork.s.sol --sig 'executeDirect()' --rpc-url base --broadcast");
        } else {
            console.log("Position is healthy - cannot liquidate");
        }
    }

    /// @notice Execute a direct liquidation (for WETH/WETH positions)
    function executeDirect() external {
        uint256 pk = vm.envUint("PRIVATE_KEY");

        IPool pool = IPool(AAVE_POOL);
        (,,,,,uint256 hf) = pool.getUserAccountData(TEST_USER);
        require(hf < 1e18, "Position is healthy");

        // For same-token liquidation, use DirectAdapter (type 2)
        bytes memory swapData = abi.encodePacked(ADAPTER_DIRECT);

        console.log("Executing direct liquidation...");
        console.log("User:", TEST_USER);
        console.log("Collateral: WETH");
        console.log("Debt: WETH");

        vm.startBroadcast(pk);

        uint256 profit = Liquidator(payable(LIQUIDATOR)).liquidate(
            TEST_USER,
            WETH,
            WETH,
            type(uint256).max,
            0,
            swapData
        );

        vm.stopBroadcast();

        console.log("Profit:", profit);
    }

    /// @notice Check multiple positions from the CSV
    function checkPositions() external view {
        console.log("=== Checking Positions ===");

        address[4] memory users = [
            0xafc12216b7bFd3C63490E608FC2e984C82054C4b,
            0xAe54f3c2b44cA6842d3D1e1Cf3f4039C64e5Bb45,
            0xAe379Dd776061b8cA772e0481D48606DF8a24953,
            0xaE88A8e7740262AfeDe0bB4b06BD5AFeA67BB449
        ];

        IPool pool = IPool(AAVE_POOL);

        for (uint i = 0; i < users.length; i++) {
            (
                uint256 totalCollateral,
                uint256 totalDebt,
                ,,,
                uint256 hf
            ) = pool.getUserAccountData(users[i]);

            console.log("");
            console.log("User:", users[i]);
            console.log("  Collateral:", totalCollateral / 1e8, "USD");
            console.log("  Debt:", totalDebt / 1e8, "USD");
            console.log("  HF:", hf / 1e16, "%");
            console.log("  Liquidatable:", hf < 1e18 ? "YES" : "NO");
        }
    }
}
