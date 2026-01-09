//! Liqd.ag swap router implementation.
//!
//! Provides integration with Liqd.ag DEX aggregator for HyperLiquid.

use super::{SwapAllocation, SwapHop, SwapParams, SwapRoute, SwapRouter};
use alloy::primitives::{Address, Bytes, U256};
use anyhow::Result;
use async_trait::async_trait;
use dashmap::DashMap;
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, instrument};

/// HyperLiquid chain ID.
const HYPERLIQUID_CHAIN_ID: u64 = 998;

/// Cached swap route with timestamp for TTL expiration.
#[derive(Clone)]
struct CachedRoute {
    route: SwapRoute,
    cached_at: Instant,
}

/// Cache key for swap routes.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct CacheKey {
    token_in: Address,
    token_out: Address,
    /// Bucketed amount (rounded to reduce cache misses)
    amount_bucket: u64,
}

/// Liqd.ag swap router with caching.
#[derive(Clone)]
pub struct LiqdRouter {
    client: reqwest::Client,
    base_url: String,
    /// Route cache
    cache: Arc<DashMap<CacheKey, CachedRoute>>,
    /// Cache TTL (default: 5 seconds)
    cache_ttl: Duration,
    /// Supported chain IDs
    supported_chains: Vec<u64>,
}

impl std::fmt::Debug for LiqdRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiqdRouter")
            .field("base_url", &self.base_url)
            .field("cache_size", &self.cache.len())
            .field("cache_ttl", &self.cache_ttl)
            .field("supported_chains", &self.supported_chains)
            .finish()
    }
}

impl LiqdRouter {
    /// Create a new Liqd router.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: "https://api.liqd.ag".to_string(),
            cache: Arc::new(DashMap::new()),
            cache_ttl: Duration::from_secs(5),
            supported_chains: vec![HYPERLIQUID_CHAIN_ID],
        }
    }

    /// Create with custom cache TTL.
    pub fn with_cache_ttl(mut self, cache_ttl: Duration) -> Self {
        self.cache_ttl = cache_ttl;
        self
    }

    /// Create with custom base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Bucket amount to nearest power for cache efficiency.
    fn bucket_amount(amount: U256) -> u64 {
        let amount_u128: u128 = amount.to();
        if amount_u128 == 0 {
            return 0;
        }
        let log_amount = (amount_u128 as f64).log10();
        (log_amount * 100.0) as u64
    }

    /// Format raw token amount to human-readable string.
    fn format_amount(amount: U256, decimals: u8) -> String {
        let divisor = 10_u128.pow(decimals as u32);
        let amount_u128 = amount.to_string().parse::<u128>().unwrap_or(0);

        if divisor == 0 {
            return "0".to_string();
        }

        let whole = amount_u128 / divisor;
        let frac = amount_u128 % divisor;

        if frac == 0 {
            whole.to_string()
        } else {
            let frac_str = format!("{:0width$}", frac, width = decimals as usize);
            let trimmed = frac_str.trim_end_matches('0');
            if trimmed.is_empty() {
                whole.to_string()
            } else {
                format!("{}.{}", whole, trimmed)
            }
        }
    }

    /// Fetch route from Liqd API.
    #[instrument(skip(self), fields(token_in = %params.token_in, token_out = %params.token_out))]
    async fn fetch_route(&self, params: &SwapParams) -> Result<SwapRoute> {
        let url = format!("{}/v2/route", self.base_url);

        let token_in_str = format!("{}", params.token_in).to_lowercase();
        let token_out_str = format!("{}", params.token_out).to_lowercase();
        let amount_human = Self::format_amount(params.amount_in, params.decimals_in);

        debug!(
            token_in = %token_in_str,
            token_out = %token_out_str,
            amount = %amount_human,
            "Requesting swap route from Liqd"
        );

        let response = self
            .client
            .get(&url)
            .query(&[
                ("tokenIn", token_in_str),
                ("tokenOut", token_out_str),
                ("amountIn", amount_human),
                ("multiHop", params.multi_hop.to_string()),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Liqd API error: {} - {}", status, body);
        }

        let api_response: LiqdApiResponse = response.json().await?;
        self.convert_response(params, api_response)
    }

    /// Convert API response to SwapRoute.
    fn convert_response(&self, params: &SwapParams, response: LiqdApiResponse) -> Result<SwapRoute> {
        if !response.success {
            let msg = response.message.unwrap_or_else(|| "Unknown API error".to_string());
            anyhow::bail!("Liqd API returned error: {}", msg);
        }

        let execution = response
            .execution
            .ok_or_else(|| anyhow::anyhow!("Missing execution info in response"))?;

        let mut hops = Vec::new();
        let mut tokens = vec![params.token_in];

        // Process hop swaps
        for api_hop in execution.details.hop_swaps {
            let mut hop_allocations = Vec::new();

            for alloc in api_hop {
                let alloc_amount: U256 = alloc.amount_in.parse().unwrap_or(U256::ZERO);

                let allocation = SwapAllocation {
                    token_in: alloc.token_in.parse().unwrap_or(Address::ZERO),
                    token_out: alloc.token_out.parse().unwrap_or(Address::ZERO),
                    router_index: alloc.router_index,
                    fee: alloc.fee,
                    amount_in: alloc_amount,
                    stable: alloc.stable,
                };

                let out_addr: Address = alloc.token_out.parse().unwrap_or(Address::ZERO);
                if !tokens.contains(&out_addr) {
                    tokens.push(out_addr);
                }

                hop_allocations.push(allocation);
            }

            hops.push(SwapHop {
                allocations: hop_allocations,
            });
        }

        let expected_output: U256 = execution.details.amount_out.parse().unwrap_or(U256::ZERO);
        let min_output: U256 = execution.details.min_amount_out.parse().unwrap_or(U256::ZERO);

        let price_impact = response
            .average_price_impact
            .and_then(|s| s.trim_end_matches('%').parse::<f64>().ok());

        // Parse calldata if available
        let encoded_calldata = if execution.calldata.starts_with("0x") {
            hex::decode(&execution.calldata[2..])
                .ok()
                .map(Bytes::from)
        } else {
            hex::decode(&execution.calldata).ok().map(Bytes::from)
        };

        Ok(SwapRoute {
            token_in: params.token_in,
            token_out: params.token_out,
            amount_in: params.amount_in,
            expected_output,
            min_output,
            hops,
            tokens,
            price_impact,
            expected_input_usd: None,
            expected_output_usd: None,
            encoded_calldata,
        })
    }

    /// Clear expired entries from cache.
    pub fn cleanup_cache(&self) {
        self.cache
            .retain(|_, cached| cached.cached_at.elapsed() < self.cache_ttl);
    }

    /// Get current cache size.
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }
}

impl Default for LiqdRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SwapRouter for LiqdRouter {
    fn router_id(&self) -> &str {
        "liqd"
    }

    fn supported_chains(&self) -> &[u64] {
        &self.supported_chains
    }

    async fn get_route(&self, params: SwapParams) -> Result<SwapRoute> {
        self.fetch_route(&params).await
    }

    async fn get_route_cached(&self, params: SwapParams) -> Result<SwapRoute> {
        let cache_key = CacheKey {
            token_in: params.token_in,
            token_out: params.token_out,
            amount_bucket: Self::bucket_amount(params.amount_in),
        };

        // Check cache first
        if let Some(cached) = self.cache.get(&cache_key) {
            if cached.cached_at.elapsed() < self.cache_ttl {
                debug!(
                    token_in = %params.token_in,
                    token_out = %params.token_out,
                    cache_age_ms = cached.cached_at.elapsed().as_millis(),
                    "Cache hit for swap route"
                );
                let mut route = cached.route.clone();
                route.amount_in = params.amount_in;
                return Ok(route);
            }
        }

        // Cache miss - fetch fresh
        debug!(
            token_in = %params.token_in,
            token_out = %params.token_out,
            "Cache miss, fetching fresh swap route"
        );

        let route = self.fetch_route(&params).await?;

        // Cache the result
        self.cache.insert(
            cache_key,
            CachedRoute {
                route: route.clone(),
                cached_at: Instant::now(),
            },
        );

        Ok(route)
    }

    fn encode_route(&self, route: &SwapRoute) -> Result<Bytes> {
        // If we have pre-encoded calldata from the API, use it
        if let Some(ref calldata) = route.encoded_calldata {
            return Ok(calldata.clone());
        }

        // Otherwise, we need to encode using the contract ABI
        // This is typically done by the LiquidatorContract, not here
        anyhow::bail!("Route encoding requires contract ABI - use LiquidatorContract.encode_liquidate()")
    }
}

// API response types

#[derive(Debug, Deserialize)]
struct LiqdApiResponse {
    success: bool,
    #[serde(rename = "averagePriceImpact")]
    average_price_impact: Option<String>,
    execution: Option<ExecutionInfo>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExecutionInfo {
    #[serde(rename = "to")]
    _to: String,
    calldata: String,
    details: ExecutionDetails,
}

#[derive(Debug, Deserialize)]
struct ExecutionDetails {
    #[serde(rename = "path")]
    _path: Vec<String>,
    #[serde(rename = "amountIn")]
    _amount_in: String,
    #[serde(rename = "amountOut")]
    amount_out: String,
    #[serde(rename = "minAmountOut")]
    min_amount_out: String,
    #[serde(rename = "hopSwaps")]
    hop_swaps: Vec<Vec<ApiAllocation>>,
}

#[derive(Debug, Deserialize)]
struct ApiAllocation {
    #[serde(rename = "tokenIn")]
    token_in: String,
    #[serde(rename = "tokenOut")]
    token_out: String,
    #[serde(rename = "routerIndex")]
    router_index: u8,
    fee: u32,
    #[serde(rename = "amountIn")]
    amount_in: String,
    stable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_liqd_router_creation() {
        let router = LiqdRouter::new();
        assert_eq!(router.router_id(), "liqd");
        assert!(router.supports_chain(HYPERLIQUID_CHAIN_ID));
        assert!(!router.supports_chain(1)); // Ethereum not supported
    }

    #[test]
    fn test_amount_bucketing() {
        // Same order of magnitude should bucket similarly
        let b1 = LiqdRouter::bucket_amount(U256::from(1_000_000u64));
        let b2 = LiqdRouter::bucket_amount(U256::from(1_010_000u64));
        assert!((b1 as i64 - b2 as i64).abs() < 5); // Within 5 buckets

        // Different orders of magnitude should bucket differently
        let b3 = LiqdRouter::bucket_amount(U256::from(10_000_000u64));
        assert!(b3 > b1);
    }

    #[test]
    fn test_format_amount() {
        // 1.5 USDC (6 decimals)
        let amount = U256::from(1_500_000u64);
        let formatted = LiqdRouter::format_amount(amount, 6);
        assert_eq!(formatted, "1.5");

        // 1 ETH (18 decimals)
        let amount = U256::from(1_000_000_000_000_000_000u128);
        let formatted = LiqdRouter::format_amount(amount, 18);
        assert_eq!(formatted, "1");

        // 0.5 ETH (18 decimals)
        let amount = U256::from(500_000_000_000_000_000u128);
        let formatted = LiqdRouter::format_amount(amount, 18);
        assert_eq!(formatted, "0.5");
    }

    #[test]
    fn test_deserialize_api_response() {
        let json = r#"{
            "success": true,
            "averagePriceImpact": "0.5%",
            "execution": {
                "to": "0x744489ee3d540777a66f2cf297479745e0852f7a",
                "calldata": "0xabcd",
                "details": {
                    "path": ["0x1111", "0x2222"],
                    "amountIn": "1000000",
                    "amountOut": "500000000000000000",
                    "minAmountOut": "495000000000000000",
                    "hopSwaps": [[{
                        "tokenIn": "0x1111",
                        "tokenOut": "0x2222",
                        "routerIndex": 0,
                        "fee": 3000,
                        "amountIn": "1000000",
                        "stable": false
                    }]]
                }
            }
        }"#;

        let response: LiqdApiResponse = serde_json::from_str(json).unwrap();
        assert!(response.success);
        assert!(response.execution.is_some());
    }
}
