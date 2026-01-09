# Elite Liquidation Bot: Implementation Plan

## Executive Summary

This document outlines the implementation plan to transform the current liquidation bot into an elite, production-grade system based on the consolidated strategy. The plan identifies gaps, prioritizes features, and provides a phased implementation approach.

**Current State Assessment:**
- ✅ Trigger index exists (basic implementation)
- ✅ Tiered position tracking (Critical/Hot/Warm/Cold)
- ✅ Oracle event listening via WebSocket
- ✅ Pre-staging for critical positions
- ✅ Basic profitability calculation
- ⚠️ Missing: Interval tree optimization, dynamic thresholds, EV-based scoring, operating modes, competitor analysis, multi-collateral handling

**Target State:**
- Interval tree for O(log n) liquidation price queries
- Dynamic tier thresholds based on asset volatility
- Expected Value (EV) based priority scoring with win probability
- Operating modes (NORMAL/ELEVATED/SPIKE/DEGRADED) with automatic transitions
- Real-time gas price integration in all profitability calculations
- Multi-collateral composite trigger handling
- Historical position seeding on startup
- Competitor fingerprinting and strategic response
- Position archiving and lifecycle management

---

## Phase 1: Core Infrastructure Enhancements

### 1.1 Interval Tree for Liquidation Price Index

**Priority:** CRITICAL  
**Effort:** Medium (3-5 days)  
**Dependencies:** None

**Current Issue:**
- `TriggerIndex` uses linear search: `O(n)` per price update
- With 1000+ positions, this becomes a bottleneck
- Strategy requires `O(log n)` range queries

**Implementation:**
1. Add `interval_tree` crate dependency
2. Create `LiquidationPriceIndex` struct:
   ```rust
   pub struct LiquidationPriceIndex {
       // Asset -> IntervalTree<PriceRange, Vec<User>>
       trees: DashMap<Address, IntervalTree<U256, Vec<Address>>>,
   }
   ```
3. Implement range query: `get_liquidatable_in_range(asset, min_price, max_price)`
4. Replace linear search in `TriggerIndex::get_liquidatable_at()`
5. Maintain sorted order for efficient inserts

**Files to Modify:**
- `crates/core/src/trigger_index.rs` - Add interval tree implementation
- `Cargo.toml` - Add `interval_tree` or implement custom interval tree

**Testing:**
- Unit tests for range queries
- Performance benchmarks: 1000 positions, 100 price updates
- Verify correctness against linear search

---

### 1.2 Dynamic Tier Thresholds Based on Volatility

**Priority:** HIGH  
**Effort:** Medium (2-3 days)  
**Dependencies:** Oracle price history

**Current Issue:**
- Fixed thresholds in `PositionTier::from_health_factor()`
- No adaptation to market volatility
- Strategy requires per-asset, volatility-adjusted thresholds

**Implementation:**
1. Add volatility tracking:
   ```rust
   pub struct AssetVolatility {
       asset: Address,
       volatility_per_block: f64,  // Rolling 24h volatility
       last_updated: Instant,
   }
   ```
2. Calculate volatility from price history (24h rolling window)
3. Update `TierConfig` to support dynamic thresholds:
   ```rust
   pub struct DynamicTierConfig {
       base_critical: f64,
       base_hot: f64,
       volatility_multiplier: f64,
   }
   ```
4. Modify `PositionTier::classify()` to use dynamic thresholds:
   ```rust
   pub fn classify_dynamic(hf: f64, asset_volatility: f64, blocks_to_execute: u64) -> Self {
       let critical_threshold = 1.0 + (asset_volatility * blocks_to_execute as f64 * 1.5);
       // ...
   }
   ```

**Files to Modify:**
- `crates/core/src/position.rs` - Add dynamic tier classification
- `crates/core/src/config/bot.rs` - Add volatility config
- `crates/core/src/volatility.rs` - New file for volatility tracking

**Testing:**
- Test threshold calculation with various volatility levels
- Verify tier transitions during high volatility periods

---

### 1.3 Hysteresis for Tier Transitions

**Priority:** HIGH  
**Effort:** Low (1-2 days)  
**Dependencies:** Dynamic thresholds

**Current Issue:**
- Tier transitions are immediate, causing oscillation during volatile periods
- Strategy requires separate enter/exit thresholds with cooldown

**Implementation:**
1. Add hysteresis bands to `TierConfig`:
   ```rust
   pub struct TierHysteresis {
       critical_enter: f64,  // 1.02
       critical_exit: f64,   // 1.04
       hot_enter: f64,       // 1.08
       hot_exit: f64,        // 1.12
       // ...
   }
   ```
2. Track tier entry time in `TrackedPosition`:
   ```rust
   pub struct TrackedPosition {
       // ...
       tier_entry_time: Instant,
       tier_entry_hf: f64,
   }
   ```
3. Implement cooldown logic (minimum 60s in tier before downgrade)
4. Update `TieredPositionTracker::re_tier()` to respect hysteresis

**Files to Modify:**
- `crates/core/src/position.rs` - Add tier entry tracking
- `crates/core/src/position_tracker.rs` - Add hysteresis logic
- `crates/core/src/config/bot.rs` - Add hysteresis config

**Testing:**
- Test rapid price oscillations don't cause tier thrashing
- Verify cooldown prevents premature downgrades

---

## Phase 2: Profitability and Execution

### 2.1 Expected Value (EV) Based Priority Scoring

**Priority:** CRITICAL  
**Effort:** Medium (3-4 days)  
**Dependencies:** Win probability estimation

**Current Issue:**
- Simple profitability check: `net_profit >= min_profit`
- No consideration of win probability or competitor factors
- Strategy requires: `EV = (profit × win_prob) - (gas × (1 - win_prob))`

**Implementation:**
1. Create `ExpectedValue` struct:
   ```rust
   pub struct ExpectedValue {
       net_profit: f64,
       win_probability: f64,
       gas_cost: f64,
       ev: f64,  // Calculated
   }
   
   impl ExpectedValue {
       pub fn calculate(net_profit: f64, win_prob: f64, gas_cost: f64) -> Self {
           let ev = (net_profit * win_prob) - (gas_cost * (1.0 - win_prob));
           Self { net_profit, win_probability: win_prob, gas_cost, ev }
       }
   }
   ```
2. Implement win probability estimation:
   ```rust
   pub fn estimate_win_probability(
       position: &TrackedPosition,
       tier: PositionTier,
       data_freshness: Duration,
       oracle_freshness: Duration,
       competitor_factor: f64,
   ) -> f64 {
       let base = match tier {
           PositionTier::Critical => 0.8,
           PositionTier::Hot => 0.6,
           PositionTier::Warm => 0.4,
           _ => 0.2,
       };
       
       let freshness_penalty = if data_freshness > Duration::from_secs(5) { 0.7 } else { 1.0 };
       let oracle_penalty = if oracle_freshness > Duration::from_secs(30) { 0.7 } else { 1.0 };
       
       base * freshness_penalty * oracle_penalty * (1.0 - competitor_factor)
   }
   ```
3. Update `Liquidator::estimate_profit()` to return `ExpectedValue`
4. Sort liquidation queue by EV (descending)

**Files to Modify:**
- `crates/core/src/liquidator.rs` - Add EV calculation
- `crates/core/src/scanner.rs` - Sort by EV in liquidation queue
- `crates/core/src/ev_calculator.rs` - New file for EV logic

**Testing:**
- Test EV calculation with various win probabilities
- Verify queue ordering by EV

---

### 2.2 Real-Time Gas Price Integration

**Priority:** HIGH  
**Effort:** Medium (2-3 days)  
**Dependencies:** Gas price monitoring

**Current Issue:**
- Gas cost is static: `DEFAULT_ESTIMATED_GAS_COST_USD = 0.03`
- No real-time gas price fetching
- Strategy requires: `gas_cost = gas_estimate × current_gas_price`

**Implementation:**
1. Add gas price monitor:
   ```rust
   pub struct GasPriceMonitor {
       current_gas_price: Arc<RwLock<U256>>,
       gas_price_5min_avg: Arc<RwLock<f64>>,
       update_interval: Duration,
   }
   ```
2. Fetch gas price every block (or every 5 seconds)
3. Update `LiquidationParams` to include real-time gas:
   ```rust
   pub struct LiquidationParams {
       // ...
       gas_estimate: u64,  // Estimated gas units
       gas_price_fetcher: Arc<dyn Fn() -> Future<Output = U256>>,
   }
   ```
4. Modify `ProfitEstimate` to use real-time gas:
   ```rust
   pub fn calculate_gas_cost(&self, gas_price: U256, native_price_usd: f64) -> f64 {
       let gas_cost_native = self.gas_estimate * gas_price;
       // Convert to USD
   }
   ```
5. Add gas volatility buffer during high volatility

**Files to Modify:**
- `crates/core/src/liquidator.rs` - Add real-time gas calculation
- `crates/chain/src/gas.rs` - Add gas price monitor
- `crates/core/src/config/bot.rs` - Add gas volatility config

**Testing:**
- Test gas cost calculation with varying gas prices
- Verify gas volatility buffer during spikes

---

### 2.3 Dynamic Profit Threshold Adjustment

**Priority:** MEDIUM  
**Effort:** Low (1-2 days)  
**Dependencies:** Win rate tracking

**Current Issue:**
- Fixed `min_profit_usd` threshold
- No adaptation based on win rate
- Strategy requires: `min_profit = base × (1.5 if win_rate < 40%)`

**Implementation:**
1. Track win rate (rolling 24h):
   ```rust
   pub struct WinRateTracker {
       wins: u64,
       attempts: u64,
       last_24h_wins: VecDeque<(Instant, bool)>,
   }
   ```
2. Calculate rolling win rate
3. Adjust `min_profit_usd` dynamically:
   ```rust
   pub fn adjusted_min_profit(&self, base: f64, win_rate: f64) -> f64 {
       let multiplier = match win_rate {
           r if r < 0.30 => 2.0,
           r if r < 0.40 => 1.5,
           r if r > 0.70 => 0.8,
           _ => 1.0,
       };
       base * multiplier
   }
   ```

**Files to Modify:**
- `crates/core/src/liquidator.rs` - Add win rate tracking
- `crates/core/src/win_rate.rs` - New file for win rate logic

**Testing:**
- Test threshold adjustment with various win rates
- Verify adaptive behavior

---

## Phase 3: Operating Modes and Traffic Management

### 3.1 Operating Modes System

**Priority:** HIGH  
**Effort:** Medium (3-4 days)  
**Dependencies:** Queue depth tracking, RPC health monitoring

**Current Issue:**
- No operating modes
- No adaptation to traffic spikes
- Strategy requires: NORMAL/ELEVATED/SPIKE/DEGRADED modes

**Implementation:**
1. Create `OperatingMode` enum:
   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   pub enum OperatingMode {
       Normal,
       Elevated,
       Spike,
       Degraded,
   }
   ```
2. Create `ModeManager`:
   ```rust
   pub struct ModeManager {
       current_mode: Arc<RwLock<OperatingMode>>,
       queue_depth: Arc<RwLock<usize>>,
       rpc_health: Arc<RwLock<f64>>,  // 0.0-1.0
       mode_entry_time: Arc<RwLock<Instant>>,
   }
   ```
3. Implement mode transition logic:
   ```rust
   pub fn update_mode(&self) {
       let queue = self.queue_depth.read().clone();
       let health = self.rpc_health.read().clone();
       let entry_time = self.mode_entry_time.read().elapsed();
       
       let new_mode = match (queue, health, self.current_mode.read().clone()) {
           (q, _, _) if q > 100 || health < 0.5 => OperatingMode::Spike,
           (q, _, _) if q > 20 => OperatingMode::Elevated,
           (q, h, _) if h < 0.5 => OperatingMode::Degraded,
           (_, _, m) if entry_time < Duration::from_secs(60) => m,  // Cooldown
           _ => OperatingMode::Normal,
       };
       
       if new_mode != *self.current_mode.read() {
           *self.current_mode.write() = new_mode;
           *self.mode_entry_time.write() = Instant::now();
       }
   }
   ```
4. Apply mode multipliers to profit thresholds and RPC allocation

**Files to Create:**
- `crates/core/src/mode_manager.rs` - New file

**Files to Modify:**
- `crates/core/src/scanner.rs` - Integrate mode manager
- `crates/core/src/config/bot.rs` - Add mode config

**Testing:**
- Test mode transitions with various queue depths
- Verify cooldown prevents oscillation

---

### 3.2 RPC Budget Allocation Strategy

**Priority:** MEDIUM  
**Effort:** Medium (2-3 days)  
**Dependencies:** Operating modes

**Current Issue:**
- No RPC budget tracking
- No allocation strategy
- Strategy requires: 30 req/s budget with dynamic allocation

**Implementation:**
1. Create `RpcBudgetManager`:
   ```rust
   pub struct RpcBudgetManager {
       total_budget: u64,  // req/s
       allocations: DashMap<BudgetCategory, u64>,
       current_usage: DashMap<BudgetCategory, u64>,
   }
   
   pub enum BudgetCategory {
       OracleEvents,
       EventPolling,
       CriticalTier,
       HotTier,
       WarmTier,
       GasPrice,
       Execution,
       Reserve,
   }
   ```
2. Allocate budget by mode:
   ```rust
   pub fn allocate_by_mode(&self, mode: OperatingMode) -> HashMap<BudgetCategory, u64> {
       match mode {
           OperatingMode::Normal => {
               // Balanced allocation
           }
           OperatingMode::Elevated => {
               // Shift toward execution
           }
           OperatingMode::Spike => {
               // Maximum execution
           }
           OperatingMode::Degraded => {
               // Minimal, essential only
           }
       }
   }
   ```
3. Track usage and enforce limits
4. Implement multicall batching to reduce RPC calls

**Files to Create:**
- `crates/core/src/rpc_budget.rs` - New file

**Files to Modify:**
- `crates/core/src/scanner.rs` - Integrate budget manager
- `crates/chain/src/provider.rs` - Add multicall batching

**Testing:**
- Test budget allocation per mode
- Verify multicall reduces RPC usage

---

## Phase 4: Multi-Collateral and Edge Cases

### 4.1 Multi-Collateral Composite Trigger Handling

**Priority:** MEDIUM  
**Effort:** High (4-5 days)  
**Dependencies:** Interval tree, volatility tracking

**Current Issue:**
- Only tracks primary collateral liquidation price
- Misses correlated price moves
- Strategy requires: Composite triggers for multi-collateral positions

**Implementation:**
1. Detect multi-collateral positions:
   ```rust
   pub fn is_multi_collateral(&self, position: &TrackedPosition) -> bool {
       position.collaterals.len() > 1
   }
   ```
2. Calculate composite trigger:
   ```rust
   pub struct CompositeTrigger {
       weights: Vec<(Address, f64)>,  // Asset -> weight
       threshold_drop: f64,
       correlation_matrix: Option<Matrix<f64>>,  // For >$50k positions
   }
   
   pub fn calculate_composite_trigger(position: &TrackedPosition) -> CompositeTrigger {
       let total_value = position.total_collateral_usd();
       let weights: Vec<_> = position.collaterals.iter()
           .map(|(asset, coll)| (*asset, coll.value_usd / total_value))
           .collect();
       
       // Calculate threshold drop needed
       // ...
   }
   ```
3. Index composite triggers separately
4. Monitor correlated price moves

**Files to Create:**
- `crates/core/src/composite_trigger.rs` - New file

**Files to Modify:**
- `crates/core/src/trigger_index.rs` - Add composite trigger support
- `crates/core/src/scanner.rs` - Handle composite triggers in oracle updates

**Testing:**
- Test composite trigger calculation
- Verify detection of correlated moves

---

### 4.2 LST Depeg Cascade Monitoring

**Priority:** MEDIUM  
**Effort:** Medium (2-3 days)  
**Dependencies:** DEX price monitoring

**Current Issue:**
- No LST depeg monitoring
- No special handling for stHYPE/HYPE ratio
- Strategy requires: Real-time DEX ratio monitoring

**Implementation:**
1. Add DEX price monitor for LST assets:
   ```rust
   pub struct LSTMonitor {
       lst_asset: Address,
       underlying_asset: Address,
       dex_ratio: Arc<RwLock<f64>>,
       oracle_ratio: Arc<RwLock<f64>>,
   }
   ```
2. Monitor DEX ratio every block
3. Flag positions when deviation > thresholds:
   ```rust
   pub fn check_depeg(&self) -> DepegStatus {
       let deviation = (self.dex_ratio - self.oracle_ratio).abs() / self.oracle_ratio;
       match deviation {
           d if d > 0.02 => DepegStatus::Critical,  // 2%
           d if d > 0.01 => DepegStatus::Warning,  // 1%
           d if d > 0.005 => DepegStatus::Caution,  // 0.5%
           _ => DepegStatus::Normal,
       }
   }
   ```
4. Apply confidence penalty and higher profit threshold for depegged positions

**Files to Create:**
- `crates/core/src/lst_monitor.rs` - New file

**Files to Modify:**
- `crates/core/src/scanner.rs` - Integrate LST monitoring
- `crates/core/src/liquidator.rs` - Apply depeg penalties

**Testing:**
- Test depeg detection
- Verify confidence penalty application

---

### 4.3 Partial Liquidation Optimization

**Priority:** LOW  
**Effort:** Medium (2-3 days)  
**Dependencies:** Slippage estimation

**Current Issue:**
- Uses fixed close factor (50%)
- No optimization for optimal liquidation size
- Strategy requires: Find size that maximizes net profit

**Implementation:**
1. Add optimal size calculation:
   ```rust
   pub fn find_optimal_liquidation_size(
       position: &TrackedPosition,
       liquidity_depth: &LiquidityDepth,
   ) -> U256 {
       let max_size = position.debt.amount * close_factor;
       let mut best_size = max_size;
       let mut best_profit = f64::NEG_INFINITY;
       
       // Test sizes: 10%, 20%, ..., 100% of max
       for pct in (10..=100).step_by(10) {
           let size = max_size * pct / 100;
           let slippage = estimate_slippage(size, liquidity_depth);
           let profit = calculate_profit(size, slippage);
           if profit > best_profit {
               best_profit = profit;
               best_size = size;
           }
       }
       
       best_size
   }
   ```

**Files to Modify:**
- `crates/core/src/liquidator.rs` - Add optimal size calculation
- `crates/api/src/swap/mod.rs` - Add liquidity depth estimation

**Testing:**
- Test optimal size calculation with various liquidity conditions
- Verify profit maximization

---

### 4.4 Bad Debt Race Handling

**Priority:** MEDIUM  
**Effort:** Low (1-2 days)  
**Dependencies:** None

**Current Issue:**
- Skips bad debt positions entirely
- Strategy requires: Race for first liquidation if profitable

**Implementation:**
1. Detect bad debt (HF < 1.0):
   ```rust
   pub fn is_bad_debt(&self, position: &TrackedPosition) -> bool {
       position.health_factor < 1.0
   }
   ```
2. Calculate max profitable debt:
   ```rust
   pub fn max_profitable_debt(&self, position: &TrackedPosition) -> f64 {
       let collateral_value = position.total_collateral_usd();
       collateral_value / (1.0 + liquidation_bonus)
   }
   ```
3. If profitable, reduce liquidation to max profitable amount
4. Set urgency to CRITICAL regardless of HF

**Files to Modify:**
- `crates/core/src/liquidator.rs` - Add bad debt handling
- `crates/core/src/scanner.rs` - Prioritize bad debt positions

**Testing:**
- Test bad debt detection and handling
- Verify profitability calculation

---

## Phase 5: Historical Seeding and Lifecycle

### 5.1 Historical Position Seeding

**Priority:** HIGH  
**Effort:** Medium (3-4 days)  
**Dependencies:** Position fetching, multicall

**Current Issue:**
- Positions only discovered via events
- Cold start means missing first wave of liquidations
- Strategy requires: Scan ALL positions with debt on startup

**Implementation:**
1. Add historical scanner:
   ```rust
   pub struct HistoricalScanner {
       provider: Arc<ProviderManager>,
       assets: Arc<AssetRegistry>,
   }
   
   impl HistoricalScanner {
       pub async fn scan_all_positions(&self) -> Result<Vec<TrackedPosition>> {
           // 1. Get all users with debt (from pool contract)
           // 2. Batch fetch positions via multicall
           // 3. Calculate liquidation prices
           // 4. Build interval tree index
       }
   }
   ```
2. Integrate into bootstrap:
   ```rust
   pub async fn bootstrap(&self) -> Result<()> {
       // 1. Historical scan
       let positions = self.historical_scanner.scan_all_positions().await?;
       for pos in positions {
           self.tracker.upsert(pos);
       }
       
       // 2. Rebuild trigger index
       self.tracker.rebuild_trigger_index();
       
       // 3. Pre-stage critical positions
       // ...
   }
   ```

**Files to Create:**
- `crates/core/src/historical_scanner.rs` - New file

**Files to Modify:**
- `crates/core/src/scanner.rs` - Integrate historical scanning
- `crates/chain/src/provider.rs` - Add batch position fetching

**Testing:**
- Test historical scanning with large position sets
- Verify bootstrap completes in < 60 seconds

---

### 5.2 Position Archiving and Lifecycle

**Priority:** LOW  
**Effort:** Medium (2-3 days)  
**Dependencies:** None

**Current Issue:**
- Positions removed when debt = 0
- No archiving for later reload
- Strategy requires: Archive to disk, reload on events

**Implementation:**
1. Add position archive:
   ```rust
   pub struct PositionArchive {
       archive_dir: PathBuf,
   }
   
   impl PositionArchive {
       pub fn archive(&self, position: &TrackedPosition) -> Result<()> {
           let path = self.archive_path(&position.user);
           let json = serde_json::to_string(position)?;
           std::fs::write(path, json)?;
           Ok(())
       }
       
       pub fn load(&self, user: &Address) -> Result<Option<TrackedPosition>> {
           let path = self.archive_path(user);
           if path.exists() {
               let json = std::fs::read_to_string(path)?;
               Ok(Some(serde_json::from_str(&json)?))
           } else {
               Ok(None)
           }
       }
   }
   ```
2. Archive positions when moved to FROZEN tier
3. Reload from archive on relevant events

**Files to Create:**
- `crates/core/src/archive.rs` - New file

**Files to Modify:**
- `crates/core/src/position_tracker.rs` - Add archiving on tier transitions
- `crates/core/src/scanner.rs` - Reload from archive on events

**Testing:**
- Test archiving and loading
- Verify archive cleanup

---

## Phase 6: Competitor Analysis and Intelligence

### 6.1 Competitor Fingerprinting

**Priority:** LOW  
**Effort:** Medium (3-4 days)  
**Dependencies:** Liquidation event monitoring

**Current Issue:**
- No competitor tracking
- No strategic response
- Strategy requires: Fingerprint competitors, find niches

**Implementation:**
1. Track competitor liquidations:
   ```rust
   pub struct CompetitorProfile {
       address: Address,
       position_size_range: (f64, f64),
       avg_latency_ms: f64,
       strategy: ExecutionStrategy,
       win_rate_against_us: f64,
       asset_preferences: HashSet<Address>,
   }
   ```
2. Analyze liquidation events:
   ```rust
   pub fn analyze_competitor_liquidation(
       &self,
       event: &LiquidationCall,
   ) -> CompetitorProfile {
       // Extract: size, latency, strategy, assets
   }
   ```
3. Identify niches (size ranges, assets, time periods)
4. Adjust strategy based on competitor activity

**Files to Create:**
- `crates/core/src/competitor.rs` - New file

**Files to Modify:**
- `crates/core/src/scanner.rs` - Track competitor liquidations
- `crates/core/src/liquidator.rs` - Adjust strategy based on competitors

**Testing:**
- Test competitor fingerprinting
- Verify niche identification

---

## Phase 7: Monitoring and Observability

### 7.1 Enhanced Metrics and Dashboard

**Priority:** MEDIUM  
**Effort:** Medium (2-3 days)  
**Dependencies:** All previous phases

**Implementation:**
1. Add metrics collection:
   ```rust
   pub struct BotMetrics {
       current_mode: OperatingMode,
       queue_depth: usize,
       win_rate_1h: f64,
       win_rate_24h: f64,
       avg_latency_ms: f64,
       rpc_health: f64,
       gas_price_trend: GasTrend,
       capital_status: CapitalStatus,
   }
   ```
2. Export metrics (Prometheus format or custom)
3. Create dashboard (optional: Grafana)

**Files to Create:**
- `crates/core/src/metrics.rs` - New file

**Files to Modify:**
- `crates/core/src/scanner.rs` - Collect metrics
- `src/main.rs` - Expose metrics endpoint

**Testing:**
- Test metrics collection
- Verify dashboard updates

---

### 7.2 Comprehensive Logging

**Priority:** MEDIUM  
**Effort:** Low (1-2 days)  
**Dependencies:** None

**Implementation:**
1. Add structured logging for every liquidation attempt:
   ```rust
   info!(
       event = "liquidation_attempt",
       user = %user,
       hf = position.health_factor,
       oracle_price = oracle_price,
       gas_price = gas_price,
       estimated_profit = profit,
       actual_profit = actual,
       competitor_tx = competitor_hash,
       latency_ms = latency,
   );
   ```
2. Log post-analysis: why won/lost, profit accuracy, speed

**Files to Modify:**
- `crates/core/src/liquidator.rs` - Add detailed logging
- `crates/core/src/scanner.rs` - Add event logging

**Testing:**
- Verify log format
- Test log analysis

---

## Implementation Timeline

**Phase 1 (Core Infrastructure):** 2-3 weeks
- Interval tree
- Dynamic thresholds
- Hysteresis

**Phase 2 (Profitability):** 1-2 weeks
- EV-based scoring
- Real-time gas
- Dynamic thresholds

**Phase 3 (Operating Modes):** 1-2 weeks
- Mode system
- RPC budget

**Phase 4 (Edge Cases):** 2-3 weeks
- Multi-collateral
- LST monitoring
- Partial liquidation
- Bad debt

**Phase 5 (Lifecycle):** 1 week
- Historical seeding
- Archiving

**Phase 6 (Competitors):** 1 week
- Fingerprinting

**Phase 7 (Monitoring):** 1 week
- Metrics
- Logging

**Total Estimated Time:** 9-14 weeks

---

## Risk Mitigation

1. **Performance Regression:**
   - Benchmark before/after each phase
   - Use feature flags for gradual rollout

2. **Complexity:**
   - Implement phases incrementally
   - Maintain backward compatibility

3. **Testing:**
   - Unit tests for each component
   - Integration tests for critical paths
   - Fork tests for real-world scenarios

4. **Rollback Plan:**
   - Feature flags for each major feature
   - Ability to disable new features if issues arise

---

## Success Metrics

1. **Latency:** < 10ms from oracle update to tx broadcast
2. **Win Rate:** > 60% in normal conditions
3. **Coverage:** 100% of positions with debt tracked
4. **Efficiency:** < 5 RPC calls per cycle (vs current ~100)
5. **Profitability:** Positive EV on > 80% of attempts

---

## Next Steps

1. Review and prioritize phases
2. Set up development environment
3. Create feature branches for each phase
4. Begin Phase 1 implementation
5. Regular progress reviews and adjustments

