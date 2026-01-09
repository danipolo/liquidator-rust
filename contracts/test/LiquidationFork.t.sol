// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test, console} from "forge-std/Test.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {Liquidator} from "../src/Liquidator.sol";
import {UniswapV3Adapter} from "../src/adapters/UniswapV3Adapter.sol";
import {DirectAdapter} from "../src/adapters/DirectAdapter.sol";

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

    function supply(address asset, uint256 amount, address onBehalfOf, uint16 referralCode) external;
    function borrow(address asset, uint256 amount, uint256 interestRateMode, uint16 referralCode, address onBehalfOf) external;
}

interface IAaveOracle {
    function getAssetPrice(address asset) external view returns (uint256);
    function BASE_CURRENCY_UNIT() external view returns (uint256);
}

interface IPoolAddressesProvider {
    function getPriceOracle() external view returns (address);
    function setAddress(bytes32 id, address newAddress) external;
    function getACLAdmin() external view returns (address);
}

/// @notice Mock oracle that returns manipulated prices
contract MockOracle {
    mapping(address => uint256) public prices;
    uint256 public constant BASE_CURRENCY_UNIT = 1e8;

    function setPrice(address asset, uint256 price) external {
        prices[asset] = price;
    }

    function getAssetPrice(address asset) external view returns (uint256) {
        return prices[asset];
    }
}

/// @title LiquidationForkTest
/// @notice Fork test to verify the full liquidation flow on Base
contract LiquidationForkTest is Test {
    // Base mainnet addresses
    address constant AAVE_POOL = 0xA238Dd80C259a72e81d7e4664a9801593F98d1c5;
    address constant POOL_ADDRESSES_PROVIDER = 0xe20fCBdBfFC4Dd138cE8b2E6FBb6CB49777ad64D;
    address constant UNISWAP_FACTORY = 0x33128a8fC17869897dcE68Ed026d694621f6FDfD;
    address constant UNISWAP_ROUTER = 0x2626664c2603336E57B271c5C0b26F421741e481;
    address constant WETH = 0x4200000000000000000000000000000000000006;
    address constant USDC = 0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913;

    // Test user with WETH+USDC collateral, USDC debt
    address constant TEST_USER = 0xAe54f3c2b44cA6842d3D1e1Cf3f4039C64e5Bb45;

    Liquidator liquidator;
    DirectAdapter directAdapter;
    UniswapV3Adapter uniswapAdapter;
    address owner;

    uint8 constant ADAPTER_UNISWAP_V3 = 1;
    uint8 constant ADAPTER_DIRECT = 2;

    function setUp() public {
        // Fork Base mainnet
        // vm.createSelectFork(vm.envString("BASE_RPC_URL"));

        owner = address(this);

        // Deploy fresh contracts for testing
        liquidator = new Liquidator(AAVE_POOL, UNISWAP_FACTORY, WETH);
        directAdapter = new DirectAdapter();
        uniswapAdapter = new UniswapV3Adapter(UNISWAP_ROUTER);

        liquidator.setAdapter(ADAPTER_DIRECT, address(directAdapter));
        liquidator.setAdapter(ADAPTER_UNISWAP_V3, address(uniswapAdapter));

        console.log("Liquidator deployed:", address(liquidator));
        console.log("Owner:", owner);
    }

    function testCheckPositionHealth() public view {
        IPool pool = IPool(AAVE_POOL);
        (
            uint256 totalCollateral,
            uint256 totalDebt,
            ,
            uint256 liqThreshold,
            ,
            uint256 hf
        ) = pool.getUserAccountData(TEST_USER);

        console.log("=== Position Health ===");
        console.log("User:", TEST_USER);
        console.log("Collateral (USD):", totalCollateral / 1e8);
        console.log("Debt (USD):", totalDebt / 1e8);
        console.log("Liq Threshold:", liqThreshold);
        console.log("Health Factor:", hf / 1e16, "%");

        // Position should be healthy initially
        assertGt(hf, 1e18, "Position should be healthy");
    }

    function testLiquidationWithPriceManipulation() public {
        IPool pool = IPool(AAVE_POOL);
        address oracle = IPoolAddressesProvider(POOL_ADDRESSES_PROVIDER).getPriceOracle();

        // Get current prices
        uint256 wethPrice = IAaveOracle(oracle).getAssetPrice(WETH);
        uint256 usdcPrice = IAaveOracle(oracle).getAssetPrice(USDC);

        console.log("=== Before Price Manipulation ===");
        console.log("WETH price:", wethPrice);
        console.log("USDC price:", usdcPrice);

        (,,,,,uint256 hfBefore) = pool.getUserAccountData(TEST_USER);
        console.log("Health Factor:", hfBefore / 1e16, "%");

        // Deploy mock oracle with manipulated prices
        MockOracle mockOracle = new MockOracle();
        mockOracle.setPrice(WETH, (wethPrice * 70) / 100); // 30% drop
        mockOracle.setPrice(USDC, usdcPrice);

        // Get ACL admin to change oracle
        address aclAdmin = IPoolAddressesProvider(POOL_ADDRESSES_PROVIDER).getACLAdmin();
        console.log("ACL Admin:", aclAdmin);

        // Prank as ACL admin to set new oracle
        vm.prank(aclAdmin);
        bytes32 PRICE_ORACLE = keccak256("PRICE_ORACLE");
        IPoolAddressesProvider(POOL_ADDRESSES_PROVIDER).setAddress(PRICE_ORACLE, address(mockOracle));

        // Check new health factor
        (,,,,,uint256 hfAfter) = pool.getUserAccountData(TEST_USER);
        console.log("");
        console.log("=== After Price Manipulation ===");
        console.log("New WETH price:", mockOracle.getAssetPrice(WETH));
        console.log("Health Factor:", hfAfter / 1e16, "%");
        console.log("Liquidatable:", hfAfter < 1e18 ? "YES" : "NO");

        if (hfAfter < 1e18) {
            console.log("");
            console.log("=== Executing Liquidation ===");

            // Get USDC balance before
            uint256 usdcBefore = IERC20(USDC).balanceOf(address(liquidator));

            // Create swap data for UniswapV3
            bytes memory swapData = abi.encodePacked(ADAPTER_UNISWAP_V3, uint24(3000));

            // Execute liquidation (WETH collateral -> USDC debt)
            uint256 profit = liquidator.liquidate(
                TEST_USER,
                WETH,
                USDC,
                type(uint256).max,
                0,
                swapData
            );

            uint256 usdcAfter = IERC20(USDC).balanceOf(address(liquidator));

            console.log("Liquidation complete!");
            console.log("Profit:", profit);
            console.log("USDC gained:", usdcAfter - usdcBefore);

            assertGt(profit, 0, "Should have made profit");
        }
    }

    function testFlashLoanMechanism() public view {
        // Verify Uniswap pool exists for flash loans
        address uniFactory = address(liquidator.uniswapFactory());
        console.log("Uniswap Factory:", uniFactory);

        // Check flash source
        Liquidator.FlashSource source = liquidator.flashSource();
        console.log("Flash Source:", uint8(source) == 0 ? "Uniswap V3" : "AAVE");

        assertEq(uint8(source), 0, "Should use Uniswap V3 flash");
    }

    function testAdaptersConfigured() public view {
        address directAddr = liquidator.adapters(ADAPTER_DIRECT);
        address uniAddr = liquidator.adapters(ADAPTER_UNISWAP_V3);

        console.log("DirectAdapter:", directAddr);
        console.log("UniswapV3Adapter:", uniAddr);

        assertEq(directAddr, address(directAdapter), "Direct adapter mismatch");
        assertEq(uniAddr, address(uniswapAdapter), "Uniswap adapter mismatch");
    }
}
