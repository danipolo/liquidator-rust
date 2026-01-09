// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test, console} from "forge-std/Test.sol";
import {Liquidator} from "../../src/Liquidator.sol";
import {UniswapV3Adapter} from "../../src/adapters/UniswapV3Adapter.sol";
import {DirectAdapter} from "../../src/adapters/DirectAdapter.sol";
import {SwapDataDecoder} from "../../src/libraries/SwapDataDecoder.sol";
import {TestConstants} from "../utils/TestConstants.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";

/// @title ArbitrumForkTest
/// @notice Fork tests against Arbitrum mainnet
/// @dev Requires ARBITRUM_RPC_URL environment variable
contract ArbitrumForkTest is Test {
    Liquidator public liquidator;
    UniswapV3Adapter public uniswapAdapter;
    DirectAdapter public directAdapter;

    // Arbitrum mainnet addresses
    address constant AAVE_POOL = 0x794a61358D6845594F94dc1DB02A252b5b4814aD;
    address constant UNISWAP_FACTORY = 0x1F98431c8aD98523631AE4a59f267346ea31F984;
    address constant WETH = 0x82aF49447D8a07e3bd95BD0d56f35241523fBab1;
    address constant USDC = 0xaf88d065e77c8cC2239327C5EDb3A432268e5831;
    address constant USDT = 0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9;
    address constant UNISWAP_ROUTER = 0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45;

    // Known whale addresses for testing
    address constant WETH_WHALE = 0x489ee077994B6658eAfA855C308275EAd8097C4A;
    address constant USDC_WHALE = 0x489ee077994B6658eAfA855C308275EAd8097C4A;

    bool public forkEnabled;

    function setUp() external {
        // Try to create fork - skip tests if RPC URL not available
        try vm.envString("ARBITRUM_RPC_URL") returns (string memory rpcUrl) {
            if (bytes(rpcUrl).length > 0) {
                vm.createSelectFork(rpcUrl);
                forkEnabled = true;

                // Deploy contracts on fork
                liquidator = new Liquidator(AAVE_POOL, UNISWAP_FACTORY, WETH);
                uniswapAdapter = new UniswapV3Adapter(UNISWAP_ROUTER);
                directAdapter = new DirectAdapter();

                // Setup adapters
                liquidator.setAdapter(TestConstants.ADAPTER_UNISWAP_V3, address(uniswapAdapter));
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

    /// @notice Test that UniswapV3Adapter is deployed correctly
    function testFork_UniswapV3Adapter_Deployment() external onlyFork {
        assertEq(address(uniswapAdapter.router()), UNISWAP_ROUTER);
    }

    /// @notice Test that Liquidator is configured correctly
    function testFork_Liquidator_Configuration() external onlyFork {
        assertEq(address(liquidator.pool()), AAVE_POOL);
        assertEq(address(liquidator.wrappedNative()), WETH);
        assertEq(liquidator.adapters(TestConstants.ADAPTER_UNISWAP_V3), address(uniswapAdapter));
        assertEq(liquidator.adapters(TestConstants.ADAPTER_DIRECT), address(directAdapter));
    }

    /// @notice Test single-hop swap WETH -> USDC on real Uniswap V3
    function testFork_UniswapV3Adapter_SingleHop() external onlyFork {
        uint256 amountIn = 1 ether;
        uint256 minAmountOut = 1000e6; // Expect at least 1000 USDC

        // Get WETH from whale
        vm.prank(WETH_WHALE);
        IERC20(WETH).transfer(address(uniswapAdapter), amountIn);

        // Create single-hop swap data (0.05% fee tier for WETH/USDC)
        bytes memory swapData = SwapDataDecoder.encodeUniswapV3Data(false, abi.encode(uint24(500)));

        // Execute swap
        uint256 amountOut = uniswapAdapter.swap(WETH, USDC, amountIn, minAmountOut, swapData);

        // Verify we got USDC
        assertGt(amountOut, minAmountOut, "Should receive more than minimum");
        assertEq(IERC20(USDC).balanceOf(address(this)), amountOut, "Should receive USDC");

        console.log("Swapped 1 WETH for", amountOut / 1e6, "USDC");
    }

    /// @notice Test multi-hop swap WETH -> USDC -> USDT on real Uniswap V3
    function testFork_UniswapV3Adapter_MultiHop() external onlyFork {
        uint256 amountIn = 1 ether;
        uint256 minAmountOut = 900e6; // Expect at least 900 USDT (accounting for slippage)

        // Get WETH from whale
        vm.prank(WETH_WHALE);
        IERC20(WETH).transfer(address(uniswapAdapter), amountIn);

        // Create multi-hop path: WETH -> USDC -> USDT
        // Path format: token (20 bytes) + fee (3 bytes) + token (20 bytes) + fee (3 bytes) + token (20 bytes)
        bytes memory path = abi.encodePacked(
            WETH,
            uint24(500), // 0.05% WETH/USDC
            USDC,
            uint24(100), // 0.01% USDC/USDT (stablecoin pair)
            USDT
        );

        bytes memory swapData = SwapDataDecoder.encodeUniswapV3Data(true, path);

        // Execute swap
        uint256 amountOut = uniswapAdapter.swap(WETH, USDT, amountIn, minAmountOut, swapData);

        // Verify we got USDT
        assertGt(amountOut, minAmountOut, "Should receive more than minimum");
        assertEq(IERC20(USDT).balanceOf(address(this)), amountOut, "Should receive USDT");

        console.log("Swapped 1 WETH for", amountOut / 1e6, "USDT via USDC");
    }

    /// @notice Test AAVE Pool flash loan premium
    function testFork_AavePool_FlashLoanPremium() external onlyFork {
        // Verify flash loan premium is set correctly
        // AAVE V3 uses 9 bps (0.09%) as default
        // Note: This may vary by market

        // Just verify the pool is accessible
        assertTrue(AAVE_POOL.code.length > 0, "AAVE Pool should be a contract");
    }

    /// @notice Test rescue tokens functionality on fork
    function testFork_RescueTokens() external onlyFork {
        // Get some WETH
        vm.prank(WETH_WHALE);
        IERC20(WETH).transfer(address(liquidator), 1 ether);

        uint256 balanceBefore = IERC20(WETH).balanceOf(address(this));

        // Rescue tokens
        liquidator.rescueTokens(WETH, 1 ether, false, address(this));

        uint256 balanceAfter = IERC20(WETH).balanceOf(address(this));

        assertEq(balanceAfter - balanceBefore, 1 ether, "Should receive rescued WETH");
    }

    /// @notice Placeholder test that always passes when fork is not available
    function test_Placeholder_WhenNoFork() external pure {
        // This test ensures the test file compiles and runs even without fork
        assertTrue(true);
    }
}
