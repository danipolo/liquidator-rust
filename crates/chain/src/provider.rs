//! Provider management for HTTP and WebSocket connections.
//! Uses Alloy providers for type-safe RPC interactions.

use alloy::primitives::{Address, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::sol;
use anyhow::Result;
use tracing::{debug, info, warn};

// Define BalancesReader contract interface with #[sol(rpc)] for typed calls
sol! {
    /// Balance entry from BalancesReader
    #[derive(Debug)]
    struct BalanceEntry {
        address underlying;
        uint256 amount;
        uint256 price;
        uint256 decimals;
    }

    /// BalancesReader contract interface
    #[sol(rpc)]
    interface IBalancesReader {
        function getAllSuppliedBalancesWithPrices(
            address pool,
            address user
        ) external view returns (BalanceEntry[] memory);

        function getAllBorrowedBalancesWithPrices(
            address pool,
            address user
        ) external view returns (BalanceEntry[] memory);
    }
}

/// Balance data from BalancesReader contract.
#[derive(Debug, Clone)]
pub struct BalanceData {
    pub underlying: Address,
    pub amount: U256,
    pub price: U256,
    pub decimals: u8,
    /// Liquidation threshold (basis points, e.g., 8000 = 80%)
    /// This is populated from asset registry, not from contract
    pub liquidation_threshold: u16,
}

impl From<BalanceEntry> for BalanceData {
    fn from(entry: BalanceEntry) -> Self {
        Self {
            underlying: entry.underlying,
            amount: entry.amount,
            price: entry.price,
            decimals: entry.decimals.to::<u8>(),
            liquidation_threshold: 8000, // Default 80%, should be updated from asset config
        }
    }
}

/// Provider manager for multiple RPC connections.
/// Uses Alloy typed providers instead of manual JSON-RPC.
#[derive(Clone)]
pub struct ProviderManager {
    /// HTTP URL (general purpose)
    http_url: String,
    /// Read URL (for contract calls like BalancesReader)
    read_url: String,
    /// Archive URL
    archive_url: String,
    /// Send URL
    send_url: String,
    /// WebSocket URL for subscriptions
    ws_url: String,
    /// Pool address
    pool_address: Address,
    /// BalancesReader address
    balances_reader_address: Address,
}

impl ProviderManager {
    /// Create a new provider manager with Alloy providers.
    pub async fn new(
        http_url: &str,
        archive_url: &str,
        send_url: &str,
        ws_url: &str,
        pool_address: Address,
        balances_reader_address: Address,
    ) -> Result<Self> {
        // Use HyperLend RPC for contract reads (more reliable than Alchemy for this)
        let read_url = "https://rpc.hyperlend.finance";

        info!(
            http = http_url,
            read = read_url,
            archive = archive_url,
            send = send_url,
            ws = ws_url,
            "Initializing provider manager with Alloy providers"
        );

        // Test connection
        let provider = ProviderBuilder::new().on_http(read_url.parse()?);
        let block = provider.get_block_number().await?;
        info!(block = block, "Provider connection verified");

        Ok(Self {
            http_url: http_url.to_string(),
            read_url: read_url.to_string(),
            archive_url: archive_url.to_string(),
            send_url: send_url.to_string(),
            ws_url: ws_url.to_string(),
            pool_address,
            balances_reader_address,
        })
    }

    /// Get the HTTP URL.
    pub fn http_url(&self) -> &str {
        &self.http_url
    }

    /// Get the archive URL.
    pub fn archive_url(&self) -> &str {
        &self.archive_url
    }

    /// Get the send URL.
    pub fn send_url(&self) -> &str {
        &self.send_url
    }

    /// Get the WebSocket URL.
    pub fn ws_url(&self) -> &str {
        &self.ws_url
    }

    /// Get the pool address.
    pub fn pool_address(&self) -> Address {
        self.pool_address
    }

    /// Get current block number using Alloy provider.
    pub async fn block_number(&self) -> Result<u64> {
        let provider = ProviderBuilder::new().on_http(self.read_url.parse()?);
        let block = provider.get_block_number().await?;
        Ok(block)
    }

    /// Get chain ID using Alloy provider.
    pub async fn chain_id(&self) -> Result<u64> {
        let provider = ProviderBuilder::new().on_http(self.read_url.parse()?);
        let chain_id = provider.get_chain_id().await?;
        Ok(chain_id)
    }

    /// Get position data for a user using typed Alloy contract calls.
    /// Returns (supplied_balances, borrowed_balances).
    /// OPTIMIZATION: Fetches supply and borrow balances in parallel (~50% faster).
    pub async fn get_position_data(
        &self,
        user: Address,
    ) -> Result<(Vec<BalanceData>, Vec<BalanceData>)> {
        debug!(user = %user, "Fetching position data via Alloy");

        // Create provider and contract instance
        let provider = ProviderBuilder::new().on_http(self.read_url.parse()?);
        let contract = IBalancesReader::new(self.balances_reader_address, &provider);

        // Create typed contract calls
        let supply_call = contract.getAllSuppliedBalancesWithPrices(self.pool_address, user);
        let borrow_call = contract.getAllBorrowedBalancesWithPrices(self.pool_address, user);

        // Execute both calls in parallel using Alloy's typed interface
        let (supply_result, borrow_result) = tokio::join!(
            supply_call.call(),
            borrow_call.call()
        );

        // Parse results with proper error handling
        let supply_balances: Vec<BalanceData> = match supply_result {
            Ok(entries) => entries._0.into_iter().map(BalanceData::from).collect(),
            Err(e) => {
                warn!(user = %user, error = %e, "Failed to fetch supply balances");
                Vec::new()
            }
        };

        let borrow_balances: Vec<BalanceData> = match borrow_result {
            Ok(entries) => entries._0.into_iter().map(BalanceData::from).collect(),
            Err(e) => {
                warn!(user = %user, error = %e, "Failed to fetch borrow balances");
                Vec::new()
            }
        };

        debug!(
            user = %user,
            supply_count = supply_balances.len(),
            borrow_count = borrow_balances.len(),
            "Position data fetched via Alloy"
        );

        Ok((supply_balances, borrow_balances))
    }

    /// Get position data for multiple users in parallel.
    /// OPTIMIZATION: Fetches all users concurrently with bounded parallelism.
    pub async fn get_positions_batch(
        &self,
        users: &[Address],
        max_concurrent: usize,
    ) -> Vec<(Address, Result<(Vec<BalanceData>, Vec<BalanceData>)>)> {
        use futures::stream::{self, StreamExt};

        stream::iter(users.iter().cloned())
            .map(|user| async move {
                let result = self.get_position_data(user).await;
                (user, result)
            })
            .buffer_unordered(max_concurrent)
            .collect()
            .await
    }

    /// Check if provider is healthy.
    pub async fn health_check(&self) -> Result<bool> {
        let block = self.block_number().await?;
        debug!(block = block, "Provider health check passed");
        Ok(block > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires network
    async fn test_provider_creation() {
        let provider = ProviderManager::new(
            "https://rpc.hyperlend.finance",
            "https://rpc.hyperlend.finance/archive",
            "https://rpc.hyperliquid.xyz/evm",
            "wss://hyperliquid.g.alchemy.com/v2/test",
            "0x00A89d7a5A02160f20150EbEA7a2b5E4879A1A8b"
                .parse()
                .unwrap(),
            "0xE17ea42a8d61e50a26bec1829399071d2129845b"
                .parse()
                .unwrap(),
        )
        .await;

        assert!(provider.is_ok());
    }
}
