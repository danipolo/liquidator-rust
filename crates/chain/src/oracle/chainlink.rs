//! Chainlink oracle implementation.

use super::{Oracle, OracleType, PriceData, RoundData};
use alloy::primitives::{Address, U256};
use alloy::providers::Provider;
use alloy::sol;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

// Chainlink AggregatorV3 interface
sol! {
    #[sol(rpc)]
    interface IAggregatorV3 {
        function latestRoundData() external view returns (
            uint80 roundId,
            int256 answer,
            uint256 startedAt,
            uint256 updatedAt,
            uint80 answeredInRound
        );

        function decimals() external view returns (uint8);

        function description() external view returns (string memory);

        function getRoundData(uint80 _roundId) external view returns (
            uint80 roundId,
            int256 answer,
            uint256 startedAt,
            uint256 updatedAt,
            uint80 answeredInRound
        );
    }
}

/// Chainlink oracle implementation.
#[derive(Clone)]
pub struct ChainlinkOracle<P> {
    /// Aggregator contract address
    aggregator: Address,
    /// Asset address this oracle prices
    asset: Address,
    /// Price decimals
    decimals: u8,
    /// Heartbeat interval
    heartbeat: Option<Duration>,
    /// Provider for RPC calls
    provider: Arc<P>,
    /// Description (cached)
    description: Option<String>,
}

impl<P> std::fmt::Debug for ChainlinkOracle<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChainlinkOracle")
            .field("aggregator", &self.aggregator)
            .field("asset", &self.asset)
            .field("decimals", &self.decimals)
            .field("heartbeat", &self.heartbeat)
            .finish()
    }
}

impl<P: Provider + Clone + 'static> ChainlinkOracle<P> {
    /// Create a new Chainlink oracle.
    pub fn new(aggregator: Address, asset: Address, decimals: u8, provider: Arc<P>) -> Self {
        Self {
            aggregator,
            asset,
            decimals,
            heartbeat: None,
            provider,
            description: None,
        }
    }

    /// Set the heartbeat interval.
    pub fn with_heartbeat(mut self, heartbeat: Duration) -> Self {
        self.heartbeat = Some(heartbeat);
        self
    }

    /// Set the description.
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    /// Fetch decimals from the contract.
    pub async fn fetch_decimals(&self) -> Result<u8> {
        let contract = IAggregatorV3::new(self.aggregator, &*self.provider);
        let decimals = contract.decimals().call().await?;
        Ok(decimals._0)
    }

    /// Fetch description from the contract.
    pub async fn fetch_description(&self) -> Result<String> {
        let contract = IAggregatorV3::new(self.aggregator, &*self.provider);
        let desc = contract.description().call().await?;
        Ok(desc._0)
    }

    /// Get historical round data.
    pub async fn get_round(&self, round_id: u128) -> Result<RoundData> {
        use alloy::primitives::Uint;
        let contract = IAggregatorV3::new(self.aggregator, &*self.provider);
        let round_id_u80: Uint<80, 2> = Uint::from(round_id as u64);
        let round = contract.getRoundData(round_id_u80).call().await?;

        // Convert int256 answer to U256 (price should always be positive)
        let answer = if round.answer.is_negative() {
            U256::ZERO
        } else {
            // I256 is two's complement, so positive values have same bit representation
            U256::from_limbs(round.answer.into_raw().into_limbs())
        };

        Ok(RoundData {
            round_id: round.roundId.to::<u128>(),
            answer,
            started_at: round.startedAt.to::<u64>(),
            updated_at: round.updatedAt.to::<u64>(),
            answered_in_round: round.answeredInRound.to::<u128>(),
        })
    }
}

#[async_trait]
impl<P: Provider + Clone + Send + Sync + 'static> Oracle for ChainlinkOracle<P> {
    fn oracle_type(&self) -> OracleType {
        OracleType::Chainlink
    }

    fn address(&self) -> Address {
        self.aggregator
    }

    fn asset(&self) -> Address {
        self.asset
    }

    fn decimals(&self) -> u8 {
        self.decimals
    }

    fn heartbeat(&self) -> Option<Duration> {
        self.heartbeat
    }

    async fn get_price(&self) -> Result<PriceData> {
        let round = self.get_latest_round().await?;

        // Get current block number
        let block_number = self.provider.get_block_number().await.unwrap_or(0);

        Ok(PriceData::new(
            round.answer,
            self.decimals,
            round.updated_at,
            block_number,
            OracleType::Chainlink,
        ))
    }

    async fn get_latest_round(&self) -> Result<RoundData> {
        let contract = IAggregatorV3::new(self.aggregator, &*self.provider);
        let round = contract.latestRoundData().call().await?;

        // Convert int256 answer to U256 (price should always be positive)
        let answer = if round.answer.is_negative() {
            U256::ZERO
        } else {
            // I256 is two's complement, so positive values have same bit representation
            U256::from_limbs(round.answer.into_raw().into_limbs())
        };

        Ok(RoundData {
            round_id: round.roundId.to::<u128>(),
            answer,
            started_at: round.startedAt.to::<u64>(),
            updated_at: round.updatedAt.to::<u64>(),
            answered_in_round: round.answeredInRound.to::<u128>(),
        })
    }
}

/// Builder for creating Chainlink oracles.
pub struct ChainlinkOracleBuilder<P> {
    aggregator: Address,
    asset: Address,
    decimals: u8,
    heartbeat: Option<Duration>,
    description: Option<String>,
    provider: Arc<P>,
}

impl<P: Provider + Clone + 'static> ChainlinkOracleBuilder<P> {
    /// Create a new builder.
    pub fn new(aggregator: Address, asset: Address, provider: Arc<P>) -> Self {
        Self {
            aggregator,
            asset,
            decimals: 8, // Default Chainlink decimals
            heartbeat: None,
            description: None,
            provider,
        }
    }

    /// Set decimals.
    pub fn decimals(mut self, decimals: u8) -> Self {
        self.decimals = decimals;
        self
    }

    /// Set heartbeat.
    pub fn heartbeat(mut self, heartbeat: Duration) -> Self {
        self.heartbeat = Some(heartbeat);
        self
    }

    /// Set description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Build the oracle.
    pub fn build(self) -> ChainlinkOracle<P> {
        let mut oracle = ChainlinkOracle::new(
            self.aggregator,
            self.asset,
            self.decimals,
            self.provider,
        );

        if let Some(heartbeat) = self.heartbeat {
            oracle = oracle.with_heartbeat(heartbeat);
        }

        if let Some(description) = self.description {
            oracle = oracle.with_description(description);
        }

        oracle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_data_validation() {
        let valid = RoundData {
            round_id: 100,
            answer: U256::from(200_000_000_000u64),
            started_at: 1700000000,
            updated_at: 1700000100,
            answered_in_round: 100,
        };

        assert!(valid.is_valid());
        assert!((valid.price_f64(8) - 2000.0).abs() < 0.01);
    }
}
