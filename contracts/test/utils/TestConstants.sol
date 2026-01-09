// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title TestConstants
/// @notice Constants for testing across different chains
library TestConstants {
    // Adapter type identifiers
    uint8 internal constant ADAPTER_LIQUIDSWAP = 0;
    uint8 internal constant ADAPTER_UNISWAP_V3 = 1;
    uint8 internal constant ADAPTER_DIRECT = 2;

    // HyperLiquid mainnet
    address internal constant HYPERLIQUID_AAVE_POOL = address(0); // TBD
    address internal constant WHYPE = 0x5555555555555555555555555555555555555555;
    address internal constant LIQUIDSWAP_ROUTER = 0x744489Ee3d540777A66f2cf297479745e0852f7A;

    // Arbitrum mainnet
    address internal constant ARBITRUM_AAVE_POOL = 0x794a61358D6845594F94dc1DB02A252b5b4814aD;
    address internal constant ARBITRUM_WETH = 0x82aF49447D8a07e3bd95BD0d56f35241523fBab1;
    address internal constant ARBITRUM_USDC = 0xaf88d065e77c8cC2239327C5EDb3A432268e5831;
    address internal constant ARBITRUM_USDT = 0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9;
    address internal constant ARBITRUM_UNISWAP_ROUTER = 0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45;

    // Base mainnet
    address internal constant BASE_AAVE_POOL = 0xA238Dd80C259a72e81d7e4664a9801593F98d1c5;
    address internal constant BASE_WETH = 0x4200000000000000000000000000000000000006;
    address internal constant BASE_USDC = 0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913;
    address internal constant BASE_UNISWAP_ROUTER = 0x2626664c2603336E57B271c5C0b26F421741e481;

    // Optimism mainnet
    address internal constant OPTIMISM_AAVE_POOL = 0x794a61358D6845594F94dc1DB02A252b5b4814aD;
    address internal constant OPTIMISM_WETH = 0x4200000000000000000000000000000000000006;
    address internal constant OPTIMISM_USDC = 0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85;
    address internal constant OPTIMISM_UNISWAP_ROUTER = 0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45;

    // Common fee tiers for Uniswap V3
    uint24 internal constant FEE_LOWEST = 100; // 0.01%
    uint24 internal constant FEE_LOW = 500; // 0.05%
    uint24 internal constant FEE_MEDIUM = 3000; // 0.30%
    uint24 internal constant FEE_HIGH = 10000; // 1.00%

    // Test amounts
    uint256 internal constant ONE_ETH = 1 ether;
    uint256 internal constant ONE_USDC = 1e6;
    uint256 internal constant ONE_THOUSAND_USDC = 1000e6;
}
