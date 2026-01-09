//! Liquidation Bot
//!
//! High-performance liquidation bot for AAVE v3/v4 forks on multiple EVM chains.
//! Features:
//! - Event-driven architecture via WebSocket subscriptions
//! - Tiered position tracking (Critical/Hot/Warm/Cold)
//! - Pre-staged transactions for sub-100ms latency
//! - DualOracle monitoring for LST arbitrage opportunities
//! - Multi-protocol support (AAVE v3, v4, and other ABIs)
//! - Multi-chain deployment (configurable RPC, gas, native tokens)
//!
//! Configuration:
//! All configuration is loaded from TOML files in the config/ directory.
//! Set DEPLOYMENT env var to select deployment (defaults to "hyperlend-prod").

use std::sync::Arc;

use anyhow::Result;
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use liquidator_api::{LiqdRouter, SwapRouterRegistry, UniswapV3Router};
use liquidator_chain::{
    DualOracleMonitor, EventListener, EventOracleType, LiquidatorContract, OracleMonitor,
    ProviderManager, TransactionSender, gas::create_gas_strategy,
};
use liquidator_core::{
    AssetRegistry, HeartbeatPredictor, Liquidator, PreStager, Scanner, ScannerConfig,
    TieredPositionTracker, init_config, load_deployment_from_env, ResolvedDeployment,
};

/// Environment variable for private key (required).
const PRIVATE_KEY_ENV: &str = "PRIVATE_KEY";

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
                .unwrap_or_else(|_| EnvFilter::new("info,liquidator_core=debug,liquidator_chain=debug")),
        )
        .init();

    info!("Starting Liquidation Bot");

    // Load deployment configuration from TOML files
    let deployment = load_deployment_from_env()?;

    // Initialize bot config from deployment
    deployment.bot.log_config();
    init_config(deployment.bot.clone());

    // Log deployment info
    info!("Deployment: {}", deployment.name);
    info!("Chain: {} ({})", deployment.chain.name, deployment.chain.chain_id);
    info!("Block time: {}ms", deployment.chain.block_time_ms);
    info!("Protocol: {} ({})", deployment.protocol.name, deployment.protocol.version);

    // Initialize components from deployment config
    let scanner = initialize_from_deployment(deployment).await?;

    // Bootstrap
    info!("Bootstrapping...");
    scanner.bootstrap().await?;

    // Run main loop
    info!("Starting main event loop...");
    scanner.run().await?;

    Ok(())
}

/// Initialize all components from deployment configuration.
async fn initialize_from_deployment(deployment: ResolvedDeployment) -> Result<Scanner> {
    info!("Initializing components...");

    let chain = &deployment.chain;
    let contracts = &deployment.contracts;

    // Provider manager
    let provider = Arc::new(
        ProviderManager::new(
            &chain.rpc.http,
            &chain.rpc.archive,
            &chain.rpc.send,
            &chain.rpc.ws,
            contracts.pool,
            contracts.balances_reader,
        )
        .await?,
    );

    info!(
        pool = %contracts.pool,
        balances_reader = %contracts.balances_reader,
        "Provider initialized"
    );

    // Asset registry from deployment
    let assets = Arc::new(AssetRegistry::from_resolved_assets(&deployment.assets));
    info!(asset_count = deployment.assets.len(), "Asset registry loaded");

    // Build oracle configs for event listener
    let oracle_configs: Vec<_> = deployment
        .assets
        .iter()
        .filter(|a| a.active)
        .map(|a| {
            let oracle_type = match a.oracle_type.as_str() {
                "standard" | "chainlink" => EventOracleType::Standard,
                "redstone" => EventOracleType::RedStone,
                "pyth" => EventOracleType::Pyth,
                "dual" | "dual_oracle" => EventOracleType::DualOracle,
                "pendle_pt" | "pendle" => EventOracleType::PendlePT,
                _ => EventOracleType::Standard,
            };
            (a.oracle, a.token, oracle_type)
        })
        .collect();

    // Event listener
    let event_listener = Arc::new(EventListener::new(
        &chain.rpc.ws,
        contracts.pool,
        oracle_configs,
    ));
    info!("Event listener configured");

    // Position tracker
    let tracker = Arc::new(TieredPositionTracker::new());

    // Oracle monitor
    let oracle_monitor = Arc::new(OracleMonitor::new(provider.clone()));

    // Register oracle-asset mappings
    for asset in deployment.assets.iter().filter(|a| a.active) {
        oracle_monitor.register_oracle(asset.oracle, asset.token);
    }

    // DualOracle monitor (LST assets)
    let dual_oracle_addrs: Vec<_> = deployment
        .assets
        .iter()
        .filter(|a| a.active && (a.oracle_type == "dual" || a.oracle_type == "dual_oracle"))
        .map(|a| a.oracle)
        .collect();
    let dual_oracle_monitor = Arc::new(DualOracleMonitor::new(dual_oracle_addrs.clone()));
    info!(lst_count = dual_oracle_addrs.len(), "DualOracle monitor initialized");

    // Heartbeat predictor
    let heartbeat_predictor = Arc::new(HeartbeatPredictor::new());

    // Pre-stager
    let pre_stager = Arc::new(PreStager::new());

    // Swap router registry
    let router_registry = create_router_from_config(
        chain.chain_id,
        &chain.rpc.http,
        &chain.swap_adapter,
    );
    info!(
        chain_id = chain.chain_id,
        adapter = %chain.swap_adapter,
        "Swap router initialized"
    );

    // Gas strategy from chain config
    let gas_strategy = create_gas_strategy(
        &chain.gas.pricing,
        chain.gas.default_gas_price_gwei,
        chain.gas.max_gas_price_gwei,
        chain.gas.priority_fee_gwei,
    );
    info!(
        pricing = %chain.gas.pricing,
        default_gwei = chain.gas.default_gas_price_gwei,
        max_gwei = chain.gas.max_gas_price_gwei,
        "Gas strategy configured"
    );

    // Transaction sender
    let private_key = std::env::var(PRIVATE_KEY_ENV)
        .map_err(|_| anyhow::anyhow!("Missing env var: {}", PRIVATE_KEY_ENV))?;

    let tx_sender = Arc::new(
        TransactionSender::with_gas_strategy(
            &private_key,
            &chain.rpc.send,
            chain.chain_id,
            gas_strategy,
        )
        .await?,
    );
    info!(address = %tx_sender.address, "Transaction sender initialized");

    // Liquidator contract
    let liquidator_contract = LiquidatorContract::with_sender(contracts.liquidator, tx_sender);

    // Liquidator
    let liquidator = Arc::new(Liquidator::new(
        provider.clone(),
        liquidator_contract,
        router_registry,
        chain.chain_id,
        contracts.profit_receiver,
    ));

    // Scanner
    let scanner_config = ScannerConfig::default();
    let scanner = Scanner::new(
        tracker,
        oracle_monitor,
        dual_oracle_monitor,
        heartbeat_predictor,
        pre_stager,
        liquidator,
        event_listener,
        provider,
        assets,
        scanner_config,
    );

    info!("All components initialized");

    Ok(scanner)
}

/// Create swap router from config.
fn create_router_from_config(
    chain_id: u64,
    rpc_url: &str,
    swap_adapter: &str,
) -> Arc<SwapRouterRegistry> {
    let mut registry = SwapRouterRegistry::new();

    match swap_adapter {
        "liqd" | "liqd.ag" => {
            registry = registry.with_router(Arc::new(LiqdRouter::new()));
            info!("Using LiqdRouter");
        }
        "uniswap_v3" | "uniswapv3" => {
            registry = registry.with_router(Arc::new(UniswapV3Router::new(rpc_url, chain_id)));
            info!("Using UniswapV3Router");
        }
        other => {
            tracing::warn!(adapter = other, "Unknown swap adapter, defaulting to UniswapV3");
            registry = registry.with_router(Arc::new(UniswapV3Router::new(rpc_url, chain_id)));
        }
    }

    Arc::new(registry)
}

/// Print startup banner.
fn print_banner() {
    println!(
        r#"
                                        /\
                                       /  \
                                /\    /    \
                               /  \  /      \      /\
                              /    \/        \    /  \
                         /\  /                \  /    \
                        /  \/                  \/      \
                    ___/                                \___

  ████████╗██████╗  █████╗ ███╗   ███╗██╗   ██╗███╗   ██╗████████╗ █████╗ ███╗   ██╗ █████╗
  ╚══██╔══╝██╔══██╗██╔══██╗████╗ ████║██║   ██║████╗  ██║╚══██╔══╝██╔══██╗████╗  ██║██╔══██╗
     ██║   ██████╔╝███████║██╔████╔██║██║   ██║██╔██╗ ██║   ██║   ███████║██╔██╗ ██║███████║
     ██║   ██╔══██╗██╔══██║██║╚██╔╝██║██║   ██║██║╚██╗██║   ██║   ██╔══██║██║╚██╗██║██╔══██║
     ██║   ██║  ██║██║  ██║██║ ╚═╝ ██║╚██████╔╝██║ ╚████║   ██║   ██║  ██║██║ ╚████║██║  ██║
     ╚═╝   ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝     ╚═╝ ╚═════╝ ╚═╝  ╚═══╝   ╚═╝   ╚═╝  ╚═╝╚═╝  ╚═══╝╚═╝  ╚═╝

                              M E V   S T R A T E G I E S
                                       v0.1.0
"#
    );
}
