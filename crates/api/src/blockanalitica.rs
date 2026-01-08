//! BlockAnalitica API client for at-risk wallet discovery.

use alloy::primitives::Address;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, instrument};

/// Profitability filter for at-risk wallets.
///
/// Calculates minimum position sizes based on liquidation economics:
/// - Liquidation bonus (typically 5-10%)
/// - Close factor (50% of position)
/// - Gas costs
/// - Slippage
/// - Minimum profit threshold
#[derive(Debug, Clone)]
pub struct ProfitabilityFilter {
    /// Minimum profit required (USD)
    pub min_profit_usd: f64,
    /// Estimated gas cost (USD)
    pub gas_cost_usd: f64,
    /// Expected slippage (decimal, e.g., 0.01 = 1%)
    pub slippage: f64,
    /// Close factor (decimal, e.g., 0.5 = 50%)
    pub close_factor: f64,
    /// Default liquidation bonus (decimal, e.g., 0.05 = 5%)
    pub default_bonus: f64,
}

impl Default for ProfitabilityFilter {
    fn default() -> Self {
        // DUST TEST: Allow any position to test execution
        // Restore to production after testing:
        // min_profit_usd: 0.25 -> $14 min collateral
        Self {
            min_profit_usd: -1.0,  // DUST TEST: Accept losses to test execution
            gas_cost_usd: 0.03,
            slippage: 0.01,
            close_factor: 0.5,
            default_bonus: 0.05,
        }
    }
}

impl ProfitabilityFilter {
    /// Create a new profitability filter with custom settings.
    pub fn new(min_profit_usd: f64, gas_cost_usd: f64, slippage: f64) -> Self {
        Self {
            min_profit_usd,
            gas_cost_usd,
            slippage,
            ..Default::default()
        }
    }

    /// Calculate minimum collateral required for profitability.
    ///
    /// Formula: Collateral × close_factor × (bonus - slippage) - gas >= min_profit
    /// Solving for Collateral:
    /// Collateral >= (min_profit + gas) / (close_factor × (bonus - slippage))
    pub fn min_collateral_usd(&self) -> f64 {
        let effective_bonus = (self.default_bonus - self.slippage).max(0.001);
        (self.min_profit_usd + self.gas_cost_usd) / (self.close_factor * effective_bonus)
    }

    /// Calculate minimum collateral with a specific liquidation bonus.
    pub fn min_collateral_with_bonus(&self, bonus: f64) -> f64 {
        let effective_bonus = (bonus - self.slippage).max(0.001);
        (self.min_profit_usd + self.gas_cost_usd) / (self.close_factor * effective_bonus)
    }

    /// Check if a wallet is potentially profitable to liquidate.
    pub fn is_profitable(&self, total_supply_usd: f64, total_borrow_usd: f64) -> bool {
        // Position must have enough collateral
        let min_collateral = self.min_collateral_usd();

        // Check both supply and borrow are above minimum
        // DUST TEST: Accept any position with non-zero debt
        total_supply_usd >= min_collateral && total_borrow_usd >= 0.0001
    }

    /// Get the reason why a position is filtered out.
    pub fn filter_reason(&self, total_supply_usd: f64, total_borrow_usd: f64) -> Option<String> {
        let min_collateral = self.min_collateral_usd();

        if total_supply_usd < min_collateral {
            Some(format!(
                "collateral ${:.2} < minimum ${:.2}",
                total_supply_usd, min_collateral
            ))
        } else if total_borrow_usd < 0.0001 {
            Some(format!("debt ${:.6} < minimum $0.0001", total_borrow_usd))
        } else {
            None
        }
    }
}

/// BlockAnalitica API client.
#[derive(Debug, Clone)]
pub struct BlockAnaliticaClient {
    client: reqwest::Client,
    base_url: String,
    /// Profitability filter for excluding dust positions
    profitability_filter: ProfitabilityFilter,
}

impl BlockAnaliticaClient {
    /// Create a new BlockAnalitica client.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: "https://hyperlend-api.blockanalitica.com".to_string(),
            profitability_filter: ProfitabilityFilter::default(),
        }
    }

    /// Create a client with custom base URL.
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
            profitability_filter: ProfitabilityFilter::default(),
        }
    }

    /// Set the profitability filter.
    pub fn with_profitability_filter(mut self, filter: ProfitabilityFilter) -> Self {
        self.profitability_filter = filter;
        self
    }

    /// Get reference to the profitability filter.
    pub fn profitability_filter(&self) -> &ProfitabilityFilter {
        &self.profitability_filter
    }

    /// Fetch at-risk wallets (bad debt wallets) with profitability filtering.
    #[instrument(skip(self))]
    pub async fn fetch_at_risk_wallets(
        &self,
        hf_max: f64,
        limit: usize,
    ) -> Result<Vec<AtRiskWallet>> {
        let url = format!("{}/wallets/bad-debt-wallets/", self.base_url);

        // Request more than needed since we'll filter
        let request_limit = (limit * 3).min(500);

        let response = self
            .client
            .get(&url)
            .query(&[
                ("network", "hyper"),
                ("order", "-total_supply_usd"),
                ("p", "1"),
                ("p_size", &request_limit.to_string()),
            ])
            .send()
            .await?;

        let data: WalletsResponse = response.json().await?;
        let total_fetched = data.results.len();
        let total_available = data.count;

        // Filter by health factor and profitability
        let min_collateral = self.profitability_filter.min_collateral_usd();
        let mut filtered_count = 0;

        let wallets: Vec<AtRiskWallet> = data
            .results
            .into_iter()
            .filter(|w| {
                // Health factor filter
                if w.health_rate > hf_max {
                    return false;
                }

                // Profitability filter
                if !self
                    .profitability_filter
                    .is_profitable(w.total_supply, w.total_borrow)
                {
                    filtered_count += 1;
                    return false;
                }

                true
            })
            .take(limit)
            .collect();

        info!(
            total_available = total_available,
            total_fetched = total_fetched,
            profitable = wallets.len(),
            filtered_dust = filtered_count,
            min_collateral_usd = format!("${:.2}", min_collateral),
            "Fetched bad-debt wallets (filtered by profitability)"
        );

        Ok(wallets)
    }

    /// Fetch ALL bad-debt wallets with pagination (for analysis).
    #[instrument(skip(self))]
    pub async fn fetch_all_bad_debt_wallets(&self, hf_max: f64) -> Result<Vec<AtRiskWallet>> {
        let url = format!("{}/wallets/bad-debt-wallets/", self.base_url);
        let page_size = 500;
        let mut all_wallets = Vec::new();
        let mut page = 1;
        let mut total_available = 0;

        loop {
            let response = self
                .client
                .get(&url)
                .query(&[
                    ("network", "hyper"),
                    ("order", "-total_supply_usd"),
                    ("p", &page.to_string()),
                    ("p_size", &page_size.to_string()),
                ])
                .send()
                .await?;

            let data: WalletsResponse = response.json().await?;
            total_available = data.count;

            let page_wallets: Vec<AtRiskWallet> = data
                .results
                .into_iter()
                .filter(|w| w.health_rate <= hf_max)
                .collect();

            let fetched_count = page_wallets.len();
            all_wallets.extend(page_wallets);

            debug!(
                page = page,
                fetched = fetched_count,
                total_so_far = all_wallets.len(),
                "Fetched bad-debt page"
            );

            // Check if there are more pages
            if all_wallets.len() >= total_available as usize || fetched_count < page_size {
                break;
            }

            page += 1;
        }

        let min_collateral = self.profitability_filter.min_collateral_usd();
        let profitable_count = all_wallets
            .iter()
            .filter(|w| self.profitability_filter.is_profitable(w.total_supply, w.total_borrow))
            .count();

        info!(
            total_available = total_available,
            total_fetched = all_wallets.len(),
            profitable = profitable_count,
            min_collateral_usd = format!("${:.2}", min_collateral),
            "Fetched ALL bad-debt wallets"
        );

        Ok(all_wallets)
    }

    /// Fetch ALL at-risk wallets (HF 1.0-1.25) with pagination.
    #[instrument(skip(self))]
    pub async fn fetch_all_approaching_liquidation(
        &self,
        hf_min: f64,
        hf_max: f64,
    ) -> Result<Vec<AtRiskWallet>> {
        let url = format!("{}/wallets/at-risk/", self.base_url);
        let page_size = 500;
        let mut all_wallets = Vec::new();
        let mut page = 1;
        let mut total_available = 0;

        loop {
            let response = self
                .client
                .get(&url)
                .query(&[
                    ("network", "hyper"),
                    ("health_rate_min", &hf_min.to_string()),
                    ("health_rate_max", &hf_max.to_string()),
                    ("order", "health_rate"),
                    ("p", &page.to_string()),
                    ("p_size", &page_size.to_string()),
                ])
                .send()
                .await?;

            let data: WalletsResponse = response.json().await?;
            total_available = data.count;

            let fetched_count = data.results.len();
            all_wallets.extend(data.results);

            debug!(
                page = page,
                fetched = fetched_count,
                total_so_far = all_wallets.len(),
                "Fetched at-risk page"
            );

            // Check if there are more pages
            if all_wallets.len() >= total_available as usize || fetched_count < page_size {
                break;
            }

            page += 1;
        }

        let min_collateral = self.profitability_filter.min_collateral_usd();
        let profitable_count = all_wallets
            .iter()
            .filter(|w| self.profitability_filter.is_profitable(w.total_supply, w.total_borrow))
            .count();

        info!(
            total_available = total_available,
            total_fetched = all_wallets.len(),
            profitable = profitable_count,
            min_collateral_usd = format!("${:.2}", min_collateral),
            "Fetched ALL at-risk wallets (approaching liquidation)"
        );

        Ok(all_wallets)
    }

    /// Get statistics about available wallets from both endpoints.
    pub async fn get_wallet_stats(&self) -> Result<WalletStats> {
        // Fetch from bad-debt endpoint
        let bad_debt_url = format!("{}/wallets/bad-debt-wallets/", self.base_url);

        let bad_debt_resp = self
            .client
            .get(&bad_debt_url)
            .query(&[("network", "hyper"), ("p_size", "1")])
            .send()
            .await?;

        let bad_debt_data: WalletsResponse = bad_debt_resp.json().await?;

        // Try the at-risk endpoint (may fail or return different format)
        let at_risk_count = match self.fetch_at_risk_count().await {
            Ok(count) => count,
            Err(_) => 0, // Endpoint may not exist or have different format
        };

        let stats = WalletStats {
            bad_debt_total: bad_debt_data.count,
            at_risk_total: at_risk_count,
            min_collateral_threshold: self.profitability_filter.min_collateral_usd(),
        };

        info!(
            bad_debt_wallets = stats.bad_debt_total,
            at_risk_wallets = stats.at_risk_total,
            min_collateral_usd = format!("${:.2}", stats.min_collateral_threshold),
            "BlockAnalitica wallet stats"
        );

        Ok(stats)
    }

    /// Try to get count from at-risk endpoint.
    async fn fetch_at_risk_count(&self) -> Result<u32> {
        let url = format!("{}/wallets/at-risk/", self.base_url);

        let resp = self
            .client
            .get(&url)
            .query(&[
                ("network", "hyper"),
                ("health_rate_min", "1.0"),
                ("health_rate_max", "1.25"),
                ("p_size", "1"),
            ])
            .send()
            .await?;

        let data: WalletsResponse = resp.json().await?;
        Ok(data.count)
    }

    /// Analyze position size distribution from all bad-debt wallets.
    pub async fn analyze_position_distribution(&self) -> Result<PositionDistribution> {
        let wallets = self.fetch_all_bad_debt_wallets(10.0).await?; // HF < 10 = all

        let mut distribution = PositionDistribution::default();
        for wallet in &wallets {
            distribution.add(wallet.total_supply);
        }

        distribution.log();
        Ok(distribution)
    }

    /// Fetch wallets approaching liquidation with profitability filtering.
    #[instrument(skip(self))]
    pub async fn fetch_wallets_at_risk(
        &self,
        hf_min: f64,
        hf_max: f64,
        limit: usize,
    ) -> Result<Vec<AtRiskWallet>> {
        let url = format!("{}/wallets/at-risk/", self.base_url);

        // Request more than needed since we'll filter
        let request_limit = (limit * 3).min(500);

        let response = self
            .client
            .get(&url)
            .query(&[
                ("network", "hyper"),
                ("health_rate_min", &hf_min.to_string()),
                ("health_rate_max", &hf_max.to_string()),
                ("order", "health_rate"),
                ("p", "1"),
                ("p_size", &request_limit.to_string()),
            ])
            .send()
            .await?;

        let data: WalletsResponse = response.json().await?;
        let total_fetched = data.results.len();

        // Filter by profitability
        let min_collateral = self.profitability_filter.min_collateral_usd();
        let mut filtered_count = 0;

        let wallets: Vec<AtRiskWallet> = data
            .results
            .into_iter()
            .filter(|w| {
                if !self
                    .profitability_filter
                    .is_profitable(w.total_supply, w.total_borrow)
                {
                    filtered_count += 1;
                    return false;
                }
                true
            })
            .take(limit)
            .collect();

        info!(
            total_fetched = total_fetched,
            profitable = wallets.len(),
            filtered_dust = filtered_count,
            min_collateral_usd = format!("${:.2}", min_collateral),
            "Fetched at-risk wallets (filtered by profitability)"
        );

        Ok(wallets)
    }

    /// Fetch at-risk wallets WITHOUT profitability filtering (for analysis).
    #[instrument(skip(self))]
    pub async fn fetch_at_risk_wallets_unfiltered(
        &self,
        hf_max: f64,
        limit: usize,
    ) -> Result<Vec<AtRiskWallet>> {
        let url = format!("{}/wallets/bad-debt-wallets/", self.base_url);

        let response = self
            .client
            .get(&url)
            .query(&[
                ("network", "hyper"),
                ("order", "-total_supply_usd"),
                ("p", "1"),
                ("p_size", &limit.to_string()),
            ])
            .send()
            .await?;

        let data: WalletsResponse = response.json().await?;

        let wallets: Vec<AtRiskWallet> = data
            .results
            .into_iter()
            .filter(|w| w.health_rate <= hf_max)
            .collect();

        debug!(count = wallets.len(), "Fetched at-risk wallets (unfiltered)");

        Ok(wallets)
    }

    /// Fetch details for a specific wallet.
    #[instrument(skip(self), fields(address = %address))]
    pub async fn fetch_wallet_details(&self, address: Address) -> Result<WalletDetails> {
        let url = format!("{}/wallets/{}/", self.base_url, address);

        let response = self
            .client
            .get(&url)
            .query(&[("network", "hyper")])
            .send()
            .await?;

        let details: WalletDetails = response.json().await?;

        Ok(details)
    }

    /// Fetch market data.
    #[instrument(skip(self))]
    pub async fn fetch_markets(&self) -> Result<Vec<MarketData>> {
        let url = format!("{}/markets/", self.base_url);

        let response = self
            .client
            .get(&url)
            .query(&[("network", "hyper")])
            .send()
            .await?;

        let data: MarketsResponse = response.json().await?;

        Ok(data.results)
    }
}

impl Default for BlockAnaliticaClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Response wrapper for paginated wallet lists.
#[derive(Debug, Deserialize)]
pub struct WalletsResponse {
    pub count: u32,
    pub next: Option<String>,
    pub previous: Option<String>,
    pub results: Vec<AtRiskWallet>,
}

/// Statistics about available wallets from BlockAnalitica.
#[derive(Debug, Clone)]
pub struct WalletStats {
    /// Total bad-debt wallets (HF < 1.0)
    pub bad_debt_total: u32,
    /// Total at-risk wallets (HF 1.0-1.25)
    pub at_risk_total: u32,
    /// Minimum collateral threshold for profitability
    pub min_collateral_threshold: f64,
}

/// Position size distribution analysis.
#[derive(Debug, Clone, Default)]
pub struct PositionDistribution {
    /// Positions under $1
    pub under_1: u32,
    /// Positions $1-$10
    pub from_1_to_10: u32,
    /// Positions $10-$50
    pub from_10_to_50: u32,
    /// Positions $50-$100
    pub from_50_to_100: u32,
    /// Positions $100-$500
    pub from_100_to_500: u32,
    /// Positions $500-$1000
    pub from_500_to_1000: u32,
    /// Positions over $1000
    pub over_1000: u32,
    /// Largest position value
    pub largest_position: f64,
    /// Total positions analyzed
    pub total: u32,
}

impl PositionDistribution {
    /// Add a position to the distribution.
    pub fn add(&mut self, collateral_usd: f64) {
        self.total += 1;
        if collateral_usd > self.largest_position {
            self.largest_position = collateral_usd;
        }

        if collateral_usd < 1.0 {
            self.under_1 += 1;
        } else if collateral_usd < 10.0 {
            self.from_1_to_10 += 1;
        } else if collateral_usd < 50.0 {
            self.from_10_to_50 += 1;
        } else if collateral_usd < 100.0 {
            self.from_50_to_100 += 1;
        } else if collateral_usd < 500.0 {
            self.from_100_to_500 += 1;
        } else if collateral_usd < 1000.0 {
            self.from_500_to_1000 += 1;
        } else {
            self.over_1000 += 1;
        }
    }

    /// Log the distribution.
    pub fn log(&self) {
        info!(
            total = self.total,
            under_1 = self.under_1,
            from_1_to_10 = self.from_1_to_10,
            from_10_to_50 = self.from_10_to_50,
            from_50_to_100 = self.from_50_to_100,
            from_100_to_500 = self.from_100_to_500,
            from_500_to_1000 = self.from_500_to_1000,
            over_1000 = self.over_1000,
            largest = format!("${:.2}", self.largest_position),
            "Position size distribution"
        );
    }
}

/// At-risk wallet data from BlockAnalitica.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AtRiskWallet {
    /// Wallet address as string
    pub wallet_address: String,

    /// Total supply in USD (string in API response)
    #[serde(deserialize_with = "deserialize_f64_from_string")]
    pub total_supply: f64,

    /// Total supply change
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub total_supply_change: Option<f64>,

    /// Total borrow in USD (string in API response)
    #[serde(deserialize_with = "deserialize_f64_from_string")]
    pub total_borrow: f64,

    /// Total borrow change
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub total_borrow_change: Option<f64>,

    /// Net position
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub net: Option<f64>,

    /// Health rate (health factor)
    #[serde(deserialize_with = "deserialize_f64_from_string")]
    pub health_rate: f64,

    /// Health rate change
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub health_rate_change: Option<f64>,

    /// Supplied assets
    #[serde(default)]
    pub supplied_assets: Vec<WalletAsset>,

    /// Borrowed assets
    #[serde(default)]
    pub borrowed_assets: Vec<WalletAsset>,

    /// E-mode category
    #[serde(default)]
    pub emode_category: u8,

    /// Last activity timestamp
    #[serde(default)]
    pub last_activity: Option<String>,
}

impl AtRiskWallet {
    /// Parse wallet address from string.
    pub fn address(&self) -> Option<Address> {
        self.wallet_address.parse().ok()
    }

    /// Alias for total_supply (backward compatibility).
    pub fn total_supply_usd(&self) -> f64 {
        self.total_supply
    }

    /// Alias for total_borrow (backward compatibility).
    pub fn total_borrow_usd(&self) -> f64 {
        self.total_borrow
    }

    /// Check if this wallet is potentially profitable to liquidate.
    pub fn is_potentially_profitable(&self, filter: &ProfitabilityFilter) -> bool {
        filter.is_profitable(self.total_supply, self.total_borrow)
    }

    /// Check if this wallet is dust (very small position).
    pub fn is_dust(&self, min_collateral_usd: f64) -> bool {
        self.total_supply < min_collateral_usd || self.total_borrow < 1.0
    }
}

/// Asset in a wallet position.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WalletAsset {
    /// Asset symbol
    pub symbol: String,

    /// Asset address
    #[serde(deserialize_with = "deserialize_address")]
    pub address: Address,

    /// Amount (may be string or number)
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub amount: Option<f64>,

    /// USD value
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub usd_value: Option<f64>,
}

/// Detailed wallet information.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WalletDetails {
    /// Wallet address
    #[serde(deserialize_with = "deserialize_address")]
    pub address: Address,

    /// Health factor
    #[serde(deserialize_with = "deserialize_f64_from_string")]
    pub health_rate: f64,

    /// Total supply
    #[serde(deserialize_with = "deserialize_f64_from_string")]
    pub total_supply_usd: f64,

    /// Total borrow
    #[serde(deserialize_with = "deserialize_f64_from_string")]
    pub total_borrow_usd: f64,

    /// Supply positions
    #[serde(default)]
    pub supplies: Vec<PositionDetail>,

    /// Borrow positions
    #[serde(default)]
    pub borrows: Vec<PositionDetail>,
}

/// Detailed position data.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PositionDetail {
    /// Asset symbol
    pub symbol: String,

    /// Asset address
    #[serde(deserialize_with = "deserialize_address")]
    pub address: Address,

    /// Amount
    #[serde(deserialize_with = "deserialize_f64_from_string")]
    pub amount: f64,

    /// USD value
    #[serde(deserialize_with = "deserialize_f64_from_string")]
    pub usd_value: f64,

    /// Asset price
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub price: Option<f64>,
}

/// Market data response.
#[derive(Debug, Deserialize)]
pub struct MarketsResponse {
    pub results: Vec<MarketData>,
}

/// Market data for an asset.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MarketData {
    /// Asset symbol
    pub symbol: String,

    /// Asset address
    #[serde(deserialize_with = "deserialize_address")]
    pub address: Address,

    /// Total supply
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub total_supply: Option<f64>,

    /// Total borrow
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub total_borrow: Option<f64>,

    /// Supply APY
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub supply_apy: Option<f64>,

    /// Borrow APY
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub borrow_apy: Option<f64>,

    /// Utilization rate
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub utilization_rate: Option<f64>,
}

// Custom deserializers

fn deserialize_address<'de, D>(deserializer: D) -> Result<Address, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    s.parse().map_err(serde::de::Error::custom)
}

fn deserialize_f64_from_string<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrNumber {
        String(String),
        Number(f64),
    }

    match StringOrNumber::deserialize(deserializer)? {
        StringOrNumber::String(s) => s.parse().map_err(serde::de::Error::custom),
        StringOrNumber::Number(n) => Ok(n),
    }
}

fn deserialize_optional_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrNumber {
        String(String),
        Number(f64),
        Null,
    }

    match Option::<StringOrNumber>::deserialize(deserializer)? {
        Some(StringOrNumber::String(s)) => {
            if s.is_empty() {
                Ok(None)
            } else {
                s.parse().map(Some).map_err(serde::de::Error::custom)
            }
        }
        Some(StringOrNumber::Number(n)) => Ok(Some(n)),
        Some(StringOrNumber::Null) | None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profitability_filter() {
        let filter = ProfitabilityFilter::default();

        // Production: min_profit=$0.25, gas=$0.03, slippage=1%, bonus=5%
        // Effective bonus = 5% - 1% = 4%
        // Min collateral = ($0.25 + $0.03) / (0.5 × 0.04) = $0.28 / 0.02 = $14.00
        let min_collateral = filter.min_collateral_usd();
        assert!(
            (min_collateral - 14.0).abs() < 0.5,
            "Expected ~$14 min collateral, got ${:.2}",
            min_collateral
        );

        // $100 collateral, $50 debt should be profitable
        assert!(filter.is_profitable(100.0, 50.0));

        // $20 collateral, $10 debt should be profitable (above $14 threshold)
        assert!(filter.is_profitable(20.0, 10.0));

        // $10 collateral (below threshold) should NOT be profitable
        assert!(!filter.is_profitable(10.0, 5.0));

        // $100 collateral, $0.50 debt should NOT be profitable (below $1 min debt)
        assert!(!filter.is_profitable(100.0, 0.50));
    }

    #[test]
    fn test_profitability_with_higher_bonus() {
        let filter = ProfitabilityFilter::default();

        // With 7.5% bonus (LST assets)
        // Effective bonus = 7.5% - 1% = 6.5%
        // Min collateral = $0.28 / (0.5 × 0.065) = ~$8.62
        let min_collateral = filter.min_collateral_with_bonus(0.075);
        assert!(
            min_collateral < 10.0 && min_collateral > 8.0,
            "Expected ~$8.62 min collateral for 7.5% bonus, got ${:.2}",
            min_collateral
        );
    }

    #[test]
    fn test_deserialize_wallet() {
        // Based on actual API response format from original bot
        let json = r#"{
            "wallet_address": "0x0af3318c4060eac02d50e140de2fb0e492b59ecb",
            "total_supply": "2352.37022441269234673",
            "total_supply_change": "30.697184056199319558",
            "total_borrow": "948.8563641597621272",
            "total_borrow_change": "83.969150420123719364",
            "net": "1403.51386025300321491",
            "health_rate": "0.991665",
            "health_rate_change": "-0.082081",
            "emode_category": 1,
            "last_activity": "2025-05-14T16:26:04Z",
            "supplied_assets": [
                {
                    "symbol": "UBTC",
                    "address": "0x9FDBdA0A5e284c32744D2f17Ee5c74B284993463"
                }
            ],
            "borrowed_assets": [
                {
                    "symbol": "WHYPE",
                    "address": "0x5555555555555555555555555555555555555555"
                }
            ]
        }"#;

        let wallet: AtRiskWallet = serde_json::from_str(json).unwrap();
        assert!((wallet.total_supply - 2352.37).abs() < 0.01);
        assert!((wallet.health_rate - 0.991665).abs() < 0.0001);
        assert_eq!(wallet.supplied_assets.len(), 1);
        assert_eq!(wallet.borrowed_assets.len(), 1);
    }
}
