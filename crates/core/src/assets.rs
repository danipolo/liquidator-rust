//! Asset registry for HyperLend protocol.
//!
//! Contains all 17 supported assets with their token addresses, oracle addresses,
//! oracle types, decimals, staleness thresholds, and liquidation priorities.

use alloy::primitives::{address, Address};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

/// Oracle types used across HyperLend assets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OracleType {
    /// Standard Chainlink-compatible aggregator
    Standard,
    /// RedStone price feeds
    RedStone,
    /// Pull-based Pyth oracle
    Pyth,
    /// 3-tier fallback oracle for LST assets (Primary → Secondary → Emergency)
    DualOracle,
    /// Pendle PT with maturity convergence pricing
    PendlePT,
}

/// Asset configuration.
#[derive(Debug, Clone)]
pub struct Asset {
    /// Asset symbol (e.g., "wHYPE", "USDC")
    pub symbol: &'static str,
    /// Token contract address
    pub token: Address,
    /// Oracle aggregator address
    pub oracle: Address,
    /// Oracle type for price fetching strategy
    pub oracle_type: OracleType,
    /// Token decimals
    pub decimals: u8,
    /// Expected staleness threshold for oracle updates
    pub staleness: Duration,
    /// Liquidation priority (higher = prefer as collateral to seize)
    pub priority: u8,
    /// Liquidation bonus in basis points (e.g., 500 = 5%)
    pub liquidation_bonus_bps: u16,
    /// Maturity date for Pendle PT assets (Unix timestamp)
    pub maturity: Option<u64>,
    /// Whether this asset is active (expired PTs are inactive)
    pub active: bool,
}

impl Asset {
    const fn new(
        symbol: &'static str,
        token: Address,
        oracle: Address,
        oracle_type: OracleType,
        decimals: u8,
        staleness_secs: u64,
        priority: u8,
        liquidation_bonus_bps: u16,
    ) -> Self {
        Self {
            symbol,
            token,
            oracle,
            oracle_type,
            decimals,
            staleness: Duration::from_secs(staleness_secs),
            priority,
            liquidation_bonus_bps,
            maturity: None,
            active: true,
        }
    }

    const fn pendle_pt(
        symbol: &'static str,
        token: Address,
        oracle: Address,
        decimals: u8,
        staleness_secs: u64,
        priority: u8,
        liquidation_bonus_bps: u16,
        maturity: u64,
        active: bool,
    ) -> Self {
        Self {
            symbol,
            token,
            oracle,
            oracle_type: OracleType::PendlePT,
            decimals,
            staleness: Duration::from_secs(staleness_secs),
            priority,
            liquidation_bonus_bps,
            maturity: Some(maturity),
            active,
        }
    }

    /// Get liquidation bonus as a decimal (e.g., 0.05 for 5%)
    pub const fn liquidation_bonus(&self) -> f64 {
        self.liquidation_bonus_bps as f64 / 10000.0
    }
}

// ============================================================================
// Asset Registry - All 17 HyperLend Assets
// ============================================================================

/// wHYPE - Wrapped HYPE (native token)
pub const WHYPE: Asset = Asset::new(
    "wHYPE",
    address!("5555555555555555555555555555555555555555"),
    address!("40ea33ea76fbe35e9fb422edd175b8c8d84a63cc"), // Correct oracle address
    OracleType::RedStone,
    18,
    3600, // ~1 hour
    30,
    500, // 5% liquidation bonus
);

/// USDT - Tether USD
pub const USDT: Asset = Asset::new(
    "USDT",
    address!("B8CE59FC3717ada4C02eaDF9682A266239C4ebb0"),
    address!("5d5ee47c6bcf6b05b2a3f65c4e37312dc978d30d"), // Correct oracle address
    OracleType::Standard,
    6,
    32400, // 9+ hours (anomalous)
    90,    // High priority due to stale oracle
    450,   // 4.5% liquidation bonus (stablecoin)
);

/// USDC - USD Coin
pub const USDC: Asset = Asset::new(
    "USDC",
    address!("0b88330c2d72e1b8a29a79e34a6f19a5af34c30f"),
    address!("4c7b17c8b4f3ff766889aaf2ac5a6db565fd61a9"), // Correct oracle address
    OracleType::Standard,
    6,
    3600, // ~1 hour
    30,
    450, // 4.5% liquidation bonus (stablecoin)
);

/// USDe - Ethena USDe
pub const USDE: Asset = Asset::new(
    "USDe",
    address!("5d3a1ff2b6bab83b63cd9ad0787074081a52ef34"),
    address!("6926d2c4f5aecd82192a9faf7b8e09a1d103bf23"), // Correct oracle address
    OracleType::Standard,
    18,
    3600, // ~1 hour
    30,
    500, // 5% liquidation bonus
);

/// USDHL - HyperLend native stablecoin (Pyth oracle)
pub const USDHL: Asset = Asset::new(
    "USDHL",
    address!("00b50A0000000000000000000000000000000005"),
    address!("a19b7fe6ffd492dd84adf38d37b974cb52f40267"), // Correct oracle address
    OracleType::Pyth,
    18,
    60, // ~1 minute (very fresh)
    50,
    450, // 4.5% liquidation bonus (stablecoin)
);

/// USR - Resolv USD
pub const USR: Asset = Asset::new(
    "USR",
    address!("00aD31b9C3bECDE4E8B6bAC8b6f2be3dAE4E5E77"),
    address!("29d2fec890b037b2d34f061f9a50f76f85ddbcae"), // Correct oracle address
    OracleType::RedStone,
    18,
    7200, // ~2 hours
    30,
    500, // 5% liquidation bonus
);

/// USDH - Hyperliquid USD
pub const USDH: Asset = Asset::new(
    "USDH",
    address!("1111111111111111111111111111111111111111"),
    address!("e18aad6733d1db21e19cb83b697082d3d4ee5170"), // Correct oracle address
    OracleType::RedStone,
    18,
    3600, // ~1 hour
    30,
    450, // 4.5% liquidation bonus (stablecoin)
);

// ============================================================================
// LST Assets (DualOracle - HIGH PRIORITY for tier transitions)
// ============================================================================

/// kHYPE - Kinetix staked HYPE
pub const KHYPE: Asset = Asset::new(
    "kHYPE",
    address!("0fD73e4dFCb3d4E97F6E2F6DBf8E3a5BE8dE096D"),
    address!("6dcfa746f7b11918ef3522c92e6429ca589c3875"), // Correct oracle address
    OracleType::DualOracle,
    18,
    1800, // 30 min primary tier
    80,   // High priority - DualOracle opportunities
    750,  // 7.5% liquidation bonus (LST)
);

/// wstHYPE - Wrapped staked HYPE
pub const WSTHYPE: Asset = Asset::new(
    "wstHYPE",
    address!("094e8E3dFCb3d4E97F6E2F6DBf8E3a5BE8dE0380"),
    address!("41c56e47a104e59e5cbf17b7813470bca72e94a7"), // Correct oracle address
    OracleType::DualOracle,
    18,
    1800, // 30 min primary tier
    80,   // High priority - DualOracle opportunities
    750,  // 7.5% liquidation bonus (LST)
);

/// beHYPE - Beefy staked HYPE
pub const BEHYPE: Asset = Asset::new(
    "beHYPE",
    address!("00d8FCE3dFCb3d4E97F6E2F6DBf8E3a5BE8dEA90"),
    address!("19067140d758a4addd2d19b5c678e25062a17754"), // Correct oracle address
    OracleType::DualOracle,
    18,
    1800, // 30 min
    60,
    750, // 7.5% liquidation bonus (LST)
);

// ============================================================================
// Synthetic Assets
// ============================================================================

/// UBTC - Universal BTC
pub const UBTC: Asset = Asset::new(
    "UBTC",
    address!("009FDBe3dFCb3d4E97F6E2F6DBf8E3a5BE8dE630"),
    address!("3587a73aa02519335a8a6053a97657bece0bc2cc"), // Correct oracle address
    OracleType::RedStone,
    8,
    5400, // ~1.5 hours
    50,
    650, // 6.5% liquidation bonus
);

/// UETH - Universal ETH
pub const UETH: Asset = Asset::new(
    "UETH",
    address!("00Be67e3dFCb3d4E97F6E2F6DBf8E3a5BE8dE070"),
    address!("4bad96dd1c7d541270a0c92e1d4e5f12eeea7a57"), // Correct oracle address
    OracleType::RedStone,
    18,
    5400, // ~1.5 hours
    50,
    650, // 6.5% liquidation bonus
);

/// USOL - Universal SOL
pub const USOL: Asset = Asset::new(
    "USOL",
    address!("0068fe3dFCb3d4E97F6E2F6DBf8E3a5BE8dE2900"),
    address!("b3d84379b1aabaf81b294ffdb5e68a31f7f7cce6"), // Correct oracle address
    OracleType::RedStone,
    9,
    5400, // ~1.5 hours
    50,
    700, // 7% liquidation bonus
);

// ============================================================================
// Staked Yield Assets
// ============================================================================

/// sUSDe - Staked USDe
pub const SUSDE: Asset = Asset::new(
    "sUSDe",
    address!("0211Ce3dFCb3d4E97F6E2F6DBf8E3a5BE8dEd200"),
    address!("243507c8c114618d7c8ad94b51118db7b4e32ece"), // Correct oracle address
    OracleType::RedStone,
    18,
    3600, // ~1 hour
    40,
    600, // 6% liquidation bonus
);

// ============================================================================
// Pendle PT Assets (maturity-aware pricing)
// ============================================================================

/// PT-kHYPE-19MAR2026 - Active Pendle PT
pub const PT_KHYPE_MAR2026: Asset = Asset::pendle_pt(
    "PT-kHYPE-19MAR2026",
    address!("0ea84e3dFCb3d4E97F6E2F6DBf8E3a5BE8dEf500"),
    address!("53d5bf9a6b4360d649653ba5d2af7c3d27602c7e"), // Correct oracle address
    18,
    3600,
    40,
    1000, // 10% liquidation bonus (higher risk PT)
    1742342400, // Mar 19, 2026 00:00:00 UTC
    true,
);

/// PT-kHYPE-13NOV2025 - Expired Pendle PT
pub const PT_KHYPE_NOV2025: Asset = Asset::pendle_pt(
    "PT-kHYPE-13NOV2025",
    address!("0311de3dFCb3d4E97F6E2F6DBf8E3a5BE8dE2900"),
    address!("a8a94da411425634e3ed6c331a32ab4fd774aa43"), // Correct oracle address
    18,
    3600,
    20,
    1000, // 10% liquidation bonus (higher risk PT)
    1731456000, // Nov 13, 2025 00:00:00 UTC
    false,      // Expired
);

/// PT-sUSDe-25SEP2025 - Expired Pendle PT
pub const PT_SUSDE_SEP2025: Asset = Asset::pendle_pt(
    "PT-sUSDe-25SEP2025",
    address!("0b737e3dFCb3d4E97F6E2F6DBf8E3a5BE8dE3500"),
    address!("8514d528275025ad9be6019e6e189dfef1db6c5e"), // Correct oracle address
    18,
    3600,
    20,
    1000, // 10% liquidation bonus (higher risk PT)
    1727222400, // Sep 25, 2025 00:00:00 UTC
    false,      // Expired
);

// ============================================================================
// Static Asset List
// ============================================================================

/// All 17 HyperLend assets.
pub static ASSETS: &[Asset] = &[
    // Standard/Stablecoins
    WHYPE,
    USDT,
    USDC,
    USDE,
    USDHL,
    USR,
    USDH,
    // LST Assets (DualOracle)
    KHYPE,
    WSTHYPE,
    BEHYPE,
    // Synthetic Assets
    UBTC,
    UETH,
    USOL,
    // Staked Yield
    SUSDE,
    // Pendle PT
    PT_KHYPE_MAR2026,
    PT_KHYPE_NOV2025,
    PT_SUSDE_SEP2025,
];

// ============================================================================
// Asset Registry (runtime lookup)
// ============================================================================

/// Asset registry for efficient lookups by token or oracle address.
pub struct AssetRegistry {
    by_token: HashMap<Address, &'static Asset>,
    by_oracle: HashMap<Address, &'static Asset>,
    by_symbol: HashMap<&'static str, &'static Asset>,
}

impl AssetRegistry {
    /// Create a new asset registry from the static asset list.
    pub fn new() -> Self {
        let mut by_token = HashMap::with_capacity(ASSETS.len());
        let mut by_oracle = HashMap::with_capacity(ASSETS.len());
        let mut by_symbol = HashMap::with_capacity(ASSETS.len());

        for asset in ASSETS {
            by_token.insert(asset.token, asset);
            by_oracle.insert(asset.oracle, asset);
            by_symbol.insert(asset.symbol, asset);
        }

        Self {
            by_token,
            by_oracle,
            by_symbol,
        }
    }

    /// Get asset by token address.
    pub fn get_by_token(&self, token: &Address) -> Option<&'static Asset> {
        self.by_token.get(token).copied()
    }

    /// Get asset by oracle address.
    pub fn get_by_oracle(&self, oracle: &Address) -> Option<&'static Asset> {
        self.by_oracle.get(oracle).copied()
    }

    /// Get asset by symbol.
    pub fn get_by_symbol(&self, symbol: &str) -> Option<&'static Asset> {
        self.by_symbol.get(symbol).copied()
    }

    /// Get all active assets.
    pub fn active_assets(&self) -> impl Iterator<Item = &'static Asset> {
        ASSETS.iter().filter(|a| a.active)
    }

    /// Get all DualOracle assets (LSTs).
    pub fn dual_oracle_assets(&self) -> impl Iterator<Item = &'static Asset> {
        ASSETS
            .iter()
            .filter(|a| a.oracle_type == OracleType::DualOracle)
    }

    /// Get all oracle addresses for WebSocket subscription.
    pub fn oracle_addresses(&self) -> Vec<Address> {
        ASSETS
            .iter()
            .filter(|a| a.active)
            .map(|a| a.oracle)
            .collect()
    }

    /// Get assets sorted by liquidation priority (descending).
    pub fn by_priority(&self) -> Vec<&'static Asset> {
        let mut assets: Vec<_> = ASSETS.iter().filter(|a| a.active).collect();
        assets.sort_by(|a, b| b.priority.cmp(&a.priority));
        assets
    }

    /// Get liquidation bonus for a token (returns default 5% if not found).
    pub fn get_liquidation_bonus(&self, token: &Address) -> f64 {
        self.get_by_token(token)
            .map(|a| a.liquidation_bonus())
            .unwrap_or(0.05) // Default 5%
    }

    /// Get liquidation bonus in basis points for a token.
    pub fn get_liquidation_bonus_bps(&self, token: &Address) -> u16 {
        self.get_by_token(token)
            .map(|a| a.liquidation_bonus_bps)
            .unwrap_or(500) // Default 5%
    }
}

impl Default for AssetRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global asset registry instance.
pub static REGISTRY: LazyLock<AssetRegistry> = LazyLock::new(AssetRegistry::new);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asset_count() {
        assert_eq!(ASSETS.len(), 17);
    }

    #[test]
    fn test_registry_lookup() {
        let registry = AssetRegistry::new();

        // Test by token
        let whype = registry.get_by_token(&WHYPE.token);
        assert!(whype.is_some());
        assert_eq!(whype.unwrap().symbol, "wHYPE");

        // Test by symbol
        let usdt = registry.get_by_symbol("USDT");
        assert!(usdt.is_some());
        assert_eq!(usdt.unwrap().decimals, 6);
    }

    #[test]
    fn test_dual_oracle_assets() {
        let registry = AssetRegistry::new();
        let dual_oracles: Vec<_> = registry.dual_oracle_assets().collect();
        assert_eq!(dual_oracles.len(), 3); // kHYPE, wstHYPE, beHYPE
    }

    #[test]
    fn test_active_assets() {
        let registry = AssetRegistry::new();
        let active: Vec<_> = registry.active_assets().collect();
        // 17 total - 2 expired PTs = 15 active
        assert_eq!(active.len(), 15);
    }

    #[test]
    fn test_priority_ordering() {
        let registry = AssetRegistry::new();
        let by_priority = registry.by_priority();
        // USDT should be first (priority 90)
        assert_eq!(by_priority[0].symbol, "USDT");
    }
}
