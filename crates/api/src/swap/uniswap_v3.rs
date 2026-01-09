//! Uniswap V3 swap router implementation.
//!
//! Provides direct integration with Uniswap V3 for chains where it's deployed.
//! Uses QuoterV2 for price quotes and generates SwapRouter02-compatible routes.

use super::{SwapAllocation, SwapHop, SwapParams, SwapRoute, SwapRouter};
use alloy::primitives::{Address, Bytes, U160, U256};
use alloy::providers::ProviderBuilder;
use alloy::sol;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

// Uniswap V3 QuoterV2 interface
sol! {
    #[sol(rpc)]
    interface IQuoterV2 {
        struct QuoteExactInputSingleParams {
            address tokenIn;
            address tokenOut;
            uint256 amountIn;
            uint24 fee;
            uint160 sqrtPriceLimitX96;
        }

        function quoteExactInputSingle(QuoteExactInputSingleParams memory params)
            external
            returns (
                uint256 amountOut,
                uint160 sqrtPriceX96After,
                uint32 initializedTicksCrossed,
                uint256 gasEstimate
            );
    }
}

/// Uniswap V3 contract addresses per chain.
#[derive(Debug, Clone)]
pub struct UniswapV3Addresses {
    pub swap_router: Address,
    pub quoter_v2: Address,
    pub factory: Address,
}

impl UniswapV3Addresses {
    /// Get Uniswap V3 addresses for Plasma mainnet.
    pub fn plasma() -> Self {
        Self {
            swap_router: "0x807F4E281B7A3B324825C64ca53c69F0b418dE40".parse().unwrap(),
            quoter_v2: "0xaa52bB8110fE38D0d2d2AF0B85C3A3eE622CA455".parse().unwrap(),
            factory: "0xcb2436774C3e191c85056d248EF4260ce5f27A9D".parse().unwrap(),
        }
    }

    /// Get Uniswap V3 addresses for Arbitrum.
    pub fn arbitrum() -> Self {
        Self {
            swap_router: "0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45".parse().unwrap(),
            quoter_v2: "0x61fFE014bA17989E743c5F6cB21bF9697530B21e".parse().unwrap(),
            factory: "0x1F98431c8aD98523631AE4a59f267346ea31F984".parse().unwrap(),
        }
    }

    /// Get Uniswap V3 addresses for Base.
    pub fn base() -> Self {
        Self {
            swap_router: "0x2626664c2603336E57B271c5C0b26F421741e481".parse().unwrap(),
            quoter_v2: "0x3d4e44Eb1374240CE5F1B871ab261CD16335B76a".parse().unwrap(),
            factory: "0x33128a8fC17869897dcE68Ed026d694621f6FDfD".parse().unwrap(),
        }
    }

    /// Get Uniswap V3 addresses for Optimism.
    pub fn optimism() -> Self {
        Self {
            swap_router: "0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45".parse().unwrap(),
            quoter_v2: "0x61fFE014bA17989E743c5F6cB21bF9697530B21e".parse().unwrap(),
            factory: "0x1F98431c8aD98523631AE4a59f267346ea31F984".parse().unwrap(),
        }
    }

    /// Get Uniswap V3 addresses for Celo.
    pub fn celo() -> Self {
        Self {
            swap_router: "0x5615CDAb10dc425a742d643d949a7F474C01abc4".parse().unwrap(),
            quoter_v2: "0x82825d0554fA07f7FC52Ab63c961F330fdEFa8E8".parse().unwrap(),
            factory: "0xAfE208a311B21f13EF87E33A90049fC17A7acDEc".parse().unwrap(),
        }
    }
}

/// Common Uniswap V3 fee tiers in hundredths of a basis point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeeTier {
    /// 0.01% - Ultra stable pairs (e.g., USDC/USDT)
    Lowest = 100,
    /// 0.05% - Stable pairs
    Low = 500,
    /// 0.3% - Standard pairs
    Medium = 3000,
    /// 1% - Exotic pairs
    High = 10000,
}

impl FeeTier {
    /// Get all fee tiers to try, ordered by likelihood for the given token types.
    pub fn tiers_for_pair(is_stable_pair: bool) -> Vec<u32> {
        if is_stable_pair {
            vec![100, 500, 3000] // Try lowest fees first for stables
        } else {
            vec![3000, 500, 10000, 100] // Standard fee first for volatile
        }
    }
}

/// Uniswap V3 swap router.
#[derive(Clone)]
pub struct UniswapV3Router {
    /// RPC URL for quotes
    rpc_url: String,
    /// Contract addresses per chain
    addresses: HashMap<u64, UniswapV3Addresses>,
    /// Supported chain IDs
    supported_chains: Vec<u64>,
    /// Cache for successful fee tiers per token pair
    fee_cache: Arc<RwLock<HashMap<(Address, Address), u32>>>,
    /// Known stablecoin addresses (for fee tier selection)
    stablecoins: Vec<Address>,
}

impl std::fmt::Debug for UniswapV3Router {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UniswapV3Router")
            .field("rpc_url", &self.rpc_url)
            .field("supported_chains", &self.supported_chains)
            .finish()
    }
}

impl UniswapV3Router {
    /// Create a new Uniswap V3 router for a specific chain.
    pub fn new(rpc_url: impl Into<String>, chain_id: u64) -> Self {
        let rpc_url = rpc_url.into();
        let mut addresses = HashMap::new();
        let mut supported_chains = Vec::new();

        // Add chain-specific addresses
        match chain_id {
            9745 => {
                addresses.insert(9745, UniswapV3Addresses::plasma());
                supported_chains.push(9745);
            }
            42161 => {
                addresses.insert(42161, UniswapV3Addresses::arbitrum());
                supported_chains.push(42161);
            }
            8453 => {
                addresses.insert(8453, UniswapV3Addresses::base());
                supported_chains.push(8453);
            }
            10 => {
                addresses.insert(10, UniswapV3Addresses::optimism());
                supported_chains.push(10);
            }
            42220 => {
                addresses.insert(42220, UniswapV3Addresses::celo());
                supported_chains.push(42220);
            }
            _ => {
                warn!(chain_id, "No Uniswap V3 addresses configured for chain");
            }
        }

        Self {
            rpc_url,
            addresses,
            supported_chains,
            fee_cache: Arc::new(RwLock::new(HashMap::new())),
            stablecoins: Vec::new(),
        }
    }

    /// Add known stablecoin addresses for better fee tier selection.
    pub fn with_stablecoins(mut self, stablecoins: Vec<Address>) -> Self {
        self.stablecoins = stablecoins;
        self
    }

    /// Check if a token is a known stablecoin.
    fn is_stablecoin(&self, token: &Address) -> bool {
        self.stablecoins.contains(token)
    }

    /// Check if a pair is a stable-stable pair.
    fn is_stable_pair(&self, token_in: &Address, token_out: &Address) -> bool {
        self.is_stablecoin(token_in) && self.is_stablecoin(token_out)
    }

    /// Get cached fee tier for a token pair.
    async fn get_cached_fee(&self, token_in: Address, token_out: Address) -> Option<u32> {
        let cache = self.fee_cache.read().await;
        cache
            .get(&(token_in, token_out))
            .or_else(|| cache.get(&(token_out, token_in)))
            .copied()
    }

    /// Cache a successful fee tier for a token pair.
    async fn cache_fee(&self, token_in: Address, token_out: Address, fee: u32) {
        let mut cache = self.fee_cache.write().await;
        cache.insert((token_in, token_out), fee);
    }

    /// Get quote from QuoterV2 contract.
    async fn get_quote(
        &self,
        chain_id: u64,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        fee: u32,
    ) -> Result<U256> {
        let addrs = self
            .addresses
            .get(&chain_id)
            .ok_or_else(|| anyhow::anyhow!("No addresses for chain {}", chain_id))?;

        let provider = ProviderBuilder::new().on_http(self.rpc_url.parse()?);
        let quoter = IQuoterV2::new(addrs.quoter_v2, provider);

        let params = IQuoterV2::QuoteExactInputSingleParams {
            tokenIn: token_in,
            tokenOut: token_out,
            amountIn: amount_in,
            fee: alloy::primitives::Uint::<24, 1>::from(fee),
            sqrtPriceLimitX96: U160::ZERO,
        };

        let result = quoter.quoteExactInputSingle(params).call().await?;
        Ok(result.amountOut)
    }

    /// Find the best fee tier for a token pair by trying quotes.
    async fn find_best_fee(
        &self,
        chain_id: u64,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> Result<(u32, U256)> {
        // Check cache first
        if let Some(cached_fee) = self.get_cached_fee(token_in, token_out).await {
            debug!(
                fee = cached_fee,
                "Using cached fee tier for pair"
            );
            let quote = self.get_quote(chain_id, token_in, token_out, amount_in, cached_fee).await?;
            return Ok((cached_fee, quote));
        }

        // Determine fee tiers to try based on token types
        let is_stable = self.is_stable_pair(&token_in, &token_out);
        let fee_tiers = FeeTier::tiers_for_pair(is_stable);

        let mut best_fee = 3000u32;
        let mut best_quote = U256::ZERO;

        for fee in fee_tiers {
            match self.get_quote(chain_id, token_in, token_out, amount_in, fee).await {
                Ok(quote) if quote > best_quote => {
                    best_fee = fee;
                    best_quote = quote;
                    debug!(fee, quote = %quote, "Found better quote");
                }
                Ok(_) => {
                    debug!(fee, "Quote not better than current best");
                }
                Err(e) => {
                    debug!(fee, error = %e, "Fee tier not available for pair");
                }
            }
        }

        if best_quote.is_zero() {
            anyhow::bail!("No liquidity found for pair {:?} -> {:?}", token_in, token_out);
        }

        // Cache the best fee
        self.cache_fee(token_in, token_out, best_fee).await;

        Ok((best_fee, best_quote))
    }
}

#[async_trait]
impl SwapRouter for UniswapV3Router {
    fn router_id(&self) -> &str {
        "uniswap-v3"
    }

    fn supported_chains(&self) -> &[u64] {
        &self.supported_chains
    }

    async fn get_route(&self, params: SwapParams) -> Result<SwapRoute> {
        // For now, assume single chain (could be extended to support multiple)
        let chain_id = *self.supported_chains.first()
            .ok_or_else(|| anyhow::anyhow!("No supported chains configured"))?;

        debug!(
            token_in = %params.token_in,
            token_out = %params.token_out,
            amount_in = %params.amount_in,
            "Getting Uniswap V3 route"
        );

        // Find best fee tier and get quote
        let (fee, expected_output) = self
            .find_best_fee(chain_id, params.token_in, params.token_out, params.amount_in)
            .await?;

        // Calculate min output with slippage
        let slippage_factor = U256::from(10000 - params.slippage_bps as u64);
        let min_output = expected_output * slippage_factor / U256::from(10000);

        // Create single-hop allocation
        let allocation = SwapAllocation {
            token_in: params.token_in,
            token_out: params.token_out,
            router_index: 0, // Uniswap V3
            fee,
            amount_in: params.amount_in,
            stable: self.is_stable_pair(&params.token_in, &params.token_out),
        };

        Ok(SwapRoute {
            token_in: params.token_in,
            token_out: params.token_out,
            amount_in: params.amount_in,
            expected_output,
            min_output,
            hops: vec![SwapHop {
                allocations: vec![allocation],
            }],
            tokens: vec![params.token_in, params.token_out],
            price_impact: None, // Could calculate from sqrt price
            expected_input_usd: None,
            expected_output_usd: None,
            encoded_calldata: None, // Encoding done at execution time
        })
    }

    async fn get_route_cached(&self, params: SwapParams) -> Result<SwapRoute> {
        // For Uniswap V3, caching is less useful since we need real-time quotes
        // But the fee tier is cached, which helps
        self.get_route(params).await
    }

    fn encode_route(&self, route: &SwapRoute) -> Result<Bytes> {
        // Route encoding is handled by the liquidator contract
        // The contract uses the SwapAllocation data to call SwapRouter02
        if let Some(ref calldata) = route.encoded_calldata {
            return Ok(calldata.clone());
        }

        anyhow::bail!("Route encoding requires contract ABI - use LiquidatorContract")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plasma_addresses() {
        let addrs = UniswapV3Addresses::plasma();
        assert_eq!(
            addrs.swap_router,
            "0x807F4E281B7A3B324825C64ca53c69F0b418dE40".parse::<Address>().unwrap()
        );
    }

    #[test]
    fn test_fee_tiers() {
        let stable_tiers = FeeTier::tiers_for_pair(true);
        assert_eq!(stable_tiers[0], 100); // Lowest first for stables

        let volatile_tiers = FeeTier::tiers_for_pair(false);
        assert_eq!(volatile_tiers[0], 3000); // Medium first for volatile
    }

    #[test]
    fn test_router_creation() {
        let router = UniswapV3Router::new("https://rpc.plasma.to", 9745);
        assert_eq!(router.router_id(), "uniswap-v3");
        assert!(router.supports_chain(9745));
        assert!(!router.supports_chain(1));
    }
}
