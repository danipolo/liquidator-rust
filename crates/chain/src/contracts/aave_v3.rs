//! AAVE V3 contract interfaces.
//!
//! This module provides type definitions and ABI bindings for AAVE V3
//! protocol contracts, including the Pool, Oracle, and custom Liquidator.

use alloy::primitives::{Address, Bytes, B256, U256};
use alloy::sol;
use alloy::sol_types::{SolCall, SolType};

// AAVE V3 Pool interface
sol! {
    /// Aave V3 Pool interface (subset for liquidation)
    interface IPool {
        event Supply(address indexed reserve, address user, address indexed onBehalfOf, uint256 amount, uint16 indexed referralCode);
        event Withdraw(address indexed reserve, address indexed user, address indexed to, uint256 amount);
        event Borrow(address indexed reserve, address user, address indexed onBehalfOf, uint256 amount, uint8 interestRateMode, uint256 borrowRate, uint16 indexed referralCode);
        event Repay(address indexed reserve, address indexed user, address indexed repayer, uint256 amount, bool useATokens);
        event LiquidationCall(address indexed collateralAsset, address indexed debtAsset, address indexed user, uint256 debtToCover, uint256 liquidatedCollateralAmount, address liquidator, bool receiveAToken);

        /// Liquidate a position - direct pool call (without flash loan)
        function liquidationCall(
            address collateralAsset,
            address debtAsset,
            address user,
            uint256 debtToCover,
            bool receiveAToken
        ) external;
    }
}

// Custom liquidator contract interface with generic swap data
sol! {
    /// Swap allocation struct for LiquidSwap adapter (matches Solidity tuple)
    #[derive(Debug)]
    struct SwapAlloc {
        address tokenIn;
        address tokenOut;
        uint8 routerIndex;
        uint24 fee;
        uint256 amountIn;
        bool stable;
    }

    /// Custom Liquidator contract interface (new signature with bytes swapData)
    interface ILiquidator {
        function liquidate(
            address _user,
            address _collateral,
            address _debt,
            uint256 _debtAmount,
            uint256 _minAmountOut,
            bytes calldata _swapData
        ) external returns (uint256);

        function rescueTokens(
            address _token,
            uint256 _amount,
            bool _max,
            address _to
        ) external;
    }
}

/// Event signature constants for AAVE V3.
pub mod aave_v3_signatures {
    use super::*;

    /// keccak256("Supply(address,address,address,uint256,uint16)")
    pub const SUPPLY: B256 = B256::new([
        0x2b, 0x62, 0x7c, 0xe5, 0x32, 0x47, 0xe1, 0x4b, 0x2c, 0x94, 0x3c, 0xb3, 0x84, 0xf6, 0x22,
        0xb9, 0x70, 0x64, 0x99, 0x4c, 0x68, 0x32, 0x18, 0x0f, 0x2a, 0x71, 0x7c, 0x7f, 0xa2, 0xac,
        0xe2, 0x9e,
    ]);

    /// keccak256("Withdraw(address,address,address,uint256)")
    pub const WITHDRAW: B256 = B256::new([
        0x31, 0x15, 0xd1, 0x44, 0x9a, 0x7b, 0x73, 0x2c, 0x4a, 0x14, 0x53, 0x4b, 0x82, 0x26, 0x19,
        0xf7, 0x2c, 0xc4, 0xd7, 0x0e, 0xf5, 0x2d, 0x8e, 0x0e, 0x2a, 0x7d, 0x6d, 0x80, 0x6b, 0x48,
        0xd8, 0x39,
    ]);

    /// keccak256("Borrow(address,address,address,uint256,uint8,uint256,uint16)")
    pub const BORROW: B256 = B256::new([
        0xb3, 0xd0, 0x84, 0x82, 0x0f, 0xb1, 0xa9, 0xde, 0xcf, 0xef, 0xf7, 0xce, 0x23, 0xfb, 0x0d,
        0xb6, 0x95, 0x43, 0xa8, 0xae, 0x27, 0x5f, 0xde, 0x06, 0x3a, 0xba, 0xf5, 0x81, 0x2f, 0x3c,
        0xc5, 0x88,
    ]);

    /// keccak256("Repay(address,address,address,uint256,bool)")
    pub const REPAY: B256 = B256::new([
        0xa5, 0x34, 0xc8, 0xdc, 0xe0, 0x52, 0x79, 0xf5, 0xb3, 0x05, 0xbd, 0xfd, 0xa9, 0x35, 0x48,
        0x8f, 0xf4, 0xf1, 0xc8, 0x3d, 0xd2, 0x62, 0x1e, 0x7e, 0xb0, 0x56, 0xd7, 0xa5, 0x93, 0x98,
        0x74, 0x80,
    ]);

    /// keccak256("LiquidationCall(address,address,address,uint256,uint256,address,bool)")
    pub const LIQUIDATION_CALL: B256 = B256::new([
        0xe4, 0x13, 0xa3, 0x21, 0xe8, 0x68, 0x14, 0x69, 0x7e, 0x5d, 0x12, 0x0c, 0xb6, 0x28, 0x45,
        0x1e, 0x97, 0x08, 0x86, 0x7c, 0xfd, 0x6a, 0x6c, 0xd8, 0x16, 0xd2, 0xe7, 0xb0, 0xb4, 0xd0,
        0xb4, 0x80,
    ]);

    /// Get all pool event signatures.
    pub fn pool_signatures() -> Vec<B256> {
        vec![SUPPLY, WITHDRAW, BORROW, REPAY, LIQUIDATION_CALL]
    }
}

/// Swap adapter type - determines how swapData is encoded.
/// The adapter ID is included in the encoded swapData so the contract
/// can dynamically route to the correct adapter without redeployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum SwapAdapter {
    /// LiquidSwap adapter: abi.encode(tokens[], hops[][])
    /// Adapter ID: 0
    #[default]
    LiquidSwap = 0,
    /// Uniswap V3 adapter: abi.encode(isMultiHop, pathOrFee)
    /// Adapter ID: 1
    UniswapV3 = 1,
    /// Direct swap (no routing, 1:1 swap via pool)
    /// Adapter ID: 2
    Direct = 2,
    // Note: Adapter IDs 3+ are reserved for future adapters
}

impl SwapAdapter {
    /// Get the adapter ID for encoding.
    pub fn id(&self) -> u8 {
        *self as u8
    }

    /// Get adapter for a chain ID (default adapter for the chain).
    pub fn for_chain(chain_id: u64) -> Self {
        match chain_id {
            // HyperLiquid uses LiquidSwap
            998 | 999 => Self::LiquidSwap,
            // Plasma, Celo, Arbitrum, Base, Optimism use Uniswap V3
            9745 | 42220 | 42161 | 8453 | 10 => Self::UniswapV3,
            // Default to LiquidSwap for unknown chains
            _ => Self::LiquidSwap,
        }
    }

    /// Create adapter from ID.
    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::LiquidSwap),
            1 => Some(Self::UniswapV3),
            2 => Some(Self::Direct),
            _ => None,
        }
    }
}

/// Swap allocation for liquidation (Rust representation).
#[derive(Debug, Clone)]
pub struct SwapAllocation {
    pub token_in: Address,
    pub token_out: Address,
    pub router_index: u8,
    pub fee: u32, // uint24 in Solidity, stored as u32
    pub amount_in: U256,
    pub stable: bool,
}

impl SwapAllocation {
    /// Convert to the ABI-compatible SwapAlloc type.
    pub fn to_sol(&self) -> SwapAlloc {
        SwapAlloc {
            tokenIn: self.token_in,
            tokenOut: self.token_out,
            routerIndex: self.router_index,
            fee: alloy::primitives::Uint::<24, 1>::from(self.fee & 0xFFFFFF),
            amountIn: self.amount_in,
            stable: self.stable,
        }
    }
}

// Helper types for ABI encoding
sol! {
    /// Wrapper for self-describing swap data: (adapterType, adapterData)
    /// This allows the contract to dynamically route to the correct adapter
    /// without redeployment when new adapters are added.
    #[derive(Debug)]
    struct WrappedSwapData {
        uint8 adapterType;
        bytes adapterData;
    }

    /// LiquidSwap swap data: (tokens[], hops[][])
    #[derive(Debug)]
    struct LiquidSwapData {
        address[] tokens;
        SwapAlloc[][] hops;
    }

    /// UniswapV3 swap data: (isMultiHop, pathOrFee)
    #[derive(Debug)]
    struct UniswapV3SwapData {
        bool isMultiHop;
        bytes pathOrFee;
    }
}

/// Wrap adapter-specific data with adapter type for self-describing swapData.
///
/// Format: `abi.encode(uint8 adapterType, bytes adapterData)`
///
/// This allows the contract to dynamically route to the correct adapter
/// based on the adapterType field, enabling new adapters to be added
/// without redeploying the liquidator contract.
pub fn wrap_swap_data(adapter: SwapAdapter, adapter_data: Bytes) -> Bytes {
    let wrapped = WrappedSwapData {
        adapterType: adapter.id(),
        adapterData: adapter_data,
    };
    Bytes::from(WrappedSwapData::abi_encode(&wrapped))
}

/// Encode swap data for LiquidSwap adapter (raw, without wrapper).
/// Format: abi.encode(address[] tokens, SwapAlloc[][] hops)
fn encode_liquidswap_data_raw(
    hops: Vec<Vec<SwapAllocation>>,
    tokens: Vec<Address>,
) -> Bytes {
    let encoded_hops: Vec<Vec<SwapAlloc>> = hops
        .into_iter()
        .map(|hop| hop.into_iter().map(|a| a.to_sol()).collect())
        .collect();

    let data = LiquidSwapData {
        tokens,
        hops: encoded_hops,
    };
    Bytes::from(LiquidSwapData::abi_encode(&data))
}

/// Encode swap data for LiquidSwap adapter (wrapped with adapter type).
/// Format: abi.encode(uint8 adapterType=0, bytes adapterData)
pub fn encode_liquidswap_data(
    hops: Vec<Vec<SwapAllocation>>,
    tokens: Vec<Address>,
) -> Bytes {
    let raw = encode_liquidswap_data_raw(hops, tokens);
    wrap_swap_data(SwapAdapter::LiquidSwap, raw)
}

/// Encode swap data for Uniswap V3 adapter (raw, without wrapper).
/// Single-hop format: abi.encode(false, abi.encode(uint24 fee))
/// Multi-hop format: abi.encode(true, bytes packedPath)
fn encode_uniswap_v3_data_raw(
    tokens: &[Address],
    fee: u32,
) -> Bytes {
    if tokens.len() == 2 {
        // Single-hop: abi.encode(false, abi.encode(fee))
        // Fee is encoded as uint24 (3 bytes padded to 32 bytes in ABI)
        let fee_u24 = alloy::primitives::Uint::<24, 1>::from(fee & 0xFFFFFF);
        type FeeData = alloy::sol_types::sol_data::Uint<24>;
        let inner_encoded = FeeData::abi_encode(&fee_u24);

        let data = UniswapV3SwapData {
            isMultiHop: false,
            pathOrFee: inner_encoded.into(),
        };
        Bytes::from(UniswapV3SwapData::abi_encode(&data))
    } else {
        // Multi-hop: pack path as [token0, fee0, token1, fee1, token2, ...]
        let mut packed_path = Vec::new();
        for (i, token) in tokens.iter().enumerate() {
            packed_path.extend_from_slice(token.as_slice());
            if i < tokens.len() - 1 {
                // Add fee as 3 bytes (uint24)
                let fee_bytes = [(fee >> 16) as u8, (fee >> 8) as u8, fee as u8];
                packed_path.extend_from_slice(&fee_bytes);
            }
        }

        let data = UniswapV3SwapData {
            isMultiHop: true,
            pathOrFee: packed_path.into(),
        };
        Bytes::from(UniswapV3SwapData::abi_encode(&data))
    }
}

/// Encode swap data for Uniswap V3 adapter (wrapped with adapter type).
/// Format: abi.encode(uint8 adapterType=1, bytes adapterData)
pub fn encode_uniswap_v3_data(
    tokens: &[Address],
    fee: u32,
) -> Bytes {
    let raw = encode_uniswap_v3_data_raw(tokens, fee);
    wrap_swap_data(SwapAdapter::UniswapV3, raw)
}

/// Encode liquidation calldata for the new contract interface.
///
/// New signature: liquidate(user, collateral, debt, debtAmount, minAmountOut, swapData)
pub fn encode_liquidation(
    user: Address,
    collateral: Address,
    debt: Address,
    debt_to_cover: U256,
    min_amount_out: U256,
    swap_data: Bytes,
) -> Bytes {
    let call = ILiquidator::liquidateCall {
        _user: user,
        _collateral: collateral,
        _debt: debt,
        _debtAmount: debt_to_cover,
        _minAmountOut: min_amount_out,
        _swapData: swap_data,
    };

    Bytes::from(call.abi_encode())
}

/// Encode swap data for Direct adapter (no DEX routing).
/// Format: abi.encode(uint8 adapterType=2, bytes empty)
pub fn encode_direct_swap_data() -> Bytes {
    wrap_swap_data(SwapAdapter::Direct, Bytes::new())
}

/// Encode liquidation with adapter-specific swap data.
pub fn encode_liquidation_with_adapter(
    user: Address,
    collateral: Address,
    debt: Address,
    debt_to_cover: U256,
    min_amount_out: U256,
    adapter: SwapAdapter,
    hops: Vec<Vec<SwapAllocation>>,
    tokens: Vec<Address>,
) -> Bytes {
    let swap_data = match adapter {
        SwapAdapter::LiquidSwap => encode_liquidswap_data(hops, tokens),
        SwapAdapter::UniswapV3 => {
            // Extract fee from first allocation (single-hop assumed)
            let fee = hops.first()
                .and_then(|h| h.first())
                .map(|a| a.fee)
                .unwrap_or(3000);
            encode_uniswap_v3_data(&tokens, fee)
        }
        SwapAdapter::Direct => encode_direct_swap_data(),
    };

    encode_liquidation(user, collateral, debt, debt_to_cover, min_amount_out, swap_data)
}

/// Encode direct pool liquidation calldata (without flash loan).
pub fn encode_pool_liquidation(
    collateral: Address,
    debt: Address,
    user: Address,
    debt_to_cover: U256,
    receive_atoken: bool,
) -> Bytes {
    let call = IPool::liquidationCallCall {
        collateralAsset: collateral,
        debtAsset: debt,
        user,
        debtToCover: debt_to_cover,
        receiveAToken: receive_atoken,
    };

    Bytes::from(call.abi_encode())
}

/// Encode rescue tokens calldata (rescues all tokens).
pub fn encode_rescue_tokens(token: Address, recipient: Address) -> Bytes {
    let call = ILiquidator::rescueTokensCall {
        _token: token,
        _amount: U256::ZERO,
        _max: true,
        _to: recipient,
    };
    Bytes::from(call.abi_encode())
}

/// Encode rescue tokens calldata with specific amount.
pub fn encode_rescue_tokens_amount(token: Address, amount: U256, recipient: Address) -> Bytes {
    let call = ILiquidator::rescueTokensCall {
        _token: token,
        _amount: amount,
        _max: false,
        _to: recipient,
    };
    Bytes::from(call.abi_encode())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aave_v3_signatures() {
        let sigs = aave_v3_signatures::pool_signatures();
        assert_eq!(sigs.len(), 5);
        assert!(!aave_v3_signatures::SUPPLY.is_zero());
        assert!(!aave_v3_signatures::LIQUIDATION_CALL.is_zero());
    }

    #[test]
    fn test_encode_liquidation() {
        let calldata = encode_liquidation(
            Address::ZERO,
            Address::ZERO,
            Address::ZERO,
            U256::from(1000),
            U256::ZERO,
            Bytes::new(),
        );
        // Should produce valid calldata
        assert!(!calldata.is_empty());
    }

    #[test]
    fn test_encode_liquidswap_data() {
        let hops = vec![vec![SwapAllocation {
            token_in: Address::ZERO,
            token_out: Address::ZERO,
            router_index: 0,
            fee: 3000,
            amount_in: U256::from(1000),
            stable: false,
        }]];
        let tokens = vec![Address::ZERO, Address::ZERO];
        let swap_data = encode_liquidswap_data(hops, tokens);
        assert!(!swap_data.is_empty());
    }

    #[test]
    fn test_encode_uniswap_v3_single_hop() {
        let tokens = vec![Address::ZERO, Address::ZERO];
        let swap_data = encode_uniswap_v3_data(&tokens, 3000);
        assert!(!swap_data.is_empty());
    }

    #[test]
    fn test_encode_uniswap_v3_multi_hop() {
        let tokens = vec![Address::ZERO, Address::ZERO, Address::ZERO];
        let swap_data = encode_uniswap_v3_data(&tokens, 3000);
        assert!(!swap_data.is_empty());
    }

    #[test]
    fn test_swap_adapter_for_chain() {
        assert_eq!(SwapAdapter::for_chain(998), SwapAdapter::LiquidSwap);
        assert_eq!(SwapAdapter::for_chain(999), SwapAdapter::LiquidSwap);
        assert_eq!(SwapAdapter::for_chain(9745), SwapAdapter::UniswapV3);
        assert_eq!(SwapAdapter::for_chain(42220), SwapAdapter::UniswapV3);
        assert_eq!(SwapAdapter::for_chain(42161), SwapAdapter::UniswapV3);
    }
}
