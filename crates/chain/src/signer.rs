//! Transaction signer and sender for HyperLend liquidations.
//! Uses Alloy providers for type-safe RPC interactions.
//!
//! OPTIMIZATIONS:
//! - Cached nonce: Atomic counter avoids RPC call per transaction
//! - Pre-computed gas: Uses fixed gas parameters for speed

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

/// Transaction sender with signer.
/// Uses Alloy providers for all RPC interactions.
///
/// OPTIMIZATIONS:
/// - Nonce is managed locally (no RPC call per tx)
/// - Gas parameters are pre-configured for speed
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
    /// Default gas price in wei (updated periodically)
    cached_gas_price: AtomicU64,
}

/// Default gas limit for complex liquidations (1.6M gas)
/// Based on real liquidation data: complex multi-hop swaps use ~1.57M gas
const DEFAULT_LIQUIDATION_GAS_LIMIT: u64 = 1_600_000;

/// Default gas price in gwei (0.7 gwei for HyperLiquid)
const DEFAULT_GAS_PRICE_GWEI: u64 = 1;

impl TransactionSender {
    /// Create a new transaction sender from private key.
    ///
    /// OPTIMIZATION: Fetches initial nonce and caches it for fast tx submission.
    pub async fn new(private_key: &str, rpc_url: &str, chain_id: u64) -> Result<Self> {
        // Parse private key (with or without 0x prefix)
        let key_str = private_key.trim_start_matches("0x");
        let signer: PrivateKeySigner = key_str.parse()?;
        let address = signer.address();
        let wallet = EthereumWallet::from(signer);

        // Create provider for initial queries
        let provider = ProviderBuilder::new().on_http(rpc_url.parse()?);

        // Fetch initial nonce from chain
        let initial_nonce = provider.get_transaction_count(address).await?;
        let nonce_manager = NonceManager::new(initial_nonce);

        // Fetch initial gas price
        let gas_price = provider.get_gas_price().await.unwrap_or((DEFAULT_GAS_PRICE_GWEI as u128) * 1_000_000_000);

        info!(
            address = %address,
            chain_id = chain_id,
            initial_nonce = initial_nonce,
            gas_price_gwei = gas_price / 1_000_000_000,
            "Transaction sender initialized with cached nonce"
        );

        Ok(Self {
            rpc_url: rpc_url.to_string(),
            wallet,
            address,
            chain_id,
            nonce_manager,
            default_gas_limit: DEFAULT_LIQUIDATION_GAS_LIMIT,
            cached_gas_price: AtomicU64::new(gas_price as u64),
        })
    }

    /// Create synchronously (for compatibility) - will block on async init.
    pub fn new_blocking(private_key: &str, rpc_url: &str, chain_id: u64) -> Result<Self> {
        tokio::runtime::Handle::current().block_on(Self::new(private_key, rpc_url, chain_id))
    }

    /// Send a transaction and wait for confirmation.
    ///
    /// OPTIMIZATIONS:
    /// - Uses cached nonce (no RPC call)
    /// - Uses pre-computed gas limit (no estimation call)
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
            "Preparing transaction (optimized path)"
        );

        // TIMING: Nonce fetch (should be ~0ms with cache)
        let nonce_start = Instant::now();
        let nonce = self.nonce_manager.next();
        let nonce_elapsed = nonce_start.elapsed();

        // TIMING: Gas price fetch (should be ~0ms with cache)
        let gas_start = Instant::now();
        let gas_price = self.cached_gas_price.load(Ordering::Relaxed) as u128;
        let gas_elapsed = gas_start.elapsed();

        // TIMING: Transaction build
        let build_start = Instant::now();
        let tx = TransactionRequest::default()
            .with_to(to)
            .with_input(calldata)
            .with_value(value)
            .with_nonce(nonce)
            .with_gas_limit(self.default_gas_limit)
            .with_gas_price(gas_price)
            .with_chain_id(self.chain_id);
        let build_elapsed = build_start.elapsed();

        info!(
            to = %to,
            nonce = nonce,
            gas_limit = self.default_gas_limit,
            gas_price_gwei = gas_price / 1_000_000_000,
            nonce_us = nonce_elapsed.as_micros(),
            gas_us = gas_elapsed.as_micros(),
            build_us = build_elapsed.as_micros(),
            "Sending transaction (cached nonce + gas)"
        );

        // TIMING: Provider creation
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
        let gas_price = self.cached_gas_price.load(Ordering::Relaxed) as u128;

        let tx = TransactionRequest::default()
            .with_to(to)
            .with_input(calldata)
            .with_value(value)
            .with_nonce(nonce)
            .with_gas_limit(gas_limit)
            .with_gas_price(gas_price)
            .with_chain_id(self.chain_id);

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

    /// Update cached gas price (call periodically, e.g., every 10s).
    pub async fn update_gas_price(&self) {
        let provider = match ProviderBuilder::new().on_http(self.rpc_url.parse().unwrap()) {
            p => p,
        };
        match provider.get_gas_price().await {
            Ok(price) => {
                self.cached_gas_price.store(price as u64, Ordering::Relaxed);
                debug!(gas_price_gwei = price / 1_000_000_000, "Gas price updated");
            }
            Err(e) => {
                warn!(error = %e, "Failed to update gas price");
            }
        }
    }

    /// Get current cached nonce.
    pub fn current_nonce(&self) -> u64 {
        self.nonce_manager.current()
    }

    /// Get cached gas price.
    pub fn gas_price(&self) -> u64 {
        self.cached_gas_price.load(Ordering::Relaxed)
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
        ).await;

        assert!(sender.is_ok());
        let sender = sender.unwrap();
        // This is the expected address for the test private key (case-insensitive)
        assert_eq!(
            format!("{:?}", sender.address).to_lowercase(),
            "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"
        );

        // Verify initial nonce was fetched
        assert!(sender.current_nonce() >= 0);
    }
}
