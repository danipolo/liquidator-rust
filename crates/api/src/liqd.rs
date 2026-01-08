//! Liqd.ag swap routing API client.

use alloy::primitives::{Address, U256};
use anyhow::Result;
use dashmap::DashMap;
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, instrument};

/// Cached swap route with timestamp for TTL expiration.
#[derive(Clone)]
struct CachedRoute {
    route: SwapRoute,
    cached_at: Instant,
}

/// Cache key for swap routes.
/// Uses token pair and bucketed amount for efficient lookups.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct CacheKey {
    token_in: Address,
    token_out: Address,
    /// Bucketed amount (rounded to reduce cache misses)
    amount_bucket: u64,
}

/// Liqd.ag swap routing client with caching.
#[derive(Clone)]
pub struct LiqdClient {
    client: reqwest::Client,
    base_url: String,
    /// Route cache: CacheKey -> cached route
    /// OPTIMIZATION: Cache swap routes for common token pairs to avoid repeated API calls.
    cache: Arc<DashMap<CacheKey, CachedRoute>>,
    /// Cache TTL (default: 5 seconds = ~25 blocks on HyperLiquid)
    cache_ttl: Duration,
}

impl std::fmt::Debug for LiqdClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiqdClient")
            .field("base_url", &self.base_url)
            .field("cache_size", &self.cache.len())
            .field("cache_ttl", &self.cache_ttl)
            .finish()
    }
}

impl LiqdClient {
    /// Create a new Liqd client with caching.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: "https://api.liqd.ag".to_string(),
            cache: Arc::new(DashMap::new()),
            cache_ttl: Duration::from_secs(5),
        }
    }

    /// Create a new Liqd client with custom cache TTL.
    pub fn with_cache_ttl(cache_ttl: Duration) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: "https://api.liqd.ag".to_string(),
            cache: Arc::new(DashMap::new()),
            cache_ttl,
        }
    }

    /// Bucket amount to nearest power for cache efficiency.
    /// This reduces cache fragmentation by grouping similar amounts.
    fn bucket_amount(amount: U256) -> u64 {
        // Round to nearest 1% bucket using log scale
        // This means amounts within ~1% of each other will hit the same cache entry
        let amount_u128: u128 = amount.to();
        if amount_u128 == 0 {
            return 0;
        }
        // Use log10 * 100 as bucket (100 buckets per order of magnitude)
        let log_amount = (amount_u128 as f64).log10();
        (log_amount * 100.0) as u64
    }

    /// Get swap route with caching.
    /// Returns cached route if available and not expired, otherwise fetches fresh.
    #[instrument(skip(self), fields(token_in = %token_in, token_out = %token_out))]
    pub async fn get_swap_route_cached(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        decimals_in: u8,
        multi_hop: bool,
    ) -> Result<SwapRoute> {
        let cache_key = CacheKey {
            token_in,
            token_out,
            amount_bucket: Self::bucket_amount(amount_in),
        };

        // Check cache first
        if let Some(cached) = self.cache.get(&cache_key) {
            if cached.cached_at.elapsed() < self.cache_ttl {
                debug!(
                    token_in = %token_in,
                    token_out = %token_out,
                    cache_age_ms = cached.cached_at.elapsed().as_millis(),
                    "Cache hit for swap route"
                );
                // Return cached route with updated amount_in
                let mut route = cached.route.clone();
                route.amount_in = amount_in;
                return Ok(route);
            }
        }

        // Cache miss or expired - fetch fresh route
        debug!(
            token_in = %token_in,
            token_out = %token_out,
            "Cache miss, fetching fresh swap route"
        );

        let route = self.get_swap_route(token_in, token_out, amount_in, decimals_in, multi_hop).await?;

        // Cache the result
        self.cache.insert(cache_key, CachedRoute {
            route: route.clone(),
            cached_at: Instant::now(),
        });

        Ok(route)
    }

    /// Clear expired entries from cache (call periodically).
    pub fn cleanup_cache(&self) {
        self.cache.retain(|_, cached| cached.cached_at.elapsed() < self.cache_ttl);
    }

    /// Get current cache size.
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    /// Create a simple direct swap route (fallback when API unavailable).
    /// Uses a default fee tier and assumes direct swap without routing.
    pub fn create_direct_route(
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> SwapRoute {
        // Create a simple single-hop direct swap
        let allocation = SwapAllocation {
            token_in,
            token_out,
            router_index: 0, // Default router (Uniswap V3 style)
            fee: 3000,       // 0.3% fee tier (common default)
            amount_in,
            stable: false,
        };

        SwapRoute {
            token_in,
            token_out,
            amount_in,
            expected_output: amount_in, // Assume 1:1 for testing (will be corrected on-chain)
            hops: vec![SwapHop {
                allocations: vec![allocation],
            }],
            tokens: vec![token_in, token_out],
            price_impact: None,
            expected_input_usd: None,
            expected_output_usd: None,
        }
    }

    /// Create a client with custom base URL.
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
            cache: Arc::new(DashMap::new()),
            cache_ttl: Duration::from_secs(5),
        }
    }

    /// Get swap route from tokenIn to tokenOut.
    ///
    /// Note: `amount_in` is the raw token amount (with decimals).
    /// `decimals_in` is needed to convert to human-readable format for the API.
    #[instrument(skip(self), fields(token_in = %token_in, token_out = %token_out))]
    pub async fn get_swap_route(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        decimals_in: u8,
        multi_hop: bool,
    ) -> Result<SwapRoute> {
        let url = format!("{}/v2/route", self.base_url);

        // Format addresses as lowercase strings (liqd.ag prefers lowercase)
        let token_in_str = format!("{}", token_in).to_lowercase();
        let token_out_str = format!("{}", token_out).to_lowercase();
        // Convert raw amount to human-readable format
        let amount_human = Self::format_amount(amount_in, decimals_in);

        debug!(
            token_in = %token_in_str,
            token_out = %token_out_str,
            amount = %amount_human,
            decimals = decimals_in,
            "Requesting swap route"
        );

        let full_url = format!(
            "{}?tokenIn={}&tokenOut={}&amountIn={}&multiHop={}",
            url, token_in_str, token_out_str, amount_human, multi_hop
        );
        debug!(full_url = %full_url, "Full Liqd API URL");

        let response = self
            .client
            .get(&url)
            .query(&[
                ("tokenIn", token_in_str),
                ("tokenOut", token_out_str),
                ("amountIn", amount_human),
                ("multiHop", multi_hop.to_string()),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Liqd API error: {} - {}", status, body);
        }

        let api_response: LiqdApiResponse = response.json().await?;

        // Convert API response to SwapRoute
        let route = self.convert_response(token_in, token_out, amount_in, api_response)?;

        debug!(
            hops = route.hops.len(),
            expected_output = %route.expected_output,
            "Got swap route"
        );

        Ok(route)
    }

    /// Convert API response to internal SwapRoute format (v2 API).
    fn convert_response(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        response: LiqdApiResponse,
    ) -> Result<SwapRoute> {
        // Check for API errors
        if !response.success {
            let msg = response.message.unwrap_or_else(|| "Unknown API error".to_string());
            anyhow::bail!("Liqd API returned error: {}", msg);
        }

        let execution = response.execution
            .ok_or_else(|| anyhow::anyhow!("Missing execution info in response"))?;

        let mut hops = Vec::new();
        let mut tokens = vec![token_in];

        // Process hop swaps from execution details
        for api_hop in execution.details.hop_swaps {
            let mut hop_allocations = Vec::new();

            for alloc in api_hop {
                // Parse amount_in as U256 (it's already in raw format)
                let alloc_amount: U256 = alloc.amount_in.parse().unwrap_or(U256::ZERO);

                let allocation = SwapAllocation {
                    token_in: alloc.token_in.parse().unwrap_or(Address::ZERO),
                    token_out: alloc.token_out.parse().unwrap_or(Address::ZERO),
                    router_index: alloc.router_index,
                    fee: alloc.fee,
                    amount_in: alloc_amount,
                    stable: alloc.stable,
                };

                // Track intermediate tokens
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

        // Parse expected output from execution details (raw amount)
        let expected_output: U256 = execution.details.amount_out.parse().unwrap_or(U256::ZERO);

        // Parse price impact (e.g., "1.995849%" -> 1.995849)
        let price_impact = response.average_price_impact.and_then(|s| {
            s.trim_end_matches('%').parse::<f64>().ok()
        });

        Ok(SwapRoute {
            token_in,
            token_out,
            amount_in,
            expected_output,
            hops,
            tokens,
            price_impact,
            // USD values are calculated by the caller using oracle prices
            expected_input_usd: None,
            expected_output_usd: None,
        })
    }

    /// Build decimals map from token info (v2 API).
    fn build_decimals_map(&self, tokens_info: &TokensInfo) -> std::collections::HashMap<String, u8> {
        let mut map = std::collections::HashMap::new();

        map.insert(
            tokens_info.token_in.address.to_lowercase(),
            tokens_info.token_in.decimals,
        );
        map.insert(
            tokens_info.token_out.address.to_lowercase(),
            tokens_info.token_out.decimals,
        );

        if let Some(ref intermediates) = tokens_info.intermediates {
            for token in intermediates {
                map.insert(token.address.to_lowercase(), token.decimals);
            }
        }

        map
    }

    /// Scale amount to token decimals.
    fn scale_amount(&self, amount: f64, decimals: u8) -> U256 {
        let scaled = amount * 10_f64.powi(decimals as i32);
        U256::from(scaled.floor() as u128)
    }

    /// Format raw token amount to human-readable string for Liqd API.
    /// e.g., 1500000000000000000 with 18 decimals -> "1.5"
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
            // Format with proper decimal places, trimming trailing zeros
            let frac_str = format!("{:0width$}", frac, width = decimals as usize);
            let trimmed = frac_str.trim_end_matches('0');
            if trimmed.is_empty() {
                whole.to_string()
            } else {
                format!("{}.{}", whole, trimmed)
            }
        }
    }
}

impl Default for LiqdClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Swap route for liquidation.
#[derive(Debug, Clone, Default)]
pub struct SwapRoute {
    /// Input token
    pub token_in: Address,
    /// Output token
    pub token_out: Address,
    /// Input amount
    pub amount_in: U256,
    /// Expected output amount
    pub expected_output: U256,
    /// Swap hops
    pub hops: Vec<SwapHop>,
    /// All tokens in the path
    pub tokens: Vec<Address>,
    /// Price impact percentage
    pub price_impact: Option<f64>,
    /// Expected input value in USD (for profitability calculation)
    pub expected_input_usd: Option<f64>,
    /// Expected output value in USD (for profitability calculation)
    pub expected_output_usd: Option<f64>,
}

impl SwapRoute {
    /// Check if this is a direct swap (single hop).
    pub fn is_direct(&self) -> bool {
        self.hops.len() == 1
    }

    /// Get total number of allocations across all hops.
    pub fn total_allocations(&self) -> usize {
        self.hops.iter().map(|h| h.allocations.len()).sum()
    }
}

/// A single hop in the swap route.
#[derive(Debug, Clone)]
pub struct SwapHop {
    /// Allocations in this hop (can be split across multiple pools)
    pub allocations: Vec<SwapAllocation>,
}

/// Single allocation within a hop.
#[derive(Debug, Clone)]
pub struct SwapAllocation {
    /// Input token for this allocation
    pub token_in: Address,
    /// Output token for this allocation
    pub token_out: Address,
    /// Router/DEX index
    pub router_index: u8,
    /// Fee tier (in hundredths of basis points, e.g., 3000 = 0.3%)
    pub fee: u32,
    /// Amount in (scaled to token decimals)
    pub amount_in: U256,
    /// Whether this is a stable pool
    pub stable: bool,
}

// API response types for v2 API

#[derive(Debug, Deserialize)]
pub struct LiqdApiResponse {
    pub success: bool,
    pub tokens: Option<TokensInfo>,
    #[serde(rename = "amountIn")]
    pub amount_in: Option<String>,
    #[serde(rename = "amountOut")]
    pub amount_out: Option<String>,
    #[serde(rename = "averagePriceImpact")]
    pub average_price_impact: Option<String>,
    pub execution: Option<ExecutionInfo>,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TokensInfo {
    #[serde(rename = "tokenIn")]
    pub token_in: TokenDetail,
    #[serde(rename = "tokenOut")]
    pub token_out: TokenDetail,
    pub intermediates: Option<Vec<TokenDetail>>,
}

#[derive(Debug, Deserialize)]
pub struct TokenDetail {
    pub address: String,
    pub symbol: String,
    #[serde(default)]
    pub name: String,
    pub decimals: u8,
}

#[derive(Debug, Deserialize)]
pub struct ExecutionInfo {
    pub to: String,
    pub calldata: String,
    pub details: ExecutionDetails,
}

#[derive(Debug, Deserialize)]
pub struct ExecutionDetails {
    pub path: Vec<String>,
    #[serde(rename = "amountIn")]
    pub amount_in: String,
    #[serde(rename = "amountOut")]
    pub amount_out: String,
    #[serde(rename = "minAmountOut")]
    pub min_amount_out: String,
    #[serde(rename = "hopSwaps")]
    pub hop_swaps: Vec<Vec<ApiAllocation>>,
}

#[derive(Debug, Deserialize)]
pub struct ApiAllocation {
    #[serde(rename = "tokenIn")]
    pub token_in: String,
    #[serde(rename = "tokenOut")]
    pub token_out: String,
    #[serde(rename = "routerIndex")]
    pub router_index: u8,
    #[serde(rename = "routerName")]
    pub router_name: Option<String>,
    pub fee: u32,
    #[serde(rename = "amountIn")]
    pub amount_in: String,
    #[serde(rename = "amountOut")]
    pub amount_out: Option<String>,
    pub stable: bool,
    #[serde(rename = "priceImpact")]
    pub price_impact: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scale_amount() {
        let client = LiqdClient::new();

        // 1.5 with 6 decimals = 1500000
        let scaled = client.scale_amount(1.5, 6);
        assert_eq!(scaled, U256::from(1_500_000u64));

        // 1.5 with 18 decimals
        let scaled = client.scale_amount(1.5, 18);
        assert_eq!(scaled, U256::from(1_500_000_000_000_000_000u64));
    }

    #[test]
    fn test_swap_route_helpers() {
        let route = SwapRoute {
            token_in: Address::ZERO,
            token_out: Address::repeat_byte(1),
            amount_in: U256::from(1000u64),
            expected_output: U256::from(990u64),
            hops: vec![SwapHop {
                allocations: vec![SwapAllocation {
                    token_in: Address::ZERO,
                    token_out: Address::repeat_byte(1),
                    router_index: 0,
                    fee: 3000,
                    amount_in: U256::from(1000u64),
                    stable: false,
                }],
            }],
            tokens: vec![Address::ZERO, Address::repeat_byte(1)],
            price_impact: Some(0.1),
            expected_input_usd: Some(100.0),
            expected_output_usd: Some(99.0),
        };

        assert!(route.is_direct());
        assert_eq!(route.total_allocations(), 1);
    }

    #[test]
    fn test_deserialize_v2_api_response() {
        let json = r#"{
            "success": true,
            "tokens": {
                "tokenIn": {"address": "0x1111", "symbol": "USDC", "name": "USD Coin", "decimals": 6},
                "tokenOut": {"address": "0x2222", "symbol": "WETH", "name": "Wrapped Ether", "decimals": 18},
                "intermediates": []
            },
            "amountIn": "1000000",
            "amountOut": "500000000000000000",
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
                        "routerName": "TestRouter",
                        "fee": 3000,
                        "amountIn": "1000000",
                        "amountOut": "500000000000000000",
                        "stable": false,
                        "priceImpact": "0.5%"
                    }]]
                }
            }
        }"#;

        let response: LiqdApiResponse = serde_json::from_str(json).unwrap();
        assert!(response.success);
        assert!(response.execution.is_some());
        let exec = response.execution.unwrap();
        assert_eq!(exec.details.hop_swaps.len(), 1);
        assert_eq!(exec.details.hop_swaps[0][0].fee, 3000);
    }
}
