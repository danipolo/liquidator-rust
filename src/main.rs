//! HyperLend Liquidation Bot
//!
//! High-performance liquidation bot for HyperLend (Aave V3 fork) on HyperLiquid EVM.
//! Features:
//! - Event-driven architecture via WebSocket subscriptions
//! - Tiered position tracking (Critical/Hot/Warm/Cold)
//! - Pre-staged transactions for sub-100ms latency
//! - DualOracle monitoring for LST arbitrage opportunities

use std::sync::Arc;

use anyhow::Result;
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use hyperlend_api::{BlockAnaliticaClient, LiqdClient};
use hyperlend_chain::{
    DualOracleMonitor, EventListener, LiquidatorContract, OracleMonitor, OracleType,
    ProviderManager, TransactionSender,
};
use hyperlend_core::{
    AssetRegistry, BotConfig, HeartbeatPredictor, Liquidator, PreStager, Scanner, ScannerConfig,
    TieredPositionTracker, ASSETS, init_config,
};

/// Environment variable names.
mod env {
    pub const ALCHEMY_WS_URL: &str = "ALCHEMY_WS_URL";
    pub const ALCHEMY_HTTP_URL: &str = "ALCHEMY_HTTP_URL";
    pub const ARCHIVE_RPC: &str = "ARCHIVE_RPC";
    pub const SEND_RPC: &str = "SEND_RPC";
    pub const PRIVATE_KEY: &str = "PRIVATE_KEY";
    pub const PROFIT_RECEIVER: &str = "PROFIT_RECEIVER";
    pub const POOL: &str = "POOL";
    pub const BALANCES_READER: &str = "BALANCES_READER";
    pub const LIQUIDATOR: &str = "LIQUIDATOR";
}

#[tokio::main]
async fn main() -> Result<()> {
    // Print startup banner
    print_banner();

    // Load environment variables
    dotenvy::dotenv().ok();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,hyperlend_core=debug,hyperlend_chain=debug")),
        )
        .init();

    // Load and initialize bot config (MUST be done before any core module usage)
    // Use BOT_PROFILE env var to select: testing, production, aggressive, or file path
    let bot_config = BotConfig::from_env();
    bot_config.log_config();
    init_config(bot_config);

    info!("Starting HyperLend Liquidation Bot");
    info!("Chain: HyperLiquid EVM (999)");
    info!("Block time: 200ms");

    // Load RPC/contract configuration
    let config = load_config()?;

    // Initialize components
    let (scanner, _handles) = initialize_components(config).await?;

    // Bootstrap
    info!("Bootstrapping...");
    scanner.bootstrap().await?;

    // Run main loop
    info!("Starting main event loop...");
    scanner.run().await?;

    Ok(())
}

/// Configuration loaded from environment.
struct Config {
    ws_url: String,
    http_url: String,
    archive_url: String,
    send_url: String,
    pool: alloy::primitives::Address,
    balances_reader: alloy::primitives::Address,
    liquidator_contract: alloy::primitives::Address,
    profit_receiver: alloy::primitives::Address,
    private_key: String,
}

fn load_config() -> Result<Config> {
    let get_env = |name: &str| -> Result<String> {
        std::env::var(name).map_err(|_| anyhow::anyhow!("Missing env var: {}", name))
    };

    let get_address = |name: &str| -> Result<alloy::primitives::Address> {
        get_env(name)?
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid address for {}: {}", name, e))
    };

    Ok(Config {
        ws_url: get_env(env::ALCHEMY_WS_URL)?,
        http_url: get_env(env::ALCHEMY_HTTP_URL)
            .unwrap_or_else(|_| "https://rpc.hyperlend.finance".to_string()),
        archive_url: get_env(env::ARCHIVE_RPC)
            .unwrap_or_else(|_| "https://rpc.hyperlend.finance/archive".to_string()),
        send_url: get_env(env::SEND_RPC)
            .unwrap_or_else(|_| "https://rpc.hyperliquid.xyz/evm".to_string()),
        pool: get_address(env::POOL)
            .unwrap_or_else(|_| "0x00A89d7a5A02160f20150EbEA7a2b5E4879A1A8b".parse().unwrap()),
        balances_reader: get_address(env::BALANCES_READER)
            .unwrap_or_else(|_| "0xE17ea42a8d61e50a26bec1829399071d2129845b".parse().unwrap()),
        liquidator_contract: get_address(env::LIQUIDATOR)?,
        profit_receiver: get_address(env::PROFIT_RECEIVER)?,
        private_key: get_env(env::PRIVATE_KEY)?,
    })
}

async fn initialize_components(config: Config) -> Result<(Scanner, Vec<tokio::task::JoinHandle<()>>)> {
    info!("Initializing components...");

    // Provider manager
    let provider = Arc::new(
        ProviderManager::new(
            &config.http_url,
            &config.archive_url,
            &config.send_url,
            &config.ws_url,
            config.pool,
            config.balances_reader,
        )
        .await?,
    );

    info!(
        pool = %config.pool,
        balances_reader = %config.balances_reader,
        "Provider initialized"
    );

    // Asset registry
    let assets = Arc::new(AssetRegistry::new());
    info!(asset_count = ASSETS.len(), "Asset registry loaded");

    // Build oracle configs for event listener
    let oracle_configs: Vec<_> = ASSETS
        .iter()
        .filter(|a| a.active)
        .map(|a| {
            let oracle_type = match a.oracle_type {
                hyperlend_core::OracleType::Standard => OracleType::Standard,
                hyperlend_core::OracleType::RedStone => OracleType::RedStone,
                hyperlend_core::OracleType::Pyth => OracleType::Pyth,
                hyperlend_core::OracleType::DualOracle => OracleType::DualOracle,
                hyperlend_core::OracleType::PendlePT => OracleType::PendlePT,
            };
            (a.oracle, a.token, oracle_type)
        })
        .collect();

    // Event listener
    let event_listener = Arc::new(EventListener::new(
        &config.ws_url,
        config.pool,
        oracle_configs,
    ));
    info!("Event listener configured");

    // Position tracker
    let tracker = Arc::new(TieredPositionTracker::new());

    // Oracle monitor
    let oracle_monitor = Arc::new(OracleMonitor::new(provider.clone()));

    // Register oracle-asset mappings
    for asset in ASSETS.iter().filter(|a| a.active) {
        oracle_monitor.register_oracle(asset.oracle, asset.token);
    }

    // DualOracle monitor (LST assets)
    let dual_oracle_addrs: Vec<_> = assets
        .dual_oracle_assets()
        .map(|a| a.oracle)
        .collect();
    let dual_oracle_monitor = Arc::new(DualOracleMonitor::new(dual_oracle_addrs));
    info!(
        lst_count = assets.dual_oracle_assets().count(),
        "DualOracle monitor initialized"
    );

    // Heartbeat predictor
    let heartbeat_predictor = Arc::new(HeartbeatPredictor::new());

    // Pre-stager
    let pre_stager = Arc::new(PreStager::new());

    // API clients
    let blockanalitica = Arc::new(BlockAnaliticaClient::new());
    let liqd_client = Arc::new(LiqdClient::new());

    // Transaction sender (for signing and sending liquidation transactions)
    let tx_sender = Arc::new(TransactionSender::new(
        &config.private_key,
        &config.send_url,
        999, // HyperLiquid EVM chain ID
    ).await?);
    info!(
        address = %tx_sender.address,
        "Transaction sender initialized"
    );

    // Liquidator contract (with transaction sender for execution)
    let liquidator_contract = LiquidatorContract::with_sender(
        config.liquidator_contract,
        tx_sender,
    );

    // Liquidator
    let liquidator = Arc::new(Liquidator::new(
        provider.clone(),
        liquidator_contract,
        liqd_client,
        config.profit_receiver,
    ));

    // Scanner config
    let scanner_config = ScannerConfig::default();

    // Scanner
    let scanner = Scanner::new(
        tracker,
        oracle_monitor,
        dual_oracle_monitor,
        heartbeat_predictor,
        pre_stager,
        liquidator,
        event_listener,
        blockanalitica,
        provider,
        assets,
        scanner_config,
    );

    info!("All components initialized");

    Ok((scanner, Vec::new()))
}

/// Print startup banner.
fn print_banner() {
    println!(r#"
    ╦ ╦┬ ┬┌─┐┌─┐┬─┐╦  ┌─┐┌┐┌┌┬┐
    ╠═╣└┬┘├─┘├┤ ├┬┘║  ├┤ │││ ││
    ╩ ╩ ┴ ┴  └─┘┴└─╩═╝└─┘┘└┘─┴┘
    Liquidation Bot v0.1.0
    "#);
}
