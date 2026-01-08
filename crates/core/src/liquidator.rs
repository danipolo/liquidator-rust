//! Liquidation executor for on-chain liquidation transactions.

use alloy::primitives::{Address, U256};
use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, instrument, warn};

use crate::assets::REGISTRY;
use crate::position::TrackedPosition;
use crate::pre_staging::StagedLiquidation;
use crate::u256_math;
use hyperlend_api::{LiqdClient, SwapRoute};
use hyperlend_chain::{LiquidatorContract, ProviderManager, SwapAllocation};

/// Estimated gas cost in USD for a liquidation tx on HyperLiquid EVM.
/// Based on real liquidation data:
/// - Complex liquidation with multi-hop swaps: ~1.57M gas @ 0.7 gwei = ~$0.03
/// - Simple liquidations may use less, but we budget for complex ones
const ESTIMATED_GAS_COST_USD: f64 = 0.03;

/// Close factor for partial liquidations (50%).
const CLOSE_FACTOR: f64 = 0.5;

/// Maximum amount for unlimited debt seizure (2^256 - 1).
const MAX_AMOUNT: &str =
    "115792089237316195423570985008687907853269984665640564039457584007913129639935";

/// Liquidation executor.
pub struct Liquidator {
    /// Provider manager for chain interaction
    provider: Arc<ProviderManager>,

    /// Liquidator contract wrapper
    contract: LiquidatorContract,

    /// Liqd.ag swap routing client
    liqd_client: Arc<LiqdClient>,

    /// Profit receiver address
    profit_receiver: Address,

    /// Minimum profit threshold (USD)
    min_profit_usd: f64,

    /// Slippage tolerance (basis points)
    slippage_bps: u16,
}

impl Liquidator {
    /// Create a new liquidator.
    pub fn new(
        provider: Arc<ProviderManager>,
        contract: LiquidatorContract,
        liqd_client: Arc<LiqdClient>,
        profit_receiver: Address,
    ) -> Self {
        Self {
            provider,
            contract,
            liqd_client,
            profit_receiver,
            min_profit_usd: 1.0,
            slippage_bps: 100, // 1% default slippage
        }
    }

    /// Set minimum profit threshold.
    pub fn with_min_profit(mut self, min_profit_usd: f64) -> Self {
        self.min_profit_usd = min_profit_usd;
        self
    }

    /// Set slippage tolerance.
    pub fn with_slippage(mut self, slippage_bps: u16) -> Self {
        self.slippage_bps = slippage_bps;
        self
    }

    /// Get reference to the Liqd client.
    pub fn liqd_client(&self) -> &LiqdClient {
        &self.liqd_client
    }

    /// Execute a pre-staged liquidation.
    ///
    /// OPTIMIZATION: If staged transaction has pre-encoded calldata,
    /// skips encoding step (~5ms savings).
    #[instrument(skip(self, staged), fields(user = %staged.user))]
    pub async fn execute_staged(&self, staged: StagedLiquidation) -> Result<LiquidationResult> {
        let execution_start = Instant::now();

        // TIMING: Profit estimation
        let profit_start = Instant::now();
        let profit_estimate = self.estimate_staged_profit(&staged);
        let profit_elapsed = profit_start.elapsed();

        info!(
            user = %staged.user,
            collateral = %staged.collateral_asset,
            debt = %staged.debt_asset,
            profit = %profit_estimate.to_string(),
            has_precomputed = staged.has_precomputed_calldata(),
            profit_check_us = profit_elapsed.as_micros(),
            "Evaluating pre-staged liquidation"
        );

        // Check profitability
        if !profit_estimate.is_profitable(self.min_profit_usd) {
            warn!(
                user = %staged.user,
                expected_profit = profit_estimate.net_profit,
                min_required = self.min_profit_usd,
                "Skipping unprofitable liquidation"
            );
            anyhow::bail!(
                "Liquidation not profitable: expected ${:.2}, minimum ${:.2}",
                profit_estimate.net_profit,
                self.min_profit_usd
            );
        }

        info!(
            user = %staged.user,
            expected_profit = format!("${:.2}", profit_estimate.net_profit),
            "Executing profitable liquidation"
        );

        // TIMING: Liquidation execution
        let liquidate_start = Instant::now();
        let (tx_hash, encoding_time_us) = if staged.is_ready_for_instant_execution() {
            info!(user = %staged.user, "Using pre-encoded calldata (fast path)");
            let hash = self.contract
                .execute_preencoded(staged.encoded_calldata.as_ref().unwrap().clone())
                .await?;
            (hash, 0u128) // No encoding time for pre-encoded path
        } else {
            // Fallback: Prepare swap hops and encode at execution time
            let encode_start = Instant::now();
            let (hops, tokens) = self.prepare_hops(&staged.swap_route)?;
            let min_amount_out = self.apply_slippage(staged.debt_to_cover);
            let encode_elapsed = encode_start.elapsed();

            info!(
                user = %staged.user,
                encode_us = encode_elapsed.as_micros(),
                "Using runtime encoding (slow path)"
            );

            let hash = self.contract
                .liquidate(
                    staged.user,
                    staged.collateral_asset,
                    staged.debt_asset,
                    staged.debt_to_cover,
                    hops,
                    tokens,
                    min_amount_out,
                )
                .await?;
            (hash, encode_elapsed.as_micros())
        };
        let liquidate_elapsed = liquidate_start.elapsed();

        info!(
            tx_hash = %tx_hash,
            liquidate_ms = liquidate_elapsed.as_millis(),
            encode_us = encoding_time_us,
            "Liquidation transaction submitted"
        );

        // TIMING: Rescue tokens
        let rescue_start = Instant::now();
        let rescue_hash = self
            .contract
            .rescue_tokens(staged.debt_asset, self.profit_receiver)
            .await?;
        let rescue_elapsed = rescue_start.elapsed();

        let total_elapsed = execution_start.elapsed();

        info!(
            rescue_hash = %rescue_hash,
            rescue_ms = rescue_elapsed.as_millis(),
            total_execution_ms = total_elapsed.as_millis(),
            "[E2E TIMING] profit_check={}us, encode={}us, liquidate_tx={}ms, rescue_tx={}ms, TOTAL={}ms",
            profit_elapsed.as_micros(),
            encoding_time_us,
            liquidate_elapsed.as_millis(),
            rescue_elapsed.as_millis(),
            total_elapsed.as_millis()
        );

        Ok(LiquidationResult {
            user: staged.user,
            collateral_asset: staged.collateral_asset,
            debt_asset: staged.debt_asset,
            debt_covered: staged.debt_to_cover,
            liquidation_tx: tx_hash,
            rescue_tx: rescue_hash,
        })
    }

    /// Build and execute a liquidation from scratch.
    #[instrument(skip(self, position), fields(user = %position.user))]
    pub async fn build_and_execute(&self, position: &TrackedPosition) -> Result<LiquidationResult> {
        // Validate position
        if !position.is_liquidatable() {
            anyhow::bail!("Position not liquidatable (HF >= 1.0)");
        }

        if position.is_bad_debt() {
            anyhow::bail!("Position is bad debt");
        }

        // Get largest collateral and debt
        let (collateral_asset, collateral) = position
            .largest_collateral()
            .ok_or_else(|| anyhow::anyhow!("No collateral found"))?;

        let (debt_asset, debt) = position
            .largest_debt()
            .ok_or_else(|| anyhow::anyhow!("No debt found"))?;

        // Early profitability estimate (before fetching swap route)
        if let Some(early_estimate) = self.estimate_position_profit(position) {
            debug!(
                user = %position.user,
                estimated_profit = %early_estimate.to_string(),
                "Early profit estimate"
            );

            // Skip if clearly unprofitable (give 50% margin for swap route accuracy)
            if early_estimate.net_profit < self.min_profit_usd * 0.5 {
                warn!(
                    user = %position.user,
                    expected_profit = early_estimate.net_profit,
                    min_required = self.min_profit_usd,
                    "Skipping likely unprofitable liquidation (early check)"
                );
                anyhow::bail!(
                    "Liquidation likely not profitable: estimated ${:.2}, minimum ${:.2}",
                    early_estimate.net_profit,
                    self.min_profit_usd
                );
            }
        }

        info!(
            user = %position.user,
            collateral = %collateral_asset,
            debt = %debt_asset,
            hf = position.health_factor,
            collateral_usd = collateral.value_usd,
            debt_usd = debt.value_usd,
            "Building liquidation"
        );

        // Calculate amounts
        let collateral_amount = self.calculate_collateral_amount(collateral.amount);
        let debt_amount = debt.amount;

        // Fetch swap route (with fallback to direct route if API fails)
        let swap_route = match self
            .liqd_client
            .get_swap_route(*collateral_asset, *debt_asset, collateral_amount, collateral.decimals, true)
            .await
        {
            Ok(route) => route,
            Err(e) => {
                warn!(
                    user = %position.user,
                    error = %e,
                    "Swap API failed, using direct route fallback"
                );
                hyperlend_api::LiqdClient::create_direct_route(
                    *collateral_asset,
                    *debt_asset,
                    collateral_amount,
                )
            }
        };

        // Calculate actual profit estimate with real swap route
        let collateral_value_usd = collateral.value_usd * CLOSE_FACTOR;
        let swap_output_usd = match swap_route.expected_output_usd {
            Some(usd) => usd,
            None => {
                warn!(
                    user = %position.user,
                    "Swap route missing expected_output_usd, using 1% slippage estimate"
                );
                collateral_value_usd * 0.99
            }
        };
        let profit_estimate = self.estimate_profit(
            *collateral_asset,
            collateral_value_usd,
            collateral_value_usd,
            swap_output_usd,
        );

        info!(
            user = %position.user,
            profit = %profit_estimate.to_string(),
            "Final profit estimate with swap route"
        );

        // Check profitability with actual swap route
        if !profit_estimate.is_profitable(self.min_profit_usd) {
            warn!(
                user = %position.user,
                expected_profit = profit_estimate.net_profit,
                min_required = self.min_profit_usd,
                "Skipping unprofitable liquidation"
            );
            anyhow::bail!(
                "Liquidation not profitable: expected ${:.2}, minimum ${:.2}",
                profit_estimate.net_profit,
                self.min_profit_usd
            );
        }

        info!(
            user = %position.user,
            expected_profit = format!("${:.2}", profit_estimate.net_profit),
            "Executing profitable liquidation"
        );

        // Determine debt to seize
        let debt_to_cover = self.calculate_debt_to_cover(&swap_route, debt_amount);

        // Prepare hops
        let (hops, tokens) = self.prepare_hops(&swap_route)?;

        // Calculate min amount out
        let min_amount_out = self.apply_slippage(debt_to_cover);

        // Execute liquidation
        let tx_hash = self
            .contract
            .liquidate(
                position.user,
                *collateral_asset,
                *debt_asset,
                debt_to_cover,
                hops,
                tokens,
                min_amount_out,
            )
            .await?;

        info!(tx_hash = %tx_hash, "Liquidation transaction submitted");

        // Rescue tokens
        let rescue_hash = self
            .contract
            .rescue_tokens(*debt_asset, self.profit_receiver)
            .await?;

        info!(rescue_hash = %rescue_hash, "Profit rescued");

        Ok(LiquidationResult {
            user: position.user,
            collateral_asset: *collateral_asset,
            debt_asset: *debt_asset,
            debt_covered: debt_to_cover,
            liquidation_tx: tx_hash,
            rescue_tx: rescue_hash,
        })
    }

    /// Calculate collateral amount to liquidate (50% of position).
    fn calculate_collateral_amount(&self, total_amount: U256) -> U256 {
        // Apply close factor (50%) using integer division to preserve precision
        // For U256: amount / 2 is equivalent to amount * 0.5
        total_amount / U256::from(2)
    }

    /// Calculate debt to cover based on swap output.
    fn calculate_debt_to_cover(&self, swap_route: &SwapRoute, max_debt: U256) -> U256 {
        // If swap output covers all debt, use max amount
        if swap_route.expected_output >= max_debt {
            U256::from_str_radix(MAX_AMOUNT, 10).unwrap_or(max_debt)
        } else {
            swap_route.expected_output
        }
    }

    /// Prepare swap hops for contract call.
    fn prepare_hops(&self, swap_route: &SwapRoute) -> Result<(Vec<Vec<SwapAllocation>>, Vec<Address>)> {
        let mut hops: Vec<Vec<SwapAllocation>> = Vec::new();
        let mut tokens: Vec<Address> = Vec::new();

        // Add input token
        tokens.push(swap_route.token_in);

        for hop in &swap_route.hops {
            let mut hop_allocations = Vec::new();

            for alloc in &hop.allocations {
                hop_allocations.push(SwapAllocation {
                    token_in: alloc.token_in,
                    token_out: alloc.token_out,
                    router_index: alloc.router_index,
                    fee: alloc.fee,
                    amount_in: alloc.amount_in,
                    stable: alloc.stable,
                });

                // Add intermediate tokens
                if !tokens.contains(&alloc.token_out) {
                    tokens.push(alloc.token_out);
                }
            }

            hops.push(hop_allocations);
        }

        Ok((hops, tokens))
    }

    /// Apply slippage tolerance to amount.
    /// Uses native U256 arithmetic (2-5x faster than String parsing).
    #[inline]
    fn apply_slippage(&self, amount: U256) -> U256 {
        u256_math::apply_basis_points(amount, self.slippage_bps)
    }

    /// Check if liquidation would be profitable.
    pub fn is_profitable(&self, expected_profit_usd: f64) -> bool {
        expected_profit_usd >= self.min_profit_usd
    }

    /// Estimate expected profit from a liquidation.
    ///
    /// Profit = (collateral_value * liquidation_bonus) - gas_cost - swap_slippage_cost
    ///
    /// Returns (expected_profit_usd, breakdown) where breakdown contains:
    /// - gross_profit: liquidation bonus value
    /// - gas_cost: estimated gas in USD
    /// - slippage_cost: estimated slippage loss
    pub fn estimate_profit(
        &self,
        collateral_asset: Address,
        collateral_value_usd: f64,
        swap_input_usd: f64,
        swap_output_usd: f64,
    ) -> ProfitEstimate {
        // Get liquidation bonus for the collateral asset
        let liquidation_bonus = REGISTRY.get_liquidation_bonus(&collateral_asset);

        // Gross profit from liquidation bonus
        let gross_profit = collateral_value_usd * liquidation_bonus;

        // Slippage cost (difference between input and output of swap)
        let slippage_cost = (swap_input_usd - swap_output_usd).max(0.0);

        // Net profit after costs
        let net_profit = gross_profit - ESTIMATED_GAS_COST_USD - slippage_cost;

        ProfitEstimate {
            gross_profit,
            gas_cost: ESTIMATED_GAS_COST_USD,
            slippage_cost,
            net_profit,
            liquidation_bonus_pct: liquidation_bonus * 100.0,
        }
    }

    /// Estimate profit from a tracked position.
    pub fn estimate_position_profit(&self, position: &TrackedPosition) -> Option<ProfitEstimate> {
        let (collateral_asset, collateral) = position.largest_collateral()?;

        // Apply close factor to get actual collateral to liquidate
        let collateral_value = collateral.value_usd * CLOSE_FACTOR;

        // Estimate swap output (assume 1% slippage for estimation)
        let estimated_swap_output = collateral_value * 0.99;

        Some(self.estimate_profit(
            *collateral_asset,
            collateral_value,
            collateral_value,
            estimated_swap_output,
        ))
    }

    /// Estimate profit from a staged liquidation.
    pub fn estimate_staged_profit(&self, staged: &StagedLiquidation) -> ProfitEstimate {
        // Calculate collateral value from expected_collateral
        // This is approximate - we use the swap route's expected values
        let collateral_value_usd = staged.swap_route.expected_input_usd.unwrap_or(0.0);
        let swap_output_usd = staged.swap_route.expected_output_usd.unwrap_or(0.0);

        self.estimate_profit(
            staged.collateral_asset,
            collateral_value_usd,
            collateral_value_usd,
            swap_output_usd,
        )
    }

    /// Rescue remaining tokens from the liquidator contract.
    pub async fn rescue_tokens(&self, token: Address) -> Result<alloy::primitives::B256> {
        self.contract.rescue_tokens(token, self.profit_receiver).await
    }

    /// Get the minimum profit threshold.
    pub fn min_profit_usd(&self) -> f64 {
        self.min_profit_usd
    }

    /// Execute a liquidation with retry logic.
    ///
    /// Retries up to `max_retries` times with exponential backoff.
    /// On failure, refreshes the swap route before retrying.
    #[instrument(skip(self, position), fields(user = %position.user))]
    pub async fn execute_with_retry(
        &self,
        position: &TrackedPosition,
        max_retries: u32,
    ) -> Result<LiquidationResult> {
        let mut last_error = None;
        let base_delay = std::time::Duration::from_millis(200); // 200ms base (1 block on HyperLiquid)

        for attempt in 0..=max_retries {
            if attempt > 0 {
                let delay = base_delay * (1 << (attempt - 1).min(3)); // Cap at 1.6s
                info!(
                    user = %position.user,
                    attempt = attempt,
                    delay_ms = delay.as_millis(),
                    "Retrying liquidation after delay"
                );
                tokio::time::sleep(delay).await;
            }

            match self.build_and_execute(position).await {
                Ok(result) => {
                    if attempt > 0 {
                        info!(
                            user = %position.user,
                            attempt = attempt,
                            "Liquidation succeeded on retry"
                        );
                    }
                    return Ok(result);
                }
                Err(e) => {
                    let error_str = e.to_string();

                    // Don't retry certain errors
                    if error_str.contains("not profitable")
                        || error_str.contains("not liquidatable")
                        || error_str.contains("bad debt")
                        || error_str.contains("No collateral")
                        || error_str.contains("No debt")
                    {
                        warn!(
                            user = %position.user,
                            error = %e,
                            "Liquidation failed with non-retryable error"
                        );
                        return Err(e);
                    }

                    warn!(
                        user = %position.user,
                        attempt = attempt,
                        error = %e,
                        "Liquidation attempt failed"
                    );
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("All retry attempts exhausted")))
    }

    /// Execute a staged liquidation with retry logic.
    ///
    /// Falls back to rebuilding from position if staged tx fails.
    #[instrument(skip(self, staged, position), fields(user = %staged.user))]
    pub async fn execute_staged_with_retry(
        &self,
        staged: StagedLiquidation,
        position: &TrackedPosition,
        max_retries: u32,
    ) -> Result<LiquidationResult> {
        // First try the staged tx
        match self.execute_staged(staged.clone()).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                let error_str = e.to_string();

                // Don't retry certain errors
                if error_str.contains("not profitable") {
                    return Err(e);
                }

                warn!(
                    user = %staged.user,
                    error = %e,
                    "Staged liquidation failed, falling back to rebuild"
                );
            }
        }

        // Fall back to build_and_execute with retries
        self.execute_with_retry(position, max_retries).await
    }
}

// Note: SwapAllocation is imported from hyperlend_chain above

/// Profit estimate breakdown for a liquidation.
#[derive(Debug, Clone)]
pub struct ProfitEstimate {
    /// Gross profit from liquidation bonus
    pub gross_profit: f64,
    /// Estimated gas cost in USD
    pub gas_cost: f64,
    /// Estimated slippage cost in USD
    pub slippage_cost: f64,
    /// Net profit after all costs
    pub net_profit: f64,
    /// Liquidation bonus percentage
    pub liquidation_bonus_pct: f64,
}

impl ProfitEstimate {
    /// Check if the liquidation is profitable given a minimum threshold.
    pub fn is_profitable(&self, min_profit: f64) -> bool {
        self.net_profit >= min_profit
    }

    /// Format as a human-readable string.
    pub fn to_string(&self) -> String {
        format!(
            "gross=${:.2} ({}% bonus) - gas=${:.2} - slippage=${:.2} = net=${:.2}",
            self.gross_profit,
            self.liquidation_bonus_pct,
            self.gas_cost,
            self.slippage_cost,
            self.net_profit
        )
    }
}

/// Result of a liquidation execution.
#[derive(Debug, Clone)]
pub struct LiquidationResult {
    pub user: Address,
    pub collateral_asset: Address,
    pub debt_asset: Address,
    pub debt_covered: U256,
    pub liquidation_tx: alloy::primitives::B256,
    pub rescue_tx: alloy::primitives::B256,
}

impl LiquidationResult {
    pub fn is_success(&self) -> bool {
        !self.liquidation_tx.is_zero() && !self.rescue_tx.is_zero()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slippage_calculation() {
        // 1% slippage on 1000
        let amount = U256::from(1000u64);
        let slippage_bps = 100u16; // 1%
        let result = u256_math::apply_basis_points(amount, slippage_bps);
        assert_eq!(result, U256::from(990u64));

        // 10% slippage on 1000
        let result = u256_math::apply_basis_points(amount, 1000);
        assert_eq!(result, U256::from(900u64));
    }

    #[test]
    fn test_close_factor() {
        // 50% close factor
        let total = U256::from(1000u64);
        let result = u256_math::apply_basis_points(total, 5000); // 50% = 5000 bps
        assert_eq!(result, U256::from(500u64));
    }
}
