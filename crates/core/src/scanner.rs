//! Scanner orchestration for the liquidation bot.
//!
//! Coordinates all components: event listening, position tracking,
//! pre-staging, and liquidation execution.

use alloy::primitives::{Address, U256};
use anyhow::Result;
use futures::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{debug, error, info, instrument, warn};

use crate::assets::AssetRegistry;
use crate::config::config;
use crate::heartbeat::HeartbeatPredictor;
use crate::liquidator::Liquidator;
use crate::position::{CollateralData, DebtData, PositionTier, TrackedPosition};
use crate::position_tracker::TieredPositionTracker;
use crate::pre_staging::PreStager;
use crate::sensitivity::PositionSensitivity;
use liquidator_api::{BlockAnaliticaClient, SwapParams};
use liquidator_chain::{
    DualOracleMonitor, EventListener, OracleMonitor, OracleUpdate, PoolEvent, ProviderManager,
};

/// Scanner configuration.
/// Uses values from global BotConfig by default.
#[derive(Debug, Clone)]
pub struct ScannerConfig {
    /// Maximum health factor for initial seeding
    pub seed_hf_max: f64,
    /// Maximum wallets to seed
    pub seed_limit: usize,
    /// Bootstrap resync interval
    pub bootstrap_interval: Duration,
    /// Critical tier update interval
    pub critical_interval: Duration,
    /// Hot tier update interval
    pub hot_interval: Duration,
    /// Warm tier update interval
    pub warm_interval: Duration,
    /// Cold tier update interval
    pub cold_interval: Duration,
    /// DualOracle check interval
    pub dual_oracle_interval: Duration,
    /// Heartbeat check interval
    pub heartbeat_interval: Duration,
}

impl Default for ScannerConfig {
    fn default() -> Self {
        // Read from global config
        let cfg = config();
        Self {
            seed_hf_max: cfg.position.seed_hf_max,
            seed_limit: cfg.position.seed_limit,
            bootstrap_interval: cfg.scanner.bootstrap_interval(),
            critical_interval: cfg.scanner.critical_interval(),
            hot_interval: cfg.scanner.hot_interval(),
            warm_interval: cfg.scanner.warm_interval(),
            cold_interval: cfg.scanner.cold_interval(),
            dual_oracle_interval: cfg.scanner.dual_oracle_interval(),
            heartbeat_interval: cfg.scanner.heartbeat_interval(),
        }
    }
}

/// Main scanner orchestrating all liquidation bot components.
pub struct Scanner {
    /// Position tracker
    tracker: Arc<TieredPositionTracker>,
    /// Oracle monitor
    oracle_monitor: Arc<OracleMonitor>,
    /// DualOracle monitor for LST assets
    dual_oracle_monitor: Arc<DualOracleMonitor>,
    /// Heartbeat predictor
    heartbeat_predictor: Arc<HeartbeatPredictor>,
    /// Pre-staging pipeline
    pre_stager: Arc<PreStager>,
    /// Liquidation executor
    liquidator: Arc<Liquidator>,
    /// Event listener
    event_listener: Arc<EventListener>,
    /// BlockAnalitica API client
    blockanalitica: Arc<BlockAnaliticaClient>,
    /// Provider manager
    provider: Arc<ProviderManager>,
    /// Asset registry
    assets: Arc<AssetRegistry>,
    /// Configuration
    config: ScannerConfig,
}

impl Scanner {
    /// Create a new scanner.
    pub fn new(
        tracker: Arc<TieredPositionTracker>,
        oracle_monitor: Arc<OracleMonitor>,
        dual_oracle_monitor: Arc<DualOracleMonitor>,
        heartbeat_predictor: Arc<HeartbeatPredictor>,
        pre_stager: Arc<PreStager>,
        liquidator: Arc<Liquidator>,
        event_listener: Arc<EventListener>,
        blockanalitica: Arc<BlockAnaliticaClient>,
        provider: Arc<ProviderManager>,
        assets: Arc<AssetRegistry>,
        config: ScannerConfig,
    ) -> Self {
        Self {
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
            config,
        }
    }

    /// Bootstrap the scanner with initial data.
    #[instrument(skip(self))]
    pub async fn bootstrap(&self) -> Result<()> {
        info!("Starting bootstrap...");

        // 0. Log wallet stats to show how many positions exist in total
        match self.blockanalitica.get_wallet_stats().await {
            Ok(stats) => {
                info!(
                    bad_debt_wallets = stats.bad_debt_total,
                    at_risk_wallets = stats.at_risk_total,
                    total = stats.bad_debt_total + stats.at_risk_total,
                    min_collateral_threshold = format!("${:.2}", stats.min_collateral_threshold),
                    "BlockAnalitica wallet inventory"
                );

                // Also analyze position size distribution on first bootstrap
                if let Err(e) = self.blockanalitica.analyze_position_distribution().await {
                    debug!(error = %e, "Failed to analyze position distribution");
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to fetch wallet stats");
            }
        }

        // 1. Fetch at-risk wallets from BlockAnalitica (bad debt + approaching liquidation)
        let bad_debt_wallets = self
            .blockanalitica
            .fetch_at_risk_wallets(self.config.seed_hf_max, self.config.seed_limit)
            .await?;

        // Also fetch wallets approaching liquidation (HF 1.0-1.25 range)
        let approaching_wallets = self
            .blockanalitica
            .fetch_wallets_at_risk(1.0, 1.25, self.config.seed_limit)
            .await
            .unwrap_or_default();

        // Combine and dedupe
        let mut wallets = bad_debt_wallets;
        for wallet in approaching_wallets {
            if !wallets.iter().any(|w| w.wallet_address == wallet.wallet_address) {
                wallets.push(wallet);
            }
        }

        info!(
            total = wallets.len(),
            "Fetched combined at-risk wallets (bad-debt + approaching)"
        );

        // 2. Fetch full position data and classify
        // OPTIMIZATION: Process wallets in parallel with bounded concurrency (5-10x faster)
        let total_wallets = wallets.len();
        info!(total = total_wallets, "Processing wallets in parallel...");

        // Collect valid addresses
        let addresses: Vec<Address> = wallets
            .iter()
            .filter_map(|w| w.address())
            .collect();

        // Use the batch method with bounded parallelism (20 concurrent)
        let results = self.provider.get_positions_batch(&addresses, 20).await;

        // Process results
        let mut success_count = 0;
        let mut error_count = 0;

        for (user, result) in results {
            match result {
                Ok((supplies, borrows)) => {
                    if supplies.is_empty() && borrows.is_empty() {
                        self.tracker.remove(&user);
                        continue;
                    }

                    // Build and track position (inline version of process_wallet logic)
                    if let Err(e) = self.update_position_from_data(&user, supplies, borrows).await {
                        warn!(user = %user, error = %e, "Failed to process position");
                        error_count += 1;
                    } else {
                        success_count += 1;
                    }
                }
                Err(e) => {
                    warn!(user = %user, error = %e, "Failed to fetch position data");
                    error_count += 1;
                }
            }
        }

        info!(
            total = total_wallets,
            success = success_count,
            errors = error_count,
            "Finished processing all wallets"
        );

        // Log tracker stats after wallet processing
        let stats = self.tracker.stats();
        info!(
            critical = stats.critical_count,
            hot = stats.hot_count,
            warm = stats.warm_count,
            cold = stats.cold_count,
            total = stats.total_positions(),
            "Tracker stats after wallet processing"
        );

        // 3. Rebuild trigger index
        self.tracker.rebuild_trigger_index();

        // 4. Pre-stage critical positions
        let critical = self.tracker.critical_positions();
        info!(count = critical.len(), "Pre-staging critical positions");

        for position in critical {
            if let Err(e) = self.stage_position(&position).await {
                warn!(user = %position.user, error = %e, "Failed to pre-stage position");
            }
        }

        // 5. Initialize oracle prices
        self.oracle_monitor.refresh_all_prices().await?;

        // 6. Execute liquidations for positions that are ALREADY liquidatable
        let critical_for_liq = self.tracker.critical_positions();
        info!(
            critical_count = critical_for_liq.len(),
            "Step 6: Checking critical positions for immediate liquidation"
        );

        let mut liquidated_count = 0;
        for position in critical_for_liq {
            let is_liq = position.is_liquidatable();
            let is_bad = position.is_bad_debt();

            info!(
                user = %position.user,
                hf = %position.health_factor,
                is_liquidatable = is_liq,
                is_bad_debt = is_bad,
                collateral_usd = %position.total_collateral_usd(),
                debt_usd = %position.total_debt_usd(),
                "Evaluating critical position for immediate liquidation"
            );

            if is_liq && !is_bad {
                info!(
                    user = %position.user,
                    hf = %position.health_factor,
                    collateral_usd = %position.total_collateral_usd(),
                    debt_usd = %position.total_debt_usd(),
                    "Executing immediate liquidation (already below HF 1.0)"
                );

                match self.execute_liquidation(&position.user).await {
                    Ok(_) => {
                        liquidated_count += 1;
                        info!(user = %position.user, "Liquidation executed successfully");
                    }
                    Err(e) => {
                        error!(user = %position.user, error = %e, "Liquidation execution failed");
                    }
                }
            } else {
                info!(
                    user = %position.user,
                    reason = if !is_liq { "not liquidatable (HF >= 1.0)" } else { "bad debt" },
                    "Skipping position"
                );
            }
        }

        if liquidated_count > 0 {
            info!(count = liquidated_count, "Immediate liquidations completed");
        } else {
            info!("No positions qualified for immediate liquidation");
        }

        info!("Bootstrap complete");
        Ok(())
    }

    /// Run the main event loop.
    pub async fn run(&self) -> Result<()> {
        info!("Starting scanner event loop...");

        // Create channels for internal events
        let (liquidation_tx, mut liquidation_rx) = mpsc::channel::<Address>(100);

        // Spawn event handlers
        let scanner = Arc::new(self.clone_refs());

        // Oracle update handler (with reconnection)
        let oracle_scanner = scanner.clone();
        let oracle_liq_tx = liquidation_tx.clone();
        tokio::spawn(async move {
            loop {
                match oracle_scanner.oracle_event_loop(oracle_liq_tx.clone()).await {
                    Ok(_) => {
                        warn!("Oracle event loop ended, reconnecting in 5s...");
                    }
                    Err(e) => {
                        error!(error = %e, "Oracle event loop failed, reconnecting in 5s...");
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        });

        // Pool event handler (with reconnection)
        let pool_scanner = scanner.clone();
        tokio::spawn(async move {
            loop {
                match pool_scanner.pool_event_loop().await {
                    Ok(_) => {
                        warn!("Pool event loop ended, reconnecting in 5s...");
                    }
                    Err(e) => {
                        error!(error = %e, "Pool event loop failed, reconnecting in 5s...");
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        });

        // Background cycles
        let critical_scanner = scanner.clone();
        tokio::spawn(async move {
            critical_scanner.critical_cycle().await;
        });

        let hot_scanner = scanner.clone();
        tokio::spawn(async move {
            hot_scanner.hot_cycle().await;
        });

        let warm_scanner = scanner.clone();
        tokio::spawn(async move {
            warm_scanner.warm_cycle().await;
        });

        let cold_scanner = scanner.clone();
        tokio::spawn(async move {
            cold_scanner.cold_cycle().await;
        });

        let bootstrap_scanner = scanner.clone();
        tokio::spawn(async move {
            bootstrap_scanner.bootstrap_cycle().await;
        });

        let dual_scanner = scanner.clone();
        tokio::spawn(async move {
            dual_scanner.dual_oracle_cycle().await;
        });

        let heartbeat_scanner = scanner.clone();
        tokio::spawn(async move {
            heartbeat_scanner.heartbeat_cycle().await;
        });

        // Liquidation processor
        while let Some(user) = liquidation_rx.recv().await {
            if let Err(e) = self.execute_liquidation(&user).await {
                error!(user = %user, error = %e, "Liquidation failed");
            }
        }

        Ok(())
    }

    /// Handle oracle update events.
    async fn oracle_event_loop(&self, liq_tx: mpsc::Sender<Address>) -> Result<()> {
        info!("Starting oracle event loop - subscribing to WebSocket...");
        let mut stream = self.event_listener.subscribe_oracle_updates().await?;
        info!("Oracle WebSocket subscription active - waiting for AnswerUpdated events...");

        while let Some(update) = stream.next().await {
            if let Err(e) = self.on_oracle_update(update, &liq_tx).await {
                warn!(error = %e, "Failed to process oracle update");
            }
        }

        warn!("Oracle event stream ended");
        Ok(())
    }

    /// Handle pool events.
    async fn pool_event_loop(&self) -> Result<()> {
        info!("Starting pool event loop - subscribing to WebSocket...");
        let mut stream = self.event_listener.subscribe_pool_events().await?;
        info!("Pool WebSocket subscription active - waiting for pool events...");

        while let Some(event) = stream.next().await {
            info!(event_type = %event.event_type(), user = %event.user(), block = event.block_number(), "Pool event received");
            if let Err(e) = self.on_pool_event(event).await {
                warn!(error = %e, "Failed to process pool event");
            }
        }

        warn!("Pool event stream ended");
        Ok(())
    }

    /// Process an oracle price update.
    #[instrument(skip(self, liq_tx), fields(asset = %update.asset))]
    async fn on_oracle_update(
        &self,
        update: OracleUpdate,
        liq_tx: &mpsc::Sender<Address>,
    ) -> Result<()> {
        // Log every oracle update received
        info!(
            oracle = %update.oracle,
            asset = %update.asset,
            price = %update.price,
            block = update.block_number,
            "Oracle update received"
        );

        let old_price = self
            .tracker
            .get_price(&update.asset)
            .map(|p| p.price)
            .unwrap_or(U256::ZERO);

        // Update price cache
        self.oracle_monitor.update_price(update.clone());
        self.tracker.update_price(
            update.asset,
            liquidator_chain::OraclePrice {
                price: update.price,
                updated_at: update.timestamp,
                block_number: update.block_number,
                oracle_type: update.oracle_type,
            },
        );

        // Update heartbeat predictor
        self.heartbeat_predictor.record_update(
            update.oracle,
            update.timestamp,
            update.block_number,
        );

        // Check for liquidatable positions via trigger index
        let liquidatable = self
            .tracker
            .trigger_index()
            .get_liquidatable_at(update.asset, update.price, old_price);

        for user in liquidatable {
            // Skip bad debt / dust positions
            if let Some(position) = self.tracker.get(&user) {
                if position.is_bad_debt() {
                    debug!(user = %user, "Skipping bad debt position");
                    continue;
                }
                info!(user = %user, asset = %update.asset, "Position crossed liquidation threshold");
                let _ = liq_tx.send(user).await;
            }
        }

        // Update affected positions
        let affected = self.tracker.users_affected_by_asset(&update.asset);
        for user in &affected {
            let user = *user; // Copy the address
            if let Some(position) = self.tracker.get(&user) {
                // Use sensitivity for fast HF estimation
                if let Some(sensitivity) = &position.sensitivity {
                    let new_hf = sensitivity.estimate_hf_from_prices(&[(update.asset, update.price)]);

                    // Re-tier if needed
                    let new_tier = PositionTier::from_health_factor(new_hf);
                    if new_tier != position.tier {
                        self.tracker.re_tier(&user, new_hf, position.min_trigger_distance_pct);
                    }

                    // Check for liquidation (skip bad debt)
                    if new_hf < 1.0 && !position.is_bad_debt() {
                        let _ = liq_tx.send(user).await;
                    }
                }
            }
        }

        // Invalidate stale pre-staged transactions
        self.pre_stager.invalidate_by_asset(&update.asset, &affected);

        Ok(())
    }

    /// Process a pool event.
    #[instrument(skip(self), fields(event_type = ?event.event_type()))]
    async fn on_pool_event(&self, event: PoolEvent) -> Result<()> {
        let user = event.user();

        // Re-fetch position data
        if let Err(e) = self.process_wallet(&user).await {
            warn!(user = %user, error = %e, "Failed to update position after pool event");
        }

        // Invalidate pre-staged transaction
        self.pre_stager.invalidate(&user);

        Ok(())
    }

    /// Execute a liquidation for a user.
    #[instrument(skip(self), fields(user = %user))]
    async fn execute_liquidation(&self, user: &Address) -> Result<()> {
        // Check for valid pre-staged transaction
        if let Some(staged) = self.pre_stager.get_valid_staged(user) {
            info!(user = %user, "Using pre-staged transaction");
            self.liquidator.execute_staged(staged).await?;
        } else {
            // Build and execute fresh
            if let Some(position) = self.tracker.get(user) {
                info!(user = %user, "Building fresh liquidation");
                self.liquidator.build_and_execute(&position).await?;
            }
        }

        // Remove from tracker after successful liquidation
        self.tracker.remove(user);

        Ok(())
    }

    // Background cycles

    async fn critical_cycle(&self) {
        let mut ticker = interval(self.config.critical_interval);
        loop {
            ticker.tick().await;

            // Validate and refresh pre-staged transactions
            for position in self.tracker.critical_positions() {
                if !self.pre_stager.has_valid_staged(&position.user) {
                    if let Err(e) = self.stage_position(&position).await {
                        debug!(user = %position.user, error = %e, "Failed to re-stage");
                    }
                }
            }
        }
    }

    async fn hot_cycle(&self) {
        let mut ticker = interval(self.config.hot_interval);
        loop {
            ticker.tick().await;

            // Update sensitivities and check swap routes
            for position in self.tracker.hot_positions() {
                if position.needs_update() {
                    let sensitivity = PositionSensitivity::compute(&position, self.tracker.prices());
                    // Update would require mutable access - simplified here
                }
            }
        }
    }

    async fn warm_cycle(&self) {
        let mut ticker = interval(self.config.warm_interval);
        loop {
            ticker.tick().await;

            // Recalculate trigger prices for warm tier
            for position in self.tracker.warm_positions() {
                if position.needs_update() {
                    self.tracker.trigger_index().update_position(&position);
                }
            }
        }
    }

    async fn cold_cycle(&self) {
        let mut ticker = interval(self.config.cold_interval);
        loop {
            ticker.tick().await;

            // Full position refresh for cold tier
            for position in self.tracker.cold_positions() {
                if position.needs_update() {
                    if let Err(e) = self.process_wallet(&position.user).await {
                        debug!(user = %position.user, error = %e, "Failed to refresh cold position");
                    }
                }
            }
        }
    }

    async fn bootstrap_cycle(&self) {
        let mut ticker = interval(self.config.bootstrap_interval);
        loop {
            ticker.tick().await;

            // Resync with BlockAnalitica
            if let Err(e) = self.bootstrap().await {
                warn!(error = %e, "Bootstrap resync failed");
            }
        }
    }

    async fn dual_oracle_cycle(&self) {
        let mut ticker = interval(self.config.dual_oracle_interval);
        loop {
            ticker.tick().await;

            // Check for tier transitions in DualOracle assets
            for asset in self.assets.dual_oracle_assets() {
                if let Some(transition) = self.dual_oracle_monitor.check_transition(asset.oracle) {
                    info!(
                        asset = asset.symbol,
                        from = ?transition.from,
                        to = ?transition.to,
                        "DualOracle tier transition detected"
                    );
                }
            }
        }
    }

    async fn heartbeat_cycle(&self) {
        let mut ticker = interval(self.config.heartbeat_interval);
        loop {
            ticker.tick().await;

            // Check for imminent oracle updates
            let imminent = self
                .heartbeat_predictor
                .imminent_updates(Duration::from_millis(500));

            for oracle in imminent {
                if let Some(asset) = self.assets.get_by_oracle(&oracle) {
                    debug!(asset = asset.symbol, "Oracle update imminent");
                }
            }

            // Log stale oracles
            for oracle in self.heartbeat_predictor.stale_oracles() {
                if let Some(asset) = self.assets.get_by_oracle(&oracle) {
                    warn!(asset = asset.symbol, "Oracle is stale");
                }
            }
        }
    }

    // Helper methods

    async fn process_wallet(&self, user: &Address) -> Result<()> {
        // Fetch position data from chain
        let (supplies, borrows) = self.provider.get_position_data(*user).await?;

        if supplies.is_empty() && borrows.is_empty() {
            self.tracker.remove(user);
            return Ok(());
        }

        let mut position = TrackedPosition::new(*user);

        // Process supplies
        for supply in supplies {
            let collateral = CollateralData {
                asset: supply.underlying,
                amount: supply.amount,
                price: supply.price,
                decimals: supply.decimals,
                value_usd: CollateralData::calculate_usd_value(
                    supply.amount,
                    supply.price,
                    supply.decimals,
                ),
                liquidation_threshold: supply.liquidation_threshold,
                enabled: true,
            };
            position.collaterals.push((supply.underlying, collateral));
        }

        // Process borrows
        for borrow in borrows {
            let debt = DebtData {
                asset: borrow.underlying,
                amount: borrow.amount,
                price: borrow.price,
                decimals: borrow.decimals,
                value_usd: DebtData::calculate_usd_value(
                    borrow.amount,
                    borrow.price,
                    borrow.decimals,
                ),
            };
            position.debts.push((borrow.underlying, debt));
        }

        // Calculate health factor and tier
        position.health_factor = position.calculate_health_factor();
        position.update_tier();
        position.state_hash = position.compute_state_hash();

        // Debug: Log calculated position values
        debug!(
            user = %user,
            hf = %position.health_factor,
            tier = ?position.tier,
            collateral_usd = %position.total_collateral_usd(),
            debt_usd = %position.total_debt_usd(),
            collateral_count = position.collaterals.iter().filter(|(_, c)| c.value_usd > 0.0).count(),
            debt_count = position.debts.iter().filter(|(_, d)| d.value_usd > 0.0).count(),
            is_bad_debt = position.is_bad_debt(),
            "Position calculated"
        );

        // Log important positions
        if position.is_liquidatable() {
            if position.is_bad_debt() {
                // Skip logging dust/bad debt - too much noise
                debug!(
                    user = %user,
                    hf = %position.health_factor,
                    collateral_usd = %position.total_collateral_usd(),
                    debt_usd = %position.total_debt_usd(),
                    "BAD DEBT (dust position, skipping)"
                );
            } else {
                // Real liquidation opportunity!
                warn!(
                    user = %user,
                    hf = %position.health_factor,
                    tier = ?position.tier,
                    collateral_usd = %position.total_collateral_usd(),
                    debt_usd = %position.total_debt_usd(),
                    "LIQUIDATABLE position detected - will be added to tracker"
                );
            }
        } else if matches!(position.tier, PositionTier::Critical) {
            info!(
                user = %user,
                hf = %position.health_factor,
                tier = ?position.tier,
                "Critical position tracked"
            );
        }

        // Skip tracking bad debt positions entirely - they waste resources
        // and will never be liquidatable profitably
        if position.is_bad_debt() {
            return Ok(());
        }

        // Compute sensitivity for critical/hot tiers
        if matches!(position.tier, PositionTier::Critical | PositionTier::Hot) {
            position.sensitivity =
                Some(PositionSensitivity::compute(&position, self.tracker.prices()));
        }

        self.tracker.upsert(position);

        Ok(())
    }

    /// Update position from pre-fetched data (for batch processing).
    /// Same logic as process_wallet but without fetching.
    async fn update_position_from_data(
        &self,
        user: &Address,
        supplies: Vec<liquidator_chain::BalanceData>,
        borrows: Vec<liquidator_chain::BalanceData>,
    ) -> Result<()> {
        let mut position = TrackedPosition::new(*user);

        // Process supplies
        for supply in supplies {
            let collateral = CollateralData {
                asset: supply.underlying,
                amount: supply.amount,
                price: supply.price,
                decimals: supply.decimals,
                value_usd: CollateralData::calculate_usd_value(
                    supply.amount,
                    supply.price,
                    supply.decimals,
                ),
                liquidation_threshold: supply.liquidation_threshold,
                enabled: true,
            };
            position.collaterals.push((supply.underlying, collateral));
        }

        // Process borrows
        for borrow in borrows {
            let debt = DebtData {
                asset: borrow.underlying,
                amount: borrow.amount,
                price: borrow.price,
                decimals: borrow.decimals,
                value_usd: DebtData::calculate_usd_value(
                    borrow.amount,
                    borrow.price,
                    borrow.decimals,
                ),
            };
            position.debts.push((borrow.underlying, debt));
        }

        // Calculate health factor and tier
        position.health_factor = position.calculate_health_factor();
        position.update_tier();
        position.state_hash = position.compute_state_hash();

        // Skip tracking bad debt positions entirely
        if position.is_bad_debt() {
            return Ok(());
        }

        // Compute sensitivity for critical/hot tiers
        if matches!(position.tier, PositionTier::Critical | PositionTier::Hot) {
            position.sensitivity =
                Some(PositionSensitivity::compute(&position, self.tracker.prices()));
        }

        self.tracker.upsert(position);

        Ok(())
    }

    async fn stage_position(&self, position: &TrackedPosition) -> Result<()> {
        if !self.pre_stager.should_stage(position) {
            debug!(
                user = %position.user,
                hf = %position.health_factor,
                debt_usd = %position.total_debt_usd(),
                "Skipping pre-stage (dust or low debt)"
            );
            return Ok(());
        }

        info!(
            user = %position.user,
            hf = %position.health_factor,
            collateral_usd = %position.total_collateral_usd(),
            debt_usd = %position.total_debt_usd(),
            "Pre-staging position"
        );

        let (collateral_asset, collateral) = position
            .largest_collateral()
            .ok_or_else(|| anyhow::anyhow!("No collateral"))?;

        let (debt_asset, debt) = position
            .largest_debt()
            .ok_or_else(|| anyhow::anyhow!("No debt"))?;

        // Get swap route using chain-aware router registry
        let collateral_amount = collateral.amount / U256::from(2); // 50% close factor
        let swap_params = SwapParams::new(
            *collateral_asset,
            *debt_asset,
            collateral_amount,
            collateral.decimals,
        );
        let swap_route = match self
            .liquidator
            .router_registry()
            .get_route_with_fallback(self.liquidator.chain_id(), swap_params)
            .await
        {
            Ok(route) => route,
            Err(e) => {
                warn!(
                    user = %position.user,
                    error = %e,
                    "Swap router failed, using direct route fallback"
                );
                // Create minimal direct route fallback
                use liquidator_api::swap::{SwapAllocation as ApiAlloc, SwapHop, SwapRoute};
                SwapRoute {
                    token_in: *collateral_asset,
                    token_out: *debt_asset,
                    amount_in: collateral_amount,
                    expected_output: collateral_amount,
                    min_output: collateral_amount * U256::from(995) / U256::from(1000),
                    hops: vec![SwapHop {
                        allocations: vec![ApiAlloc {
                            token_in: *collateral_asset,
                            token_out: *debt_asset,
                            router_index: 0,
                            fee: 3000,
                            amount_in: collateral_amount,
                            stable: false,
                        }],
                    }],
                    tokens: vec![*collateral_asset, *debt_asset],
                    price_impact: None,
                    expected_input_usd: None,
                    expected_output_usd: None,
                    encoded_calldata: None,
                }
            }
        };

        // Create price snapshot
        let mut price_snapshot = smallvec::SmallVec::new();
        if let Some(price) = self.tracker.get_price(collateral_asset) {
            price_snapshot.push((*collateral_asset, price.price));
        }
        if let Some(price) = self.tracker.get_price(debt_asset) {
            price_snapshot.push((*debt_asset, price.price));
        }

        // Pre-encode calldata for fast execution path (~5ms savings)
        let debt_to_cover = debt.amount;
        let expected_collateral = collateral.amount / U256::from(2);
        let min_amount_out = swap_route.min_output;

        match self.liquidator.encode_liquidation_calldata(
            position.user,
            *collateral_asset,
            *debt_asset,
            debt_to_cover,
            &swap_route,
            min_amount_out,
        ) {
            Ok(encoded_calldata) => {
                // Use fast path with pre-encoded calldata
                self.pre_stager.stage_with_calldata(
                    position,
                    swap_route,
                    debt_to_cover,
                    expected_collateral,
                    price_snapshot,
                    encoded_calldata,
                    min_amount_out,
                    1_600_000, // Estimated gas for liquidation
                );
                info!(
                    user = %position.user,
                    "Position pre-staged with pre-encoded calldata (FAST PATH)"
                );
            }
            Err(e) => {
                // Fallback to slow path without pre-encoding
                warn!(
                    user = %position.user,
                    error = %e,
                    "Failed to pre-encode calldata, using slow path"
                );
                self.pre_stager.stage(
                    position,
                    swap_route,
                    debt_to_cover,
                    expected_collateral,
                    price_snapshot,
                );
                info!(user = %position.user, "Position pre-staged (slow path)");
            }
        }

        Ok(())
    }

    fn clone_refs(&self) -> Self {
        Self {
            tracker: self.tracker.clone(),
            oracle_monitor: self.oracle_monitor.clone(),
            dual_oracle_monitor: self.dual_oracle_monitor.clone(),
            heartbeat_predictor: self.heartbeat_predictor.clone(),
            pre_stager: self.pre_stager.clone(),
            liquidator: self.liquidator.clone(),
            event_listener: self.event_listener.clone(),
            blockanalitica: self.blockanalitica.clone(),
            provider: self.provider.clone(),
            assets: self.assets.clone(),
            config: self.config.clone(),
        }
    }
}
