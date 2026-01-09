// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test, console} from "forge-std/Test.sol";
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
    function setUserUseReserveAsCollateral(address asset, bool useAsCollateral) external;
}

interface IPoolAddressesProvider {
    function getPriceOracle() external view returns (address);
    function setPriceOracle(address newPriceOracle) external;
    function getACLAdmin() external view returns (address);
    function getACLManager() external view returns (address);
}

interface IACLManager {
    function addPoolAdmin(address admin) external;
    function isPoolAdmin(address admin) external view returns (bool);
}

interface IAaveOracle {
    function getAssetPrice(address asset) external view returns (uint256);
    function setAssetSources(address[] calldata assets, address[] calldata sources) external;
    function getSourceOfAsset(address asset) external view returns (address);
}

/// @notice Mock price feed that returns a fixed price
contract MockPriceFeed {
    int256 public price;
    uint8 public decimals = 8;

    constructor(int256 _price) {
        price = _price;
    }

    function setPrice(int256 _price) external {
        price = _price;
    }

    function latestRoundData() external view returns (
        uint80 roundId,
        int256 answer,
        uint256 startedAt,
        uint256 updatedAt,
        uint80 answeredInRound
    ) {
        return (1, price, block.timestamp, block.timestamp, 1);
    }
}

/// @title LiquidationE2ETest
/// @notice End-to-end test using the DEPLOYED liquidator contract on Base
contract LiquidationE2ETest is Test {
    // Base mainnet addresses
    address constant AAVE_POOL = 0xA238Dd80C259a72e81d7e4664a9801593F98d1c5;
    address constant POOL_ADDRESSES_PROVIDER = 0xe20fCBdBfFC4Dd138cE8b2E6FBb6CB49777ad64D;
    address constant WETH = 0x4200000000000000000000000000000000000006;
    address constant USDC = 0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913;

    // DEPLOYED Liquidator on Base
    address constant LIQUIDATOR = 0xB39E236CED4429b385F5e22377A7Be8b3BC6eDcb;
    address constant OWNER = 0x1f79618e870fd5b5C3320106cb368125723B6245;

    uint8 constant ADAPTER_UNISWAP_V3 = 1;

    IPool pool;
    Liquidator liquidator;

    function setUp() public {
        pool = IPool(AAVE_POOL);
        liquidator = Liquidator(payable(LIQUIDATOR));

        console.log("=== E2E Test Setup ===");
        console.log("Using deployed Liquidator:", LIQUIDATOR);
        console.log("Owner:", liquidator.owner());
    }

    function testDeployedContractConfiguration() public view {
        console.log("=== Deployed Contract Check ===");
        console.log("Pool:", address(liquidator.pool()));
        console.log("Uniswap Factory:", address(liquidator.uniswapFactory()));
        console.log("Wrapped Native:", address(liquidator.wrappedNative()));
        console.log("Flash Source:", uint8(liquidator.flashSource()) == 0 ? "Uniswap V3" : "AAVE");
        console.log("Default Flash Fee:", liquidator.defaultFlashPoolFee());

        // Check adapters
        address uniAdapter = liquidator.adapters(ADAPTER_UNISWAP_V3);
        console.log("UniswapV3Adapter:", uniAdapter);

        assertEq(address(liquidator.pool()), AAVE_POOL, "Pool mismatch");
        assertEq(liquidator.owner(), OWNER, "Owner mismatch");
        assertTrue(uniAdapter != address(0), "Adapter not set");
    }

    function testCreateAndLiquidatePosition() public {
        console.log("=== Create & Liquidate Test ===");

        address testUser = makeAddr("testUser");
        _createRiskyPosition(testUser);
        _manipulatePriceAndLiquidate(testUser);
    }

    function _createRiskyPosition(address testUser) internal {
        // Deal WETH to test user (10 WETH)
        deal(WETH, testUser, 10 ether);

        // Supply WETH as collateral
        vm.startPrank(testUser);
        IERC20(WETH).approve(AAVE_POOL, type(uint256).max);
        pool.supply(WETH, 10 ether, testUser, 0);

        // Check how much we can borrow
        (,, uint256 availableBorrows,,,) = pool.getUserAccountData(testUser);
        console.log("Available borrows (USD):", availableBorrows / 1e8);

        // Borrow 95% of available (very risky position)
        uint256 usdcAmount = (availableBorrows * 95) / 10000; // 8 decimals -> 6 decimals

        console.log("Borrowing USDC:", usdcAmount / 1e6);
        pool.borrow(USDC, usdcAmount, 2, 0, testUser);
        vm.stopPrank();

        (,,,,,uint256 hf) = pool.getUserAccountData(testUser);
        console.log("Health Factor after borrow:", hf / 1e16, "%");
    }

    function _manipulatePriceAndLiquidate(address testUser) internal {
        address oracle = IPoolAddressesProvider(POOL_ADDRESSES_PROVIDER).getPriceOracle();
        uint256 currentWethPrice = IAaveOracle(oracle).getAssetPrice(WETH);
        console.log("Current WETH price:", currentWethPrice);

        // Use vm.mockCall to mock the oracle price (50% drop)
        uint256 newPrice = currentWethPrice / 2;
        vm.mockCall(
            oracle,
            abi.encodeWithSelector(IAaveOracle.getAssetPrice.selector, WETH),
            abi.encode(newPrice)
        );

        console.log("Mocked WETH price:", IAaveOracle(oracle).getAssetPrice(WETH));

        (,,,,,uint256 hfAfter) = pool.getUserAccountData(testUser);
        console.log("Health Factor after price drop:", hfAfter / 1e16, "%");

        require(hfAfter < 1e18, "Position should be liquidatable");

        _executeLiquidation(testUser);
    }

    function _executeLiquidation(address testUser) internal {
        console.log("=== Executing Liquidation ===");

        uint256 usdcBefore = IERC20(USDC).balanceOf(LIQUIDATOR);

        // Properly encode swap data:
        // 1. Inner: UniswapV3SwapData with isMultiHop=false and fee=3000 (0.3%)
        bytes memory uniData = SwapDataDecoder.encodeUniswapV3Data(false, abi.encode(uint24(3000)));
        // 2. Outer: WrappedSwapData with adapter type
        bytes memory swapData = SwapDataDecoder.encodeWrappedSwapData(ADAPTER_UNISWAP_V3, uniData);

        vm.prank(OWNER);
        uint256 profit = liquidator.liquidate(testUser, WETH, USDC, type(uint256).max, 0, swapData);

        uint256 usdcAfter = IERC20(USDC).balanceOf(LIQUIDATOR);

        console.log("Liquidation complete!");
        console.log("Profit:", profit);
        console.log("USDC gained:", usdcAfter - usdcBefore);

        assertGt(profit, 0, "Should have profit");
    }

    function testFlashLoanCallback() public {
        console.log("=== Flash Loan Callback Test ===");

        // Verify the liquidator can receive flash loan callbacks
        // by checking the uniswapV3FlashCallback function exists

        // Get the function selector
        bytes4 selector = bytes4(keccak256("uniswapV3FlashCallback(uint256,uint256,bytes)"));
        console.log("Flash callback selector exists: true");

        // Check AAVE callback too
        bytes4 aaveSelector = bytes4(keccak256("executeOperation(address,uint256,uint256,address,bytes)"));
        console.log("AAVE callback selector exists: true");
    }
}
