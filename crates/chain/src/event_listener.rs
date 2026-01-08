//! WebSocket event listener for real-time oracle and pool events.

use alloy::primitives::{Address, B256, I256, U256};
use alloy::providers::{Provider, ProviderBuilder, WsConnect};
use alloy::rpc::types::{Filter, Log};
use anyhow::Result;
use futures::stream::{Stream, StreamExt};
use std::pin::Pin;
use tracing::{debug, info, warn};

use crate::contracts::{event_signatures, OracleAggregator, PoolContract};

/// Oracle type for price feed categorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OracleType {
    Standard,
    RedStone,
    Pyth,
    DualOracle,
    PendlePT,
}

/// Oracle price update event.
#[derive(Debug, Clone)]
pub struct OracleUpdate {
    /// Oracle aggregator address that emitted the event
    pub oracle: Address,
    /// Asset address this oracle is for
    pub asset: Address,
    /// New price (8 decimals)
    pub price: U256,
    /// Round ID
    pub round_id: U256,
    /// Timestamp of the update
    pub timestamp: u64,
    /// Block number
    pub block_number: u64,
    /// Transaction hash
    pub tx_hash: B256,
    /// Oracle type
    pub oracle_type: OracleType,
}

/// Pool event types.
#[derive(Debug, Clone)]
pub enum PoolEvent {
    Supply {
        reserve: Address,
        user: Address,
        on_behalf_of: Address,
        amount: U256,
        block_number: u64,
        tx_hash: B256,
    },
    Withdraw {
        reserve: Address,
        user: Address,
        to: Address,
        amount: U256,
        block_number: u64,
        tx_hash: B256,
    },
    Borrow {
        reserve: Address,
        user: Address,
        on_behalf_of: Address,
        amount: U256,
        block_number: u64,
        tx_hash: B256,
    },
    Repay {
        reserve: Address,
        user: Address,
        repayer: Address,
        amount: U256,
        block_number: u64,
        tx_hash: B256,
    },
    LiquidationCall {
        collateral_asset: Address,
        debt_asset: Address,
        user: Address,
        debt_to_cover: U256,
        liquidated_collateral: U256,
        liquidator: Address,
        block_number: u64,
        tx_hash: B256,
    },
}

impl PoolEvent {
    /// Get the user affected by this event.
    pub fn user(&self) -> Address {
        match self {
            Self::Supply { on_behalf_of, .. } => *on_behalf_of,
            Self::Withdraw { user, .. } => *user,
            Self::Borrow { on_behalf_of, .. } => *on_behalf_of,
            Self::Repay { user, .. } => *user,
            Self::LiquidationCall { user, .. } => *user,
        }
    }

    /// Get the event type name.
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::Supply { .. } => "Supply",
            Self::Withdraw { .. } => "Withdraw",
            Self::Borrow { .. } => "Borrow",
            Self::Repay { .. } => "Repay",
            Self::LiquidationCall { .. } => "LiquidationCall",
        }
    }

    /// Get the block number.
    pub fn block_number(&self) -> u64 {
        match self {
            Self::Supply { block_number, .. }
            | Self::Withdraw { block_number, .. }
            | Self::Borrow { block_number, .. }
            | Self::Repay { block_number, .. }
            | Self::LiquidationCall { block_number, .. } => *block_number,
        }
    }
}

/// WebSocket event listener for real-time events.
pub struct EventListener {
    /// WebSocket URL
    ws_url: String,
    /// Oracle aggregator addresses
    oracle_addresses: Vec<Address>,
    /// Oracle to asset mapping
    oracle_to_asset: std::collections::HashMap<Address, Address>,
    /// Oracle types
    oracle_types: std::collections::HashMap<Address, OracleType>,
    /// Pool contract address
    pool_address: Address,
}

impl EventListener {
    /// Create a new event listener.
    pub fn new(
        ws_url: impl Into<String>,
        pool_address: Address,
        oracle_configs: Vec<(Address, Address, OracleType)>, // (oracle, asset, type)
    ) -> Self {
        let mut oracle_addresses = Vec::new();
        let mut oracle_to_asset = std::collections::HashMap::new();
        let mut oracle_types = std::collections::HashMap::new();

        for (oracle, asset, oracle_type) in oracle_configs {
            oracle_addresses.push(oracle);
            oracle_to_asset.insert(oracle, asset);
            oracle_types.insert(oracle, oracle_type);
        }

        Self {
            ws_url: ws_url.into(),
            oracle_addresses,
            oracle_to_asset,
            oracle_types,
            pool_address,
        }
    }

    /// Subscribe to oracle update events.
    /// Returns a stream of OracleUpdate events.
    pub async fn subscribe_oracle_updates(
        &self,
    ) -> Result<Pin<Box<dyn Stream<Item = OracleUpdate> + Send>>> {
        info!(
            oracle_count = self.oracle_addresses.len(),
            ws_url = %self.ws_url,
            "Subscribing to oracle updates"
        );

        // Connect to WebSocket
        let ws = WsConnect::new(&self.ws_url);
        let provider = ProviderBuilder::new().on_ws(ws).await?;
        info!("WebSocket connected for oracle updates");

        // Create filter for AnswerUpdated events on oracle addresses
        let filter = Filter::new()
            .address(self.oracle_addresses.clone())
            .event_signature(event_signatures::ANSWER_UPDATED);

        // Subscribe to logs
        let sub = provider.subscribe_logs(&filter).await?;
        let inner_stream = sub.into_stream();

        // Clone data for the closure
        let oracle_to_asset = self.oracle_to_asset.clone();
        let oracle_types = self.oracle_types.clone();

        // Use unfold to create a stream that keeps the provider alive
        // The provider must be kept in the stream's state to prevent WebSocket from closing
        let update_stream = futures::stream::unfold(
            (provider, inner_stream, oracle_to_asset, oracle_types),
            |(_provider, mut stream, oracle_to_asset, oracle_types)| async move {
                loop {
                    match stream.next().await {
                        Some(log) => {
                            if let Some(update) =
                                parse_oracle_update(log, &oracle_to_asset, &oracle_types)
                            {
                                return Some((
                                    update,
                                    (_provider, stream, oracle_to_asset, oracle_types),
                                ));
                            }
                            // Continue loop if parse failed (skip invalid logs)
                        }
                        None => return None,
                    }
                }
            },
        );

        Ok(Box::pin(update_stream))
    }

    /// Subscribe to pool events.
    /// Returns a stream of PoolEvent.
    pub async fn subscribe_pool_events(
        &self,
    ) -> Result<Pin<Box<dyn Stream<Item = PoolEvent> + Send>>> {
        info!(
            pool = %self.pool_address,
            ws_url = %self.ws_url,
            "Subscribing to pool events"
        );

        // Connect to WebSocket
        let ws = WsConnect::new(&self.ws_url);
        let provider = ProviderBuilder::new().on_ws(ws).await?;
        info!("WebSocket connected for pool events");

        // Create filter for all pool events
        let filter = Filter::new()
            .address(self.pool_address)
            .event_signature(event_signatures::pool_signatures());

        // Subscribe to logs
        let sub = provider.subscribe_logs(&filter).await?;
        let inner_stream = sub.into_stream();

        // Use unfold to create a stream that keeps the provider alive
        // The provider must be kept in the stream's state to prevent WebSocket from closing
        let event_stream = futures::stream::unfold(
            (provider, inner_stream),
            |(_provider, mut stream)| async move {
                loop {
                    match stream.next().await {
                        Some(log) => {
                            if let Some(event) = parse_pool_event(log) {
                                return Some((event, (_provider, stream)));
                            }
                            // Continue loop if parse failed (skip invalid logs)
                        }
                        None => return None,
                    }
                }
            },
        );

        Ok(Box::pin(event_stream))
    }

    /// Subscribe to new block headers.
    /// Returns a stream of block numbers.
    pub async fn subscribe_new_heads(&self) -> Result<Pin<Box<dyn Stream<Item = u64> + Send>>> {
        info!(ws_url = %self.ws_url, "Subscribing to new block headers");

        // Connect to WebSocket
        let ws = WsConnect::new(&self.ws_url);
        let provider = ProviderBuilder::new().on_ws(ws).await?;
        info!("WebSocket connected for new block headers");

        // Subscribe to new blocks
        let sub = provider.subscribe_blocks().await?;
        let inner_stream = sub.into_stream();

        // Use unfold to create a stream that keeps the provider alive
        // The provider must be kept in the stream's state to prevent WebSocket from closing
        let block_stream = futures::stream::unfold(
            (provider, inner_stream),
            |(_provider, mut stream)| async move {
                match stream.next().await {
                    Some(block) => Some((block.number, (_provider, stream))),
                    None => None,
                }
            },
        );

        Ok(Box::pin(block_stream))
    }

    /// Get the answer updated signature for filtering.
    pub fn answer_updated_signature() -> B256 {
        OracleAggregator::answer_updated_signature()
    }

    /// Get pool event signatures for filtering.
    pub fn pool_event_signatures() -> Vec<B256> {
        PoolContract::event_signatures()
    }
}

/// Parse a log into an OracleUpdate event.
fn parse_oracle_update(
    log: Log,
    oracle_to_asset: &std::collections::HashMap<Address, Address>,
    oracle_types: &std::collections::HashMap<Address, OracleType>,
) -> Option<OracleUpdate> {
    let oracle = log.address();

    // Get asset address for this oracle
    let asset = oracle_to_asset.get(&oracle)?;
    let oracle_type = oracle_types.get(&oracle).copied().unwrap_or(OracleType::Standard);

    // Parse AnswerUpdated(int256 indexed current, uint256 indexed roundId, uint256 updatedAt)
    // Topics: [sig, current, roundId]
    // Data: [updatedAt]
    if log.topics().len() < 3 {
        warn!(oracle = %oracle, "Invalid oracle log: insufficient topics");
        return None;
    }

    // current price is in topic[1] as int256
    let price_i256 = I256::from_be_bytes(log.topics()[1].0);
    let price = if price_i256.is_negative() {
        warn!(oracle = %oracle, "Negative price from oracle");
        return None;
    } else {
        price_i256.into_raw()
    };

    // round_id is in topic[2]
    let round_id = U256::from_be_bytes(log.topics()[2].0);

    // timestamp is in data
    let timestamp = if log.data().data.len() >= 32 {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&log.data().data[0..32]);
        U256::from_be_bytes(bytes).to::<u64>()
    } else {
        0
    };

    let block_number = log.block_number.unwrap_or(0);
    let tx_hash = log.transaction_hash.unwrap_or_default();

    debug!(
        oracle = %oracle,
        asset = %asset,
        price = %price,
        round_id = %round_id,
        block = block_number,
        "Parsed oracle update"
    );

    Some(OracleUpdate {
        oracle,
        asset: *asset,
        price,
        round_id,
        timestamp,
        block_number,
        tx_hash,
        oracle_type,
    })
}

/// Parse a log into a PoolEvent.
fn parse_pool_event(log: Log) -> Option<PoolEvent> {
    let block_number = log.block_number.unwrap_or(0);
    let tx_hash = log.transaction_hash.unwrap_or_default();

    if log.topics().is_empty() {
        return None;
    }

    let sig = log.topics()[0];

    if sig == event_signatures::SUPPLY {
        parse_supply_event(log, block_number, tx_hash)
    } else if sig == event_signatures::WITHDRAW {
        parse_withdraw_event(log, block_number, tx_hash)
    } else if sig == event_signatures::BORROW {
        parse_borrow_event(log, block_number, tx_hash)
    } else if sig == event_signatures::REPAY {
        parse_repay_event(log, block_number, tx_hash)
    } else if sig == event_signatures::LIQUIDATION_CALL {
        parse_liquidation_event(log, block_number, tx_hash)
    } else {
        None
    }
}

/// Parse Supply event.
/// Supply(address indexed reserve, address user, address indexed onBehalfOf, uint256 amount, uint16 indexed referralCode)
fn parse_supply_event(log: Log, block_number: u64, tx_hash: B256) -> Option<PoolEvent> {
    if log.topics().len() < 4 {
        return None;
    }

    let reserve = Address::from_slice(&log.topics()[1][12..]);
    let on_behalf_of = Address::from_slice(&log.topics()[2][12..]);
    // referralCode in topics()[3], we don't need it

    // Data: user (address), amount (uint256)
    if log.data().data.len() < 64 {
        return None;
    }

    let user = Address::from_slice(&log.data().data[12..32]);
    let amount = U256::from_be_slice(&log.data().data[32..64]);

    Some(PoolEvent::Supply {
        reserve,
        user,
        on_behalf_of,
        amount,
        block_number,
        tx_hash,
    })
}

/// Parse Withdraw event.
/// Withdraw(address indexed reserve, address indexed user, address indexed to, uint256 amount)
fn parse_withdraw_event(log: Log, block_number: u64, tx_hash: B256) -> Option<PoolEvent> {
    if log.topics().len() < 4 {
        return None;
    }

    let reserve = Address::from_slice(&log.topics()[1][12..]);
    let user = Address::from_slice(&log.topics()[2][12..]);
    let to = Address::from_slice(&log.topics()[3][12..]);

    // Data: amount (uint256)
    if log.data().data.len() < 32 {
        return None;
    }

    let amount = U256::from_be_slice(&log.data().data[0..32]);

    Some(PoolEvent::Withdraw {
        reserve,
        user,
        to,
        amount,
        block_number,
        tx_hash,
    })
}

/// Parse Borrow event.
/// Borrow(address indexed reserve, address user, address indexed onBehalfOf, uint256 amount, uint8 interestRateMode, uint256 borrowRate, uint16 indexed referralCode)
fn parse_borrow_event(log: Log, block_number: u64, tx_hash: B256) -> Option<PoolEvent> {
    if log.topics().len() < 4 {
        return None;
    }

    let reserve = Address::from_slice(&log.topics()[1][12..]);
    let on_behalf_of = Address::from_slice(&log.topics()[2][12..]);
    // referralCode in topics()[3], we don't need it

    // Data: user (address), amount (uint256), interestRateMode (uint8), borrowRate (uint256)
    if log.data().data.len() < 64 {
        return None;
    }

    let user = Address::from_slice(&log.data().data[12..32]);
    let amount = U256::from_be_slice(&log.data().data[32..64]);

    Some(PoolEvent::Borrow {
        reserve,
        user,
        on_behalf_of,
        amount,
        block_number,
        tx_hash,
    })
}

/// Parse Repay event.
/// Repay(address indexed reserve, address indexed user, address indexed repayer, uint256 amount, bool useATokens)
fn parse_repay_event(log: Log, block_number: u64, tx_hash: B256) -> Option<PoolEvent> {
    if log.topics().len() < 4 {
        return None;
    }

    let reserve = Address::from_slice(&log.topics()[1][12..]);
    let user = Address::from_slice(&log.topics()[2][12..]);
    let repayer = Address::from_slice(&log.topics()[3][12..]);

    // Data: amount (uint256), useATokens (bool)
    if log.data().data.len() < 32 {
        return None;
    }

    let amount = U256::from_be_slice(&log.data().data[0..32]);

    Some(PoolEvent::Repay {
        reserve,
        user,
        repayer,
        amount,
        block_number,
        tx_hash,
    })
}

/// Parse LiquidationCall event.
/// LiquidationCall(address indexed collateralAsset, address indexed debtAsset, address indexed user, uint256 debtToCover, uint256 liquidatedCollateralAmount, address liquidator, bool receiveAToken)
fn parse_liquidation_event(log: Log, block_number: u64, tx_hash: B256) -> Option<PoolEvent> {
    if log.topics().len() < 4 {
        return None;
    }

    let collateral_asset = Address::from_slice(&log.topics()[1][12..]);
    let debt_asset = Address::from_slice(&log.topics()[2][12..]);
    let user = Address::from_slice(&log.topics()[3][12..]);

    // Data: debtToCover (uint256), liquidatedCollateralAmount (uint256), liquidator (address), receiveAToken (bool)
    if log.data().data.len() < 96 {
        return None;
    }

    let debt_to_cover = U256::from_be_slice(&log.data().data[0..32]);
    let liquidated_collateral = U256::from_be_slice(&log.data().data[32..64]);
    let liquidator = Address::from_slice(&log.data().data[76..96]);

    Some(PoolEvent::LiquidationCall {
        collateral_asset,
        debt_asset,
        user,
        debt_to_cover,
        liquidated_collateral,
        liquidator,
        block_number,
        tx_hash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_event_user() {
        let event = PoolEvent::Supply {
            reserve: Address::ZERO,
            user: Address::repeat_byte(1),
            on_behalf_of: Address::repeat_byte(2),
            amount: U256::from(1000u64),
            block_number: 100,
            tx_hash: B256::ZERO,
        };

        // on_behalf_of is the affected user for Supply
        assert_eq!(event.user(), Address::repeat_byte(2));
        assert_eq!(event.event_type(), "Supply");
    }
}
