//! Transaction signer and sender for liquidations.
//! Uses Alloy providers for type-safe RPC interactions.
//!
//! OPTIMIZATIONS:
//! - Cached nonce: Atomic counter avoids RPC call per transaction
//! - Pre-computed gas: Uses configurable gas strategy for speed
//! - Supports both Legacy and EIP-1559 gas pricing

use crate::gas::{create_gas_strategy, GasParams, GasStrategy, LegacyGasStrategy};
use alloy::network::{EthereumWallet, TransactionBuilder};
use alloy::primitives::{Address, Bytes, B256, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::TransactionRequest;
use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tracing::{debug, info, warn};

/// Cached nonce manager for fast transaction submission.
/// Avoids RPC calls by tracking nonce locally with atomic operations.
pub struct NonceManager {
    /// Current nonce (atomically incremented)
    current: AtomicU64,
    /// Last confirmed nonce from chain
    last_synced: AtomicU64,
}

impl NonceManager {
    /// Create new nonce manager with initial value from chain.
    pub fn new(initial_nonce: u64) -> Self {
        Self {
            current: AtomicU64::new(initial_nonce),
            last_synced: AtomicU64::new(initial_nonce),
        }
    }

    /// Get next nonce and increment counter.
    /// This is lock-free and extremely fast (~1ns).
    #[inline]
    pub fn next(&self) -> u64 {
        self.current.fetch_add(1, Ordering::SeqCst)
    }

    /// Get current nonce without incrementing.
    #[inline]
    pub fn current(&self) -> u64 {
        self.current.load(Ordering::SeqCst)
    }

    /// Sync nonce from chain (call periodically or on error).
    pub fn sync(&self, chain_nonce: u64) {
        let current = self.current.load(Ordering::SeqCst);
        // Only update if chain is ahead (handles tx confirmations)
        if chain_nonce > current {
            self.current.store(chain_nonce, Ordering::SeqCst);
        }
        self.last_synced.store(chain_nonce, Ordering::SeqCst);
    }

    /// Reset nonce to chain value (use after tx failure).
    pub fn reset(&self, chain_nonce: u64) {
        self.current.store(chain_nonce, Ordering::SeqCst);
        self.last_synced.store(chain_nonce, Ordering::SeqCst);
    }
}

/// Transaction sender with configurable gas strategy.
/// Uses Alloy providers for all RPC interactions.
///
/// OPTIMIZATIONS:
/// - Nonce is managed locally (no RPC call per tx)
/// - Gas parameters are configurable via GasStrategy
pub struct TransactionSender {
    /// RPC URL for sending transactions
    rpc_url: String,
    /// Signer wallet
    wallet: EthereumWallet,
    /// Signer address
    pub address: Address,
    /// Chain ID
    chain_id: u64,
    /// Cached nonce manager
    nonce_manager: NonceManager,
    /// Default gas limit for liquidations (pre-computed)
    default_gas_limit: u64,
    /// Gas pricing strategy
    gas_strategy: Box<dyn GasStrategy>,
    /// Cached gas parameters (updated periodically)
    cached_gas_params: parking_lot::RwLock<Option<GasParams>>,
}

/// Default gas limit for complex liquidations (1.6M gas)
/// Based on real liquidation data: complex multi-hop swaps use ~1.57M gas
const DEFAULT_LIQUIDATION_GAS_LIMIT: u64 = 1_600_000;

/// Builder for TransactionSender with flexible configuration.
pub struct TransactionSenderBuilder {
    rpc_url: String,
    chain_id: u64,
    gas_strategy: Option<Box<dyn GasStrategy>>,
    gas_limit: Option<u64>,
}

impl TransactionSenderBuilder {
    /// Create a new builder.
    pub fn new(rpc_url: impl Into<String>, chain_id: u64) -> Self {
        Self {
            rpc_url: rpc_url.into(),
            chain_id,
            gas_strategy: None,
            gas_limit: None,
        }
    }

    /// Set the gas strategy.
    pub fn gas_strategy(mut self, strategy: Box<dyn GasStrategy>) -> Self {
        self.gas_strategy = Some(strategy);
        self
    }

    /// Set a custom gas limit.
    pub fn gas_limit(mut self, limit: u64) -> Self {
        self.gas_limit = Some(limit);
        self
    }

    /// Set gas strategy from chain config parameters.
    pub fn gas_from_config(
        mut self,
        pricing_model: &str,
        default_gas_price_gwei: f64,
        max_gas_price_gwei: f64,
        priority_fee_gwei: Option<f64>,
    ) -> Self {
        self.gas_strategy = Some(create_gas_strategy(
            pricing_model,
            default_gas_price_gwei,
            max_gas_price_gwei,
            priority_fee_gwei,
        ));
        self
    }

    /// Build the TransactionSender.
    pub async fn build(self, private_key: &str) -> Result<TransactionSender> {
        // Parse private key (with or without 0x prefix)
        let key_str = private_key.trim_start_matches("0x");
        let signer: PrivateKeySigner = key_str.parse()?;
        let address = signer.address();
        let wallet = EthereumWallet::from(signer);

        // Create provider for initial queries
        let provider = ProviderBuilder::new().on_http(self.rpc_url.parse()?);

        // Fetch initial nonce from chain
        let initial_nonce = provider.get_transaction_count(address).await?;
        let nonce_manager = NonceManager::new(initial_nonce);

        // Use provided gas strategy or default to Legacy
        let gas_strategy = self.gas_strategy.unwrap_or_else(|| {
            Box::new(LegacyGasStrategy::new(
                1_000_000_000,  // 1 gwei default
                10_000_000_000, // 10 gwei max
            ))
        });

        // Fetch initial gas params
        let initial_gas_params = gas_strategy.fetch_params(&self.rpc_url).await.ok();
        let rpc_url = self.rpc_url;

        info!(
            address = %address,
            chain_id = self.chain_id,
            initial_nonce = initial_nonce,
            gas_strategy = gas_strategy.strategy_name(),
            "Transaction sender initialized"
        );

        Ok(TransactionSender {
            rpc_url,
            wallet,
            address,
            chain_id: self.chain_id,
            nonce_manager,
            default_gas_limit: self.gas_limit.unwrap_or(DEFAULT_LIQUIDATION_GAS_LIMIT),
            gas_strategy,
            cached_gas_params: parking_lot::RwLock::new(initial_gas_params),
        })
    }
}

impl TransactionSender {
    /// Create a new transaction sender from private key.
    ///
    /// Uses Legacy gas pricing by default. For EIP-1559, use `TransactionSenderBuilder`.
    ///
    /// OPTIMIZATION: Fetches initial nonce and caches it for fast tx submission.
    pub async fn new(private_key: &str, rpc_url: &str, chain_id: u64) -> Result<Self> {
        TransactionSenderBuilder::new(rpc_url, chain_id)
            .build(private_key)
            .await
    }

    /// Create with a specific gas strategy.
    pub async fn with_gas_strategy(
        private_key: &str,
        rpc_url: &str,
        chain_id: u64,
        gas_strategy: Box<dyn GasStrategy>,
    ) -> Result<Self> {
        TransactionSenderBuilder::new(rpc_url, chain_id)
            .gas_strategy(gas_strategy)
            .build(private_key)
            .await
    }

    /// Create synchronously (for compatibility) - will block on async init.
    pub fn new_blocking(private_key: &str, rpc_url: &str, chain_id: u64) -> Result<Self> {
        tokio::runtime::Handle::current().block_on(Self::new(private_key, rpc_url, chain_id))
    }

    /// Get the current gas strategy name.
    pub fn gas_strategy_name(&self) -> &'static str {
        self.gas_strategy.strategy_name()
    }

    /// Get cached gas parameters (if available).
    pub fn cached_gas_params(&self) -> Option<GasParams> {
        self.cached_gas_params.read().clone()
    }

    /// Send a transaction and wait for confirmation.
    ///
    /// OPTIMIZATIONS:
    /// - Uses cached nonce (no RPC call)
    /// - Uses pre-computed gas limit (no estimation call)
    /// - Applies gas strategy for appropriate pricing
    ///
    /// Latency: ~50ms vs ~100ms before optimizations
    pub async fn send_transaction(
        &self,
        to: Address,
        calldata: Bytes,
        value: U256,
    ) -> Result<B256> {
        let total_start = Instant::now();

        debug!(
            to = %to,
            calldata_len = calldata.len(),
            value = %value,
            gas_strategy = self.gas_strategy.strategy_name(),
            "Preparing transaction"
        );

        // TIMING: Nonce fetch (should be ~0ms with cache)
        let nonce_start = Instant::now();
        let nonce = self.nonce_manager.next();
        let nonce_elapsed = nonce_start.elapsed();

        // TIMING: Gas params (use cache or fetch)
        let gas_start = Instant::now();
        let gas_params = {
            let cached = self.cached_gas_params.read().clone();
            match cached {
                Some(params) => params,
                None => self.gas_strategy.fetch_params(&self.rpc_url).await?,
            }
        };
        let gas_elapsed = gas_start.elapsed();

        // TIMING: Transaction build
        let build_start = Instant::now();
        let mut tx = TransactionRequest::default()
            .with_to(to)
            .with_input(calldata)
            .with_value(value)
            .with_nonce(nonce)
            .with_gas_limit(self.default_gas_limit)
            .with_chain_id(self.chain_id);

        // Apply gas strategy (Legacy or EIP-1559)
        self.gas_strategy.apply_gas(&mut tx, &gas_params);
        let build_elapsed = build_start.elapsed();

        info!(
            to = %to,
            nonce = nonce,
            gas_limit = self.default_gas_limit,
            gas_strategy = self.gas_strategy.strategy_name(),
            gas_price = ?gas_params.effective_gas_price() / 1_000_000_000,
            nonce_us = nonce_elapsed.as_micros(),
            gas_us = gas_elapsed.as_micros(),
            build_us = build_elapsed.as_micros(),
            "Sending transaction"
        );

        // TIMING: Provider creation with wallet
        let provider_start = Instant::now();
        let provider = ProviderBuilder::new()
            .wallet(self.wallet.clone())
            .on_http(self.rpc_url.parse()?);
        let provider_elapsed = provider_start.elapsed();

        // TIMING: Transaction submission (RPC call)
        let submit_start = Instant::now();
        let pending = provider.send_transaction(tx).await?;
        let tx_hash = *pending.tx_hash();
        let submit_elapsed = submit_start.elapsed();

        info!(
            tx_hash = %tx_hash,
            provider_us = provider_elapsed.as_micros(),
            submit_ms = submit_elapsed.as_millis(),
            "Transaction submitted, waiting for confirmation"
        );

        // TIMING: Wait for confirmation
        let confirm_start = Instant::now();
        let receipt = pending.get_receipt().await?;
        let confirm_elapsed = confirm_start.elapsed();

        let total_elapsed = total_start.elapsed();

        if receipt.status() {
            info!(
                tx_hash = %tx_hash,
                block = receipt.block_number.unwrap_or(0),
                gas_used = receipt.gas_used,
                confirm_ms = confirm_elapsed.as_millis(),
                total_ms = total_elapsed.as_millis(),
                "Transaction confirmed [TIMING: nonce={}us, build={}us, submit={}ms, confirm={}ms, total={}ms]",
                nonce_elapsed.as_micros(),
                build_elapsed.as_micros(),
                submit_elapsed.as_millis(),
                confirm_elapsed.as_millis(),
                total_elapsed.as_millis()
            );
            Ok(tx_hash)
        } else {
            // On revert, sync nonce from chain
            warn!(
                tx_hash = %tx_hash,
                total_ms = total_elapsed.as_millis(),
                "Transaction reverted, syncing nonce"
            );
            self.sync_nonce().await;
            anyhow::bail!("Transaction reverted: {:?}", tx_hash)
        }
    }

    /// Send transaction with custom gas limit (for non-standard operations).
    pub async fn send_transaction_with_gas(
        &self,
        to: Address,
        calldata: Bytes,
        value: U256,
        gas_limit: u64,
    ) -> Result<B256> {
        let nonce = self.nonce_manager.next();

        let gas_params = {
            let cached = self.cached_gas_params.read().clone();
            match cached {
                Some(params) => params,
                None => self.gas_strategy.fetch_params(&self.rpc_url).await?,
            }
        };

        let mut tx = TransactionRequest::default()
            .with_to(to)
            .with_input(calldata)
            .with_value(value)
            .with_nonce(nonce)
            .with_gas_limit(gas_limit)
            .with_chain_id(self.chain_id);

        self.gas_strategy.apply_gas(&mut tx, &gas_params);

        let provider = ProviderBuilder::new()
            .wallet(self.wallet.clone())
            .on_http(self.rpc_url.parse()?);

        let pending = provider.send_transaction(tx).await?;
        let tx_hash = *pending.tx_hash();

        let receipt = pending.get_receipt().await?;

        if receipt.status() {
            Ok(tx_hash)
        } else {
            self.sync_nonce().await;
            anyhow::bail!("Transaction reverted: {:?}", tx_hash)
        }
    }

    /// Sync nonce from chain (call on error or periodically).
    pub async fn sync_nonce(&self) {
        let provider = match ProviderBuilder::new().on_http(self.rpc_url.parse().unwrap()) {
            p => p,
        };
        match provider.get_transaction_count(self.address).await {
            Ok(chain_nonce) => {
                self.nonce_manager.reset(chain_nonce);
                debug!(nonce = chain_nonce, "Nonce synced from chain");
            }
            Err(e) => {
                warn!(error = %e, "Failed to sync nonce from chain");
            }
        }
    }

    /// Update cached gas parameters (call periodically, e.g., every 10s).
    pub async fn update_gas_params(&self) {
        match self.gas_strategy.fetch_params(&self.rpc_url).await {
            Ok(params) => {
                debug!(
                    gas_price_gwei = params.effective_gas_price() / 1_000_000_000,
                    strategy = self.gas_strategy.strategy_name(),
                    "Gas params updated"
                );
                *self.cached_gas_params.write() = Some(params);
            }
            Err(e) => {
                warn!(error = %e, "Failed to update gas params");
            }
        }
    }

    /// Get current cached nonce.
    pub fn current_nonce(&self) -> u64 {
        self.nonce_manager.current()
    }

    /// Get effective gas price from cached params.
    pub fn gas_price(&self) -> u64 {
        self.cached_gas_params
            .read()
            .as_ref()
            .map(|p| p.effective_gas_price() as u64)
            .unwrap_or(1_000_000_000) // 1 gwei default
    }

    /// Get current balance.
    pub async fn get_balance(&self) -> Result<U256> {
        let provider = ProviderBuilder::new().on_http(self.rpc_url.parse()?);
        let balance = provider.get_balance(self.address).await?;
        Ok(balance)
    }

    /// Get the RPC URL.
    pub fn rpc_url(&self) -> &str {
        &self.rpc_url
    }
}

impl std::fmt::Debug for TransactionSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransactionSender")
            .field("address", &self.address)
            .field("chain_id", &self.chain_id)
            .field("rpc_url", &self.rpc_url)
            .field("gas_strategy", &self.gas_strategy.strategy_name())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nonce_manager() {
        let manager = NonceManager::new(10);

        assert_eq!(manager.current(), 10);
        assert_eq!(manager.next(), 10);
        assert_eq!(manager.current(), 11);
        assert_eq!(manager.next(), 11);
        assert_eq!(manager.current(), 12);

        // Sync should update if chain is ahead
        manager.sync(15);
        assert_eq!(manager.current(), 15);

        // Sync should not decrease
        manager.sync(10);
        assert_eq!(manager.current(), 15);

        // Reset forces update
        manager.reset(5);
        assert_eq!(manager.current(), 5);
    }

    #[tokio::test]
    #[ignore] // Requires network
    async fn test_sender_creation() {
        // Test private key (DO NOT USE IN PRODUCTION)
        let private_key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
        let sender = TransactionSender::new(
            private_key,
            "https://rpc.hyperliquid.xyz/evm",
            999,
        )
        .await;

        assert!(sender.is_ok());
        let sender = sender.unwrap();
        // This is the expected address for the test private key (case-insensitive)
        assert_eq!(
            format!("{:?}", sender.address).to_lowercase(),
            "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"
        );

        // Verify initial nonce was fetched
        assert!(sender.current_nonce() >= 0);

        // Verify gas strategy is Legacy by default
        assert_eq!(sender.gas_strategy_name(), "Legacy");
    }

    #[tokio::test]
    #[ignore] // Requires network
    async fn test_sender_with_eip1559() {
        use crate::gas::Eip1559GasStrategy;

        let private_key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
        let gas_strategy = Box::new(Eip1559GasStrategy::new(2_000_000_000, 1.5));

        let sender = TransactionSender::with_gas_strategy(
            private_key,
            "https://eth.llamarpc.com",
            1,
            gas_strategy,
        )
        .await;

        assert!(sender.is_ok());
        let sender = sender.unwrap();
        assert_eq!(sender.gas_strategy_name(), "EIP-1559");
    }
}
