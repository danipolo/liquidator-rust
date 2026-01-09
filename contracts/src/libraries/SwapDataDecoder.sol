// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {ILiquidSwap} from "../interfaces/ILiquidSwap.sol";

/// @title SwapDataDecoder
/// @notice Library for decoding swap data for different adapter types
/// @dev Provides type-safe decoding of adapter-specific swap parameters
library SwapDataDecoder {
    /// @notice Adapter type identifiers
    uint8 internal constant ADAPTER_LIQUIDSWAP = 0;
    uint8 internal constant ADAPTER_UNISWAP_V3 = 1;
    uint8 internal constant ADAPTER_DIRECT = 2;

    /// @notice Outer wrapper decoded first to route to correct adapter
    /// @param adapterType The adapter type identifier (0=LiquidSwap, 1=UniswapV3, 2=Direct)
    /// @param adapterData Adapter-specific payload
    struct WrappedSwapData {
        uint8 adapterType;
        bytes adapterData;
    }

    /// @notice LiquidSwap-specific swap data
    /// @param tokens Array of token addresses in the swap path
    /// @param hops Array of swap allocations per hop
    struct LiquidSwapData {
        address[] tokens;
        ILiquidSwap.Swap[][] hops;
    }

    /// @notice UniswapV3-specific swap data
    /// @param isMultiHop True for multi-hop swap, false for single-hop
    /// @param pathOrFee For single-hop: abi.encode(uint24 fee), for multi-hop: packed path bytes
    struct UniswapV3SwapData {
        bool isMultiHop;
        bytes pathOrFee;
    }

    /// @notice Decodes the outer wrapper to get adapter type and inner data
    /// @param data The encoded WrappedSwapData
    /// @return adapterType The adapter type identifier
    /// @return adapterData The adapter-specific encoded data
    function decodeWrappedSwapData(bytes memory data)
        internal
        pure
        returns (uint8 adapterType, bytes memory adapterData)
    {
        WrappedSwapData memory wrapped = abi.decode(data, (WrappedSwapData));
        return (wrapped.adapterType, wrapped.adapterData);
    }

    /// @notice Decodes LiquidSwap-specific swap data
    /// @param data The encoded LiquidSwapData
    /// @return tokens Array of token addresses
    /// @return hops Array of swap allocations
    function decodeLiquidSwapData(bytes memory data)
        internal
        pure
        returns (address[] memory tokens, ILiquidSwap.Swap[][] memory hops)
    {
        LiquidSwapData memory decoded = abi.decode(data, (LiquidSwapData));
        return (decoded.tokens, decoded.hops);
    }

    /// @notice Decodes UniswapV3-specific swap data
    /// @param data The encoded UniswapV3SwapData
    /// @return isMultiHop True for multi-hop swap
    /// @return pathOrFee The path or fee data
    function decodeUniswapV3Data(bytes memory data)
        internal
        pure
        returns (bool isMultiHop, bytes memory pathOrFee)
    {
        UniswapV3SwapData memory decoded = abi.decode(data, (UniswapV3SwapData));
        return (decoded.isMultiHop, decoded.pathOrFee);
    }

    /// @notice Encodes swap data with adapter wrapper
    /// @param adapterType The adapter type identifier
    /// @param adapterData The adapter-specific data
    /// @return The encoded WrappedSwapData
    function encodeWrappedSwapData(uint8 adapterType, bytes memory adapterData)
        internal
        pure
        returns (bytes memory)
    {
        return abi.encode(WrappedSwapData({adapterType: adapterType, adapterData: adapterData}));
    }

    /// @notice Encodes LiquidSwap data
    /// @param tokens Array of token addresses
    /// @param hops Array of swap allocations
    /// @return The encoded LiquidSwapData
    function encodeLiquidSwapData(address[] memory tokens, ILiquidSwap.Swap[][] memory hops)
        internal
        pure
        returns (bytes memory)
    {
        return abi.encode(LiquidSwapData({tokens: tokens, hops: hops}));
    }

    /// @notice Encodes UniswapV3 data
    /// @param isMultiHop True for multi-hop swap
    /// @param pathOrFee The path or fee data
    /// @return The encoded UniswapV3SwapData
    function encodeUniswapV3Data(bool isMultiHop, bytes memory pathOrFee) internal pure returns (bytes memory) {
        return abi.encode(UniswapV3SwapData({isMultiHop: isMultiHop, pathOrFee: pathOrFee}));
    }
}
