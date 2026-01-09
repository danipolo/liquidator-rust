//! Common contract interfaces shared across protocols.
//!
//! This module provides type definitions for standard interfaces like
//! ERC20 tokens, Chainlink oracles, and other common contracts.

use alloy::primitives::B256;
use alloy::sol;

// ERC20 interface for token interactions
sol! {
    /// Standard ERC20 interface (subset for liquidation needs)
    #[sol(rpc)]
    interface IERC20 {
        function balanceOf(address account) external view returns (uint256);
        function allowance(address owner, address spender) external view returns (uint256);
        function approve(address spender, uint256 amount) external returns (bool);
        function transfer(address to, uint256 amount) external returns (bool);
        function transferFrom(address from, address to, uint256 amount) external returns (bool);
        function decimals() external view returns (uint8);
        function symbol() external view returns (string);
    }
}

// Chainlink aggregator interface for oracle interactions
sol! {
    /// Chainlink-compatible oracle aggregator interface
    #[sol(rpc)]
    interface IAggregator {
        function latestAnswer() external view returns (int256);
        function latestRoundData() external view returns (
            uint80 roundId,
            int256 answer,
            uint256 startedAt,
            uint256 updatedAt,
            uint80 answeredInRound
        );
        function decimals() external view returns (uint8);

        event AnswerUpdated(int256 indexed current, uint256 indexed roundId, uint256 updatedAt);
    }
}

/// Event signature constants for common events.
pub mod common_signatures {
    use super::*;

    /// keccak256("AnswerUpdated(int256,uint256,uint256)")
    pub const ANSWER_UPDATED: B256 = B256::new([
        0x05, 0x59, 0x88, 0x4f, 0xd3, 0x34, 0x29, 0x55, 0xd1, 0xfc, 0x4b, 0x32, 0xf8, 0x0a, 0xb7,
        0x04, 0x98, 0x87, 0xe6, 0xe4, 0x32, 0x88, 0x03, 0x12, 0xfa, 0xea, 0x3c, 0x13, 0x6b, 0x0c,
        0xdb, 0xc4,
    ]);

    /// keccak256("Transfer(address,address,uint256)")
    pub const ERC20_TRANSFER: B256 = B256::new([
        0xdd, 0xf2, 0x52, 0xad, 0x1b, 0xe2, 0xc8, 0x9b, 0x69, 0xc2, 0xb0, 0x68, 0xfc, 0x37, 0x8d,
        0xaa, 0x95, 0x2b, 0xa7, 0xf1, 0x63, 0xc4, 0xa1, 0x16, 0x28, 0xf5, 0x5a, 0x4d, 0xf5, 0x23,
        0xb3, 0xef,
    ]);

    /// keccak256("Approval(address,address,uint256)")
    pub const ERC20_APPROVAL: B256 = B256::new([
        0x8c, 0x5b, 0xe1, 0xe5, 0xeb, 0xec, 0x7d, 0x5b, 0xd1, 0x4f, 0x71, 0x42, 0x7d, 0x1e, 0x84,
        0xf3, 0xdd, 0x03, 0x14, 0xc0, 0xf7, 0xb2, 0x29, 0x1e, 0x5b, 0x20, 0x0a, 0xc8, 0xc7, 0xc3,
        0xb9, 0x25,
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_common_signatures() {
        assert!(!common_signatures::ANSWER_UPDATED.is_zero());
        assert!(!common_signatures::ERC20_TRANSFER.is_zero());
        assert!(!common_signatures::ERC20_APPROVAL.is_zero());
    }
}
