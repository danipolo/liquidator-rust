//! Protocol event types and signatures.
//!
//! This module defines event types that are common across different lending
//! protocols, along with their Keccak256 signatures for log filtering.

use alloy::primitives::{Address, B256, U256};

/// Event signatures for log subscription.
///
/// These are the Keccak256 hashes of event signatures for filtering logs.
/// Different protocols may have different events, so all fields are optional.
#[derive(Debug, Clone, Default)]
pub struct ProtocolEventSignatures {
    /// Supply/Deposit event
    pub supply: Option<B256>,
    /// Withdraw event
    pub withdraw: Option<B256>,
    /// Borrow event
    pub borrow: Option<B256>,
    /// Repay event
    pub repay: Option<B256>,
    /// Liquidation event
    pub liquidation: Option<B256>,
    /// Reserve data updated (AAVE-specific)
    pub reserve_data_updated: Option<B256>,
    /// Interest rate update
    pub interest_rate_update: Option<B256>,
}

impl ProtocolEventSignatures {
    /// Create signatures for AAVE V3.
    pub fn aave_v3() -> Self {
        use alloy::primitives::keccak256;

        Self {
            // Supply(address indexed reserve, address user, address indexed onBehalfOf, uint256 amount, uint16 indexed referralCode)
            supply: Some(keccak256(
                "Supply(address,address,address,uint256,uint16)",
            )),
            // Withdraw(address indexed reserve, address indexed user, address indexed to, uint256 amount)
            withdraw: Some(keccak256(
                "Withdraw(address,address,address,uint256)",
            )),
            // Borrow(address indexed reserve, address user, address indexed onBehalfOf, uint256 amount, uint8 interestRateMode, uint256 borrowRate, uint16 indexed referralCode)
            borrow: Some(keccak256(
                "Borrow(address,address,address,uint256,uint8,uint256,uint16)",
            )),
            // Repay(address indexed reserve, address indexed user, address indexed repayer, uint256 amount, bool useATokens)
            repay: Some(keccak256(
                "Repay(address,address,address,uint256,bool)",
            )),
            // LiquidationCall(address indexed collateralAsset, address indexed debtAsset, address indexed user, uint256 debtToCover, uint256 liquidatedCollateralAmount, address liquidator, bool receiveAToken)
            liquidation: Some(keccak256(
                "LiquidationCall(address,address,address,uint256,uint256,address,bool)",
            )),
            // ReserveDataUpdated(address indexed reserve, uint256 liquidityRate, uint256 stableBorrowRate, uint256 variableBorrowRate, uint256 liquidityIndex, uint256 variableBorrowIndex)
            reserve_data_updated: Some(keccak256(
                "ReserveDataUpdated(address,uint256,uint256,uint256,uint256,uint256)",
            )),
            interest_rate_update: None,
        }
    }

    /// Create signatures for Compound V3 (Comet).
    pub fn compound_v3() -> Self {
        use alloy::primitives::keccak256;

        Self {
            // Supply(address indexed from, address indexed dst, uint256 amount)
            supply: Some(keccak256("Supply(address,address,uint256)")),
            // Withdraw(address indexed src, address indexed to, uint256 amount)
            withdraw: Some(keccak256("Withdraw(address,address,uint256)")),
            // We use SupplyCollateral for collateral tracking
            borrow: None,
            repay: None,
            // AbsorbCollateral(address indexed absorber, address indexed borrower, address indexed asset, uint256 collateralAbsorbed, uint256 usdValue)
            liquidation: Some(keccak256(
                "AbsorbCollateral(address,address,address,uint256,uint256)",
            )),
            reserve_data_updated: None,
            interest_rate_update: None,
        }
    }

    /// Get all non-None signatures as a vector.
    pub fn all_signatures(&self) -> Vec<B256> {
        [
            self.supply,
            self.withdraw,
            self.borrow,
            self.repay,
            self.liquidation,
            self.reserve_data_updated,
            self.interest_rate_update,
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

/// Pool event type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolEventType {
    /// User supplied/deposited assets
    Supply,
    /// User withdrew assets
    Withdraw,
    /// User borrowed assets
    Borrow,
    /// User repaid debt
    Repay,
    /// Liquidation occurred
    Liquidation,
    /// Reserve data was updated
    ReserveUpdate,
    /// Unknown event type
    Unknown,
}

/// Decoded pool event with common fields.
#[derive(Debug, Clone)]
pub struct PoolEvent {
    /// Event type
    pub event_type: PoolEventType,
    /// Reserve/asset address
    pub asset: Address,
    /// User address (affected party)
    pub user: Address,
    /// Amount involved
    pub amount: U256,
    /// Block number
    pub block_number: u64,
    /// Transaction hash
    pub tx_hash: B256,
    /// Log index within the transaction
    pub log_index: u64,
}

impl PoolEvent {
    /// Check if this event affects a user's position.
    pub fn affects_position(&self, user: Address) -> bool {
        self.user == user
    }

    /// Check if this is a supply-related event (Supply or Withdraw).
    pub fn is_supply_event(&self) -> bool {
        matches!(self.event_type, PoolEventType::Supply | PoolEventType::Withdraw)
    }

    /// Check if this is a borrow-related event (Borrow or Repay).
    pub fn is_borrow_event(&self) -> bool {
        matches!(self.event_type, PoolEventType::Borrow | PoolEventType::Repay)
    }

    /// Check if this is a liquidation event.
    pub fn is_liquidation(&self) -> bool {
        matches!(self.event_type, PoolEventType::Liquidation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aave_v3_signatures() {
        let sigs = ProtocolEventSignatures::aave_v3();
        assert!(sigs.supply.is_some());
        assert!(sigs.withdraw.is_some());
        assert!(sigs.borrow.is_some());
        assert!(sigs.repay.is_some());
        assert!(sigs.liquidation.is_some());
    }

    #[test]
    fn test_compound_v3_signatures() {
        let sigs = ProtocolEventSignatures::compound_v3();
        assert!(sigs.supply.is_some());
        assert!(sigs.withdraw.is_some());
        assert!(sigs.liquidation.is_some());
        // Compound V3 doesn't have separate borrow/repay events
        assert!(sigs.borrow.is_none());
    }

    #[test]
    fn test_all_signatures() {
        let sigs = ProtocolEventSignatures::aave_v3();
        let all = sigs.all_signatures();
        assert!(all.len() >= 5);
    }
}
