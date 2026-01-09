# Architecture Specifications: Algorithm Abstraction Framework

## Executive Summary

This document specifies the architectural framework for making liquidation bot algorithms pluggable, extensible, and compatible across multiple chains and protocols. The design enables algorithm swapping, A/B testing, and protocol-specific optimizations while maintaining a unified interface.

**Design Principles:**
1. **Trait-Based Abstractions**: All algorithms implement well-defined traits
2. **Dependency Injection**: Algorithms receive dependencies, not create them
3. **Chain/Protocol Agnostic**: Core algorithms work across all chains/protocols
4. **Composition Over Inheritance**: Combine small algorithms into strategies
5. **Configuration-Driven**: Algorithm selection via config, not code changes
6. **Backward Compatible**: New algorithms don't break existing deployments

---

## 1. Core Abstractions

### 1.1 Algorithm Trait Hierarchy

```rust
//! Core algorithm traits for pluggable liquidation strategies

/// Base trait for all algorithms
pub trait Algorithm: Send + Sync + Debug {
    /// Algorithm identifier (e.g., "interval_tree_index", "ev_scorer_v1")
    fn id(&self) -> &'static str;
    
    /// Algorithm version for compatibility checking
    fn version(&self) -> AlgorithmVersion;
    
    /// Check if algorithm is compatible with given chain/protocol
    fn is_compatible(&self, chain_id: u64, protocol: ProtocolVersion) -> bool;
}

/// Algorithm version for compatibility tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AlgorithmVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl AlgorithmVersion {
    pub fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self { major, minor, patch }
    }
    
    /// Check if versions are compatible (same major version)
    pub fn is_compatible(&self, other: &Self) -> bool {
        self.major == other.major
    }
}
```

### 1.2 Indexing Algorithm Trait

```rust
/// Trait for position indexing algorithms (e.g., interval tree, linear search)
#[async_trait]
pub trait IndexingAlgorithm: Algorithm {
    /// Index a position with its liquidation trigger prices
    async fn index_position(
        &self,
        position: &TrackedPosition,
        context: &IndexContext,
    ) -> Result<()>;
    
    /// Query positions that become liquidatable at a given price
    async fn query_liquidatable(
        &self,
        asset: Address,
        price_range: PriceRange,
        context: &QueryContext,
    ) -> Result<Vec<Address>>;
    
    /// Remove a position from the index
    async fn remove_position(&self, user: Address) -> Result<()>;
    
    /// Rebuild the entire index (for bootstrap)
    async fn rebuild_index(
        &self,
        positions: &[Arc<TrackedPosition>],
        context: &IndexContext,
    ) -> Result<()>;
    
    /// Get index statistics
    fn stats(&self) -> IndexStats;
}

/// Context passed to indexing algorithms
pub struct IndexContext {
    pub chain_id: u64,
    pub protocol: ProtocolVersion,
    pub asset_registry: Arc<AssetRegistry>,
    pub price_cache: Arc<PriceCache>,
}

/// Context for query operations
pub struct QueryContext {
    pub chain_id: u64,
    pub protocol: ProtocolVersion,
    pub current_prices: HashMap<Address, U256>,
}

/// Price range for queries
#[derive(Debug, Clone)]
pub struct PriceRange {
    pub min: U256,
    pub max: U256,
}

impl PriceRange {
    pub fn point(price: U256) -> Self {
        Self { min: price, max: price }
    }
    
    pub fn from_old_new(old: U256, new: U256) -> Self {
        Self {
            min: old.min(new),
            max: old.max(new),
        }
    }
}
```

### 1.3 Scoring Algorithm Trait

```rust
/// Trait for priority scoring algorithms (e.g., EV-based, profit-based)
#[async_trait]
pub trait ScoringAlgorithm: Algorithm {
    /// Score a liquidation opportunity
    async fn score(
        &self,
        position: &TrackedPosition,
        context: &ScoringContext,
    ) -> Result<LiquidationScore>;
    
    /// Batch score multiple positions (for efficiency)
    async fn score_batch(
        &self,
        positions: &[Arc<TrackedPosition>],
        context: &ScoringContext,
    ) -> Result<Vec<LiquidationScore>>;
    
    /// Check if a position meets minimum score threshold
    fn meets_threshold(&self, score: &LiquidationScore) -> bool;
}

/// Context for scoring operations
pub struct ScoringContext {
    pub chain_id: u64,
    pub protocol: ProtocolVersion,
    pub current_gas_price: U256,
    pub gas_price_trend: GasTrend,
    pub win_rate_tracker: Arc<WinRateTracker>,
    pub competitor_profiles: Arc<CompetitorRegistry>,
    pub operating_mode: OperatingMode,
    pub asset_registry: Arc<AssetRegistry>,
    pub price_cache: Arc<PriceCache>,
}

/// Liquidation opportunity score
#[derive(Debug, Clone)]
pub struct LiquidationScore {
    /// Expected value (primary sort key)
    pub expected_value: f64,
    /// Win probability (0.0-1.0)
    pub win_probability: f64,
    /// Estimated profit (USD)
    pub estimated_profit: f64,
    /// Estimated gas cost (USD)
    pub estimated_gas: f64,
    /// Urgency level
    pub urgency: UrgencyLevel,
    /// Confidence in the score (0.0-1.0)
    pub confidence: f64,
    /// Algorithm-specific metadata
    pub metadata: HashMap<String, String>,
}

/// Urgency levels for prioritization
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UrgencyLevel {
    Critical = 4,  // HF < 1.005
    High = 3,      // HF < 1.02
    Medium = 2,    // HF < 1.05
    Low = 1,       // HF >= 1.05
}
```

### 1.4 Tier Classification Algorithm Trait

```rust
/// Trait for position tier classification algorithms
pub trait TierClassificationAlgorithm: Algorithm {
    /// Classify a position into a tier
    fn classify(
        &self,
        position: &TrackedPosition,
        context: &TierContext,
    ) -> PositionTier;
    
    /// Check if tier transition is allowed (hysteresis)
    fn can_transition(
        &self,
        from: PositionTier,
        to: PositionTier,
        position: &TrackedPosition,
        context: &TierContext,
    ) -> bool;
}

/// Context for tier classification
pub struct TierContext {
    pub chain_id: u64,
    pub protocol: ProtocolVersion,
    pub asset_volatility: Arc<VolatilityTracker>,
    pub block_time_ms: u64,
    pub tier_entry_times: Arc<DashMap<Address, Instant>>,
}
```

### 1.5 Execution Strategy Algorithm Trait

```rust
/// Trait for execution strategy selection (flash loan, inventory, etc.)
#[async_trait]
pub trait ExecutionStrategyAlgorithm: Algorithm {
    /// Select the best execution strategy for a liquidation
    async fn select_strategy(
        &self,
        position: &TrackedPosition,
        context: &ExecutionContext,
    ) -> Result<ExecutionStrategy>;
    
    /// Get all available strategies for a position
    async fn available_strategies(
        &self,
        position: &TrackedPosition,
        context: &ExecutionContext,
    ) -> Result<Vec<ExecutionStrategy>>;
}

/// Execution context
pub struct ExecutionContext {
    pub chain_id: u64,
    pub protocol: ProtocolVersion,
    pub liquidity_depth: Arc<LiquidityDepthCache>,
    pub inventory: Arc<InventoryManager>,
    pub gas_price: U256,
    pub operating_mode: OperatingMode,
    pub router_registry: Arc<SwapRouterRegistry>,
}

/// Execution strategy
#[derive(Debug, Clone)]
pub enum ExecutionStrategy {
    FlashLoanDex {
        provider: FlashLoanProvider,
        router: SwapRouter,
    },
    Inventory {
        required_asset: Address,
        required_amount: U256,
    },
    BridgeOrderbook {
        bridge: BridgeProvider,
        orderbook: OrderbookProvider,
    },
    Chunked {
        chunk_size: U256,
        num_chunks: u8,
    },
}
```

---

## 2. Chain Abstraction Layer

### 2.1 Chain Provider Trait

```rust
/// Trait for chain-specific operations
#[async_trait]
pub trait ChainProvider: Send + Sync + Debug {
    /// Chain ID
    fn chain_id(&self) -> u64;
    
    /// Chain name (e.g., "ethereum", "arbitrum")
    fn chain_name(&self) -> &str;
    
    /// Average block time in milliseconds
    fn block_time_ms(&self) -> u64;
    
    /// Native token address (WETH, WAVAX, etc.)
    fn native_token(&self) -> Address;
    
    /// Get RPC provider manager
    fn provider_manager(&self) -> Arc<ProviderManager>;
    
    /// Get gas price strategy
    fn gas_strategy(&self) -> Arc<dyn GasStrategy>;
    
    /// Get swap router registry for this chain
    fn swap_routers(&self) -> Arc<SwapRouterRegistry>;
    
    /// Check if chain supports a feature
    fn supports_feature(&self, feature: ChainFeature) -> bool;
}

/// Chain features
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainFeature {
    Eip1559,           // EIP-1559 gas pricing
    FlashLoans,        // Native flash loan support
    Multicall,         // Multicall contract available
    ArchiveNodes,      // Archive node access
    CustomGasToken,    // Custom gas token (not ETH)
}
```

### 2.2 Chain Factory

```rust
/// Factory for creating chain providers
pub struct ChainFactory;

impl ChainFactory {
    /// Create a chain provider from configuration
    pub fn create(config: &ChainConfig) -> Result<Arc<dyn ChainProvider>> {
        match config.chain_id {
            1 => Ok(Arc::new(EthereumProvider::new(config)?)),
            42161 => Ok(Arc::new(ArbitrumProvider::new(config)?)),
            10 => Ok(Arc::new(OptimismProvider::new(config)?)),
            8453 => Ok(Arc::new(BaseProvider::new(config)?)),
            999 => Ok(Arc::new(HyperliquidProvider::new(config)?)),
            _ => Err(anyhow::anyhow!("Unsupported chain: {}", config.chain_id)),
        }
    }
    
    /// Get default configuration for a chain
    pub fn default_config(chain_id: u64) -> Option<ChainConfig> {
        match chain_id {
            1 => Some(ChainConfig::ethereum()),
            42161 => Some(ChainConfig::arbitrum()),
            // ...
            _ => None,
        }
    }
}
```

---

## 3. Protocol Abstraction Layer

### 3.1 Protocol Adapter Pattern

```rust
/// Protocol adapter that wraps protocol-specific implementations
pub struct ProtocolAdapter {
    inner: Arc<dyn LiquidatableProtocol>,
    chain_id: u64,
    protocol_version: ProtocolVersion,
}

impl ProtocolAdapter {
    /// Create adapter from protocol implementation
    pub fn new(protocol: Arc<dyn LiquidatableProtocol>) -> Self {
        Self {
            chain_id: protocol.chain_id(),
            protocol_version: protocol.version(),
            inner: protocol,
        }
    }
    
    /// Get unified position data (normalized across protocols)
    pub async fn get_unified_position(&self, user: Address) -> Result<UnifiedPosition> {
        let position = self.inner.get_position(user).await?;
        self.normalize_position(position)
    }
    
    /// Normalize protocol-specific position to unified format
    fn normalize_position(&self, position: PositionData) -> UnifiedPosition {
        UnifiedPosition {
            user: position.user,
            collaterals: position.collaterals.into_iter()
                .map(|c| self.normalize_collateral(c))
                .collect(),
            debts: position.debts.into_iter()
                .map(|d| self.normalize_debt(d))
                .collect(),
            health_factor: position.health_factor,
            // Protocol-specific fields normalized
            liquidation_threshold: self.inner.liquidation_threshold(),
            close_factor: self.inner.close_factor(),
        }
    }
}

/// Unified position format (works across all protocols)
#[derive(Debug, Clone)]
pub struct UnifiedPosition {
    pub user: Address,
    pub collaterals: Vec<UnifiedCollateral>,
    pub debts: Vec<UnifiedDebt>,
    pub health_factor: f64,
    pub liquidation_threshold: f64,
    pub close_factor: f64,
}
```

### 3.2 Protocol Factory

```rust
/// Factory for creating protocol adapters
pub struct ProtocolFactory;

impl ProtocolFactory {
    /// Create protocol adapter from configuration
    pub async fn create(
        config: &ProtocolConfig,
        chain_provider: Arc<dyn ChainProvider>,
    ) -> Result<ProtocolAdapter> {
        let protocol = match config.version {
            ProtocolVersion::AaveV3 => {
                AaveV3Protocol::new(config, chain_provider.provider_manager()).await?
            }
            ProtocolVersion::AaveV4 => {
                AaveV4Protocol::new(config, chain_provider.provider_manager()).await?
            }
            ProtocolVersion::CompoundV3 => {
                CompoundV3Protocol::new(config, chain_provider.provider_manager()).await?
            }
            ProtocolVersion::Custom => {
                return Err(anyhow::anyhow!("Custom protocols require custom implementation"));
            }
        };
        
        Ok(ProtocolAdapter::new(Arc::new(protocol)))
    }
}
```

---

## 4. Algorithm Registry and Factory

### 4.1 Algorithm Registry

```rust
/// Registry for algorithm implementations
pub struct AlgorithmRegistry {
    indexing: HashMap<String, AlgorithmFactory<dyn IndexingAlgorithm>>,
    scoring: HashMap<String, AlgorithmFactory<dyn ScoringAlgorithm>>,
    tier_classification: HashMap<String, AlgorithmFactory<dyn TierClassificationAlgorithm>>,
    execution_strategy: HashMap<String, AlgorithmFactory<dyn ExecutionStrategyAlgorithm>>,
}

impl AlgorithmRegistry {
    /// Create new registry
    pub fn new() -> Self {
        Self {
            indexing: HashMap::new(),
            scoring: HashMap::new(),
            tier_classification: HashMap::new(),
            execution_strategy: HashMap::new(),
        }
    }
    
    /// Register an indexing algorithm
    pub fn register_indexing<A: IndexingAlgorithm + 'static>(
        &mut self,
        id: &str,
        factory: fn(AlgorithmConfig) -> Result<A>,
    ) {
        self.indexing.insert(id.to_string(), Box::new(factory));
    }
    
    /// Get indexing algorithm by ID
    pub fn get_indexing(
        &self,
        id: &str,
        config: AlgorithmConfig,
    ) -> Result<Arc<dyn IndexingAlgorithm>> {
        let factory = self.indexing.get(id)
            .ok_or_else(|| anyhow::anyhow!("Unknown indexing algorithm: {}", id))?;
        Ok(Arc::new(factory(config)?))
    }
    
    /// List all registered algorithms
    pub fn list_indexing(&self) -> Vec<String> {
        self.indexing.keys().cloned().collect()
    }
}

/// Algorithm factory function type
type AlgorithmFactory<T> = Box<dyn Fn(AlgorithmConfig) -> Result<Box<T>> + Send + Sync>;

/// Algorithm configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AlgorithmConfig {
    pub id: String,
    pub version: String,
    pub parameters: HashMap<String, serde_json::Value>,
    pub chain_id: Option<u64>,
    pub protocol: Option<ProtocolVersion>,
}
```

### 4.2 Default Algorithm Implementations

```rust
/// Register default algorithms
pub fn register_default_algorithms(registry: &mut AlgorithmRegistry) {
    // Indexing algorithms
    registry.register_indexing("linear_search", |config| {
        Ok(Box::new(LinearSearchIndex::new(config)?))
    });
    registry.register_indexing("interval_tree", |config| {
        Ok(Box::new(IntervalTreeIndex::new(config)?))
    });
    registry.register_indexing("b_tree", |config| {
        Ok(Box::new(BTreeIndex::new(config)?))
    });
    
    // Scoring algorithms
    registry.register_scoring("profit_based", |config| {
        Ok(Box::new(ProfitBasedScorer::new(config)?))
    });
    registry.register_scoring("ev_based", |config| {
        Ok(Box::new(EvBasedScorer::new(config)?))
    });
    registry.register_scoring("ml_based", |config| {
        Ok(Box::new(MlBasedScorer::new(config)?))
    });
    
    // Tier classification algorithms
    registry.register_tier_classification("fixed_threshold", |config| {
        Ok(Box::new(FixedThresholdTier::new(config)?))
    });
    registry.register_tier_classification("dynamic_volatility", |config| {
        Ok(Box::new(DynamicVolatilityTier::new(config)?))
    });
    
    // Execution strategy algorithms
    registry.register_execution_strategy("greedy", |config| {
        Ok(Box::new(GreedyExecutionStrategy::new(config)?))
    });
    registry.register_execution_strategy("optimal", |config| {
        Ok(Box::new(OptimalExecutionStrategy::new(config)?))
    });
}
```

---

## 5. Configuration-Driven Algorithm Selection

### 5.1 Algorithm Configuration Schema

```toml
# config/algorithms.toml

[algorithms]
# Indexing algorithm
indexing = { id = "interval_tree", version = "1.0.0" }

# Scoring algorithm
scoring = { id = "ev_based", version = "2.1.0" }

# Tier classification algorithm
tier_classification = { id = "dynamic_volatility", version = "1.0.0" }

# Execution strategy algorithm
execution_strategy = { id = "optimal", version = "1.0.0" }

# Algorithm-specific parameters
[algorithms.indexing.parameters]
max_positions = 10000
rebuild_interval_secs = 300

[algorithms.scoring.parameters]
min_win_probability = 0.3
competitor_factor_weight = 0.2
gas_volatility_buffer = 1.5

[algorithms.tier_classification.parameters]
volatility_lookback_blocks = 100
safety_margin = 1.5
hysteresis_cooldown_secs = 60

[algorithms.execution_strategy.parameters]
max_chunk_size_usd = 200000
min_liquidity_ratio = 2.0
```

### 5.2 Runtime Algorithm Selection

```rust
/// Algorithm manager that loads and manages algorithms
pub struct AlgorithmManager {
    registry: Arc<AlgorithmRegistry>,
    indexing: Arc<dyn IndexingAlgorithm>,
    scoring: Arc<dyn ScoringAlgorithm>,
    tier_classification: Arc<dyn TierClassificationAlgorithm>,
    execution_strategy: Arc<dyn ExecutionStrategyAlgorithm>,
}

impl AlgorithmManager {
    /// Create from configuration
    pub fn from_config(
        config: &AlgorithmConfig,
        registry: Arc<AlgorithmRegistry>,
    ) -> Result<Self> {
        let indexing = registry.get_indexing(
            &config.indexing.id,
            config.indexing.clone(),
        )?;
        
        let scoring = registry.get_scoring(
            &config.scoring.id,
            config.scoring.clone(),
        )?;
        
        let tier_classification = registry.get_tier_classification(
            &config.tier_classification.id,
            config.tier_classification.clone(),
        )?;
        
        let execution_strategy = registry.get_execution_strategy(
            &config.execution_strategy.id,
            config.execution_strategy.clone(),
        )?;
        
        Ok(Self {
            registry,
            indexing,
            scoring,
            tier_classification,
            execution_strategy,
        })
    }
    
    /// Hot-swap algorithm at runtime (for A/B testing)
    pub fn swap_indexing(
        &mut self,
        new_id: &str,
        config: AlgorithmConfig,
    ) -> Result<()> {
        let new_algorithm = self.registry.get_indexing(new_id, config)?;
        // Verify compatibility
        if !new_algorithm.is_compatible(self.chain_id, self.protocol) {
            return Err(anyhow::anyhow!("Algorithm not compatible"));
        }
        self.indexing = new_algorithm;
        Ok(())
    }
}
```

---

## 6. Compatibility Guarantees

### 6.1 Algorithm Compatibility Matrix

```rust
/// Algorithm compatibility checker
pub struct CompatibilityChecker;

impl CompatibilityChecker {
    /// Check if algorithm is compatible with chain/protocol
    pub fn check(
        algorithm: &dyn Algorithm,
        chain_id: u64,
        protocol: ProtocolVersion,
    ) -> CompatibilityResult {
        // Check explicit compatibility
        if !algorithm.is_compatible(chain_id, protocol) {
            return CompatibilityResult::Incompatible {
                reason: "Algorithm explicitly marked as incompatible".to_string(),
            };
        }
        
        // Check feature requirements
        let required_features = algorithm.required_features();
        let chain_features = get_chain_features(chain_id);
        
        for feature in required_features {
            if !chain_features.contains(&feature) {
                return CompatibilityResult::Incompatible {
                    reason: format!("Missing required feature: {:?}", feature),
                };
            }
        }
        
        CompatibilityResult::Compatible
    }
}

/// Compatibility result
#[derive(Debug)]
pub enum CompatibilityResult {
    Compatible,
    Incompatible { reason: String },
    CompatibleWithWarnings { warnings: Vec<String> },
}
```

### 6.2 Version Compatibility Rules

```rust
/// Version compatibility rules
pub struct VersionCompatibility;

impl VersionCompatibility {
    /// Check if algorithm versions are compatible
    pub fn check(
        algorithm_version: AlgorithmVersion,
        config_version: AlgorithmVersion,
    ) -> bool {
        // Same major version = compatible
        if algorithm_version.major == config_version.major {
            return true;
        }
        
        // Different major version = incompatible
        false
    }
    
    /// Get migration path between versions
    pub fn migration_path(
        from: AlgorithmVersion,
        to: AlgorithmVersion,
    ) -> Option<MigrationPath> {
        if from.major == to.major {
            // Minor/patch update, no migration needed
            return None;
        }
        
        // Major version change requires migration
        Some(MigrationPath {
            from,
            to,
            steps: vec![
                MigrationStep::BackupData,
                MigrationStep::UpgradeAlgorithm,
                MigrationStep::RebuildIndex,
                MigrationStep::ValidateResults,
            ],
        })
    }
}
```

---

## 7. Extension Points

### 7.1 Custom Algorithm Implementation

```rust
/// Example: Custom indexing algorithm
pub struct CustomIntervalTreeIndex {
    trees: DashMap<Address, IntervalTree<U256, Vec<Address>>>,
    config: AlgorithmConfig,
}

impl Algorithm for CustomIntervalTreeIndex {
    fn id(&self) -> &'static str {
        "custom_interval_tree"
    }
    
    fn version(&self) -> AlgorithmVersion {
        AlgorithmVersion::new(1, 0, 0)
    }
    
    fn is_compatible(&self, chain_id: u64, protocol: ProtocolVersion) -> bool {
        // Works on all chains and protocols
        true
    }
}

#[async_trait]
impl IndexingAlgorithm for CustomIntervalTreeIndex {
    async fn index_position(
        &self,
        position: &TrackedPosition,
        context: &IndexContext,
    ) -> Result<()> {
        // Custom implementation
        // ...
        Ok(())
    }
    
    // ... implement other methods
}
```

### 7.2 Protocol-Specific Algorithm Override

```rust
/// Protocol-specific algorithm override
pub struct ProtocolSpecificScorer {
    base_scorer: Arc<dyn ScoringAlgorithm>,
    protocol_overrides: HashMap<ProtocolVersion, Arc<dyn ScoringAlgorithm>>,
}

impl ProtocolSpecificScorer {
    /// Get scorer for protocol (with override if available)
    fn get_scorer(&self, protocol: ProtocolVersion) -> Arc<dyn ScoringAlgorithm> {
        self.protocol_overrides
            .get(&protocol)
            .cloned()
            .unwrap_or_else(|| self.base_scorer.clone())
    }
}
```

### 7.3 Chain-Specific Optimizations

```rust
/// Chain-specific algorithm adapter
pub struct ChainOptimizedIndex {
    base: Arc<dyn IndexingAlgorithm>,
    chain_optimizations: HashMap<u64, ChainOptimization>,
}

impl ChainOptimizedIndex {
    /// Apply chain-specific optimizations
    fn optimize_for_chain(&self, chain_id: u64) -> Result<()> {
        if let Some(optimization) = self.chain_optimizations.get(&chain_id) {
            match optimization {
                ChainOptimization::GasOptimized => {
                    // Use more gas-efficient data structures
                }
                ChainOptimization::StorageOptimized => {
                    // Use more storage-efficient data structures
                }
            }
        }
        Ok(())
    }
}
```

---

## 8. Testing and Validation

### 8.1 Algorithm Test Framework

```rust
/// Test framework for algorithms
pub struct AlgorithmTestFramework;

impl AlgorithmTestFramework {
    /// Test algorithm compatibility
    pub fn test_compatibility(
        algorithm: &dyn Algorithm,
        chains: &[u64],
        protocols: &[ProtocolVersion],
    ) -> TestResults {
        let mut results = TestResults::new();
        
        for chain_id in chains {
            for protocol in protocols {
                let result = CompatibilityChecker::check(algorithm, *chain_id, *protocol);
                results.add_result(*chain_id, *protocol, result);
            }
        }
        
        results
    }
    
    /// Test algorithm correctness
    pub async fn test_correctness(
        algorithm: &dyn IndexingAlgorithm,
        test_data: &[TestPosition],
    ) -> CorrectnessResults {
        // Test indexing
        for position in test_data {
            algorithm.index_position(position, &test_context()).await?;
        }
        
        // Test queries
        for query in test_queries() {
            let results = algorithm.query_liquidatable(
                query.asset,
                query.range,
                &test_context(),
            ).await?;
            
            // Verify results match expected
        }
        
        CorrectnessResults::new()
    }
}
```

### 8.2 Performance Benchmarking

```rust
/// Performance benchmark for algorithms
pub struct AlgorithmBenchmark;

impl AlgorithmBenchmark {
    /// Benchmark indexing performance
    pub async fn benchmark_indexing(
        algorithm: &dyn IndexingAlgorithm,
        num_positions: usize,
    ) -> BenchmarkResults {
        let positions = generate_test_positions(num_positions);
        
        let start = Instant::now();
        for position in &positions {
            algorithm.index_position(position, &test_context()).await?;
        }
        let index_time = start.elapsed();
        
        let start = Instant::now();
        for _ in 0..100 {
            algorithm.query_liquidatable(
                test_asset(),
                test_range(),
                &test_context(),
            ).await?;
        }
        let query_time = start.elapsed();
        
        BenchmarkResults {
            index_time,
            query_time,
            memory_usage: get_memory_usage(),
        }
    }
}
```

---

## 9. Migration and Upgrade Path

### 9.1 Algorithm Migration

```rust
/// Algorithm migration manager
pub struct AlgorithmMigration;

impl AlgorithmMigration {
    /// Migrate from one algorithm to another
    pub async fn migrate(
        from: Arc<dyn IndexingAlgorithm>,
        to: Arc<dyn IndexingAlgorithm>,
        positions: &[Arc<TrackedPosition>],
    ) -> Result<MigrationResult> {
        // 1. Export data from old algorithm
        let exported = from.export_data().await?;
        
        // 2. Initialize new algorithm
        to.initialize(exported.metadata).await?;
        
        // 3. Rebuild index with new algorithm
        to.rebuild_index(positions, &test_context()).await?;
        
        // 4. Validate migration
        let validation = validate_migration(from, to, positions).await?;
        
        if !validation.is_valid {
            return Err(anyhow::anyhow!("Migration validation failed"));
        }
        
        Ok(MigrationResult {
            positions_migrated: positions.len(),
            validation,
        })
    }
}
```

### 9.2 Rolling Algorithm Updates

```rust
/// Rolling algorithm update (zero-downtime)
pub struct RollingAlgorithmUpdate;

impl RollingAlgorithmUpdate {
    /// Perform rolling update of algorithm
    pub async fn update(
        manager: &mut AlgorithmManager,
        new_config: AlgorithmConfig,
        update_strategy: UpdateStrategy,
    ) -> Result<()> {
        match update_strategy {
            UpdateStrategy::Immediate => {
                // Immediate swap
                manager.swap_indexing(&new_config.id, new_config)?;
            }
            UpdateStrategy::Canary { percentage } => {
                // Canary deployment: route percentage to new algorithm
                let canary = manager.create_canary(&new_config)?;
                manager.set_canary_routing(percentage, canary);
                
                // Monitor canary performance
                // If successful, gradually increase percentage
            }
            UpdateStrategy::Abtest { split } => {
                // A/B test: split traffic between old and new
                let ab_test = manager.create_ab_test(&new_config, split)?;
                manager.set_ab_test(ab_test);
            }
        }
        
        Ok(())
    }
}
```

---

## 10. Configuration Schema

### 10.1 Complete Configuration Example

```toml
# config/deployments/example.toml

[deployment]
name = "aave-v3-ethereum"
chain_id = 1
protocol = "aave-v3"

# Algorithm selection
[algorithms]
indexing = { id = "interval_tree", version = "1.0.0" }
scoring = { id = "ev_based", version = "2.1.0" }
tier_classification = { id = "dynamic_volatility", version = "1.0.0" }
execution_strategy = { id = "optimal", version = "1.0.0" }

# Algorithm parameters
[algorithms.indexing.parameters]
max_positions = 10000
rebuild_interval_secs = 300

[algorithms.scoring.parameters]
min_win_probability = 0.3
competitor_factor_weight = 0.2

# Chain-specific overrides
[algorithms.chain_overrides.42161]  # Arbitrum
indexing = { id = "b_tree", version = "1.0.0" }  # Use B-tree on Arbitrum

# Protocol-specific overrides
[algorithms.protocol_overrides.compound-v3]
scoring = { id = "compound_optimized", version = "1.0.0" }
```

---

## 11. Implementation Checklist

### Phase 1: Core Abstractions
- [ ] Define algorithm trait hierarchy
- [ ] Implement base `Algorithm` trait
- [ ] Create algorithm version system
- [ ] Implement compatibility checking

### Phase 2: Algorithm Traits
- [ ] `IndexingAlgorithm` trait and implementations
- [ ] `ScoringAlgorithm` trait and implementations
- [ ] `TierClassificationAlgorithm` trait and implementations
- [ ] `ExecutionStrategyAlgorithm` trait and implementations

### Phase 3: Registry and Factory
- [ ] Algorithm registry implementation
- [ ] Algorithm factory pattern
- [ ] Default algorithm implementations
- [ ] Algorithm configuration system

### Phase 4: Chain/Protocol Abstraction
- [ ] Chain provider trait and implementations
- [ ] Protocol adapter pattern
- [ ] Unified position format
- [ ] Cross-chain compatibility layer

### Phase 5: Configuration and Runtime
- [ ] Configuration schema
- [ ] Runtime algorithm selection
- [ ] Hot-swapping support
- [ ] A/B testing framework

### Phase 6: Testing and Validation
- [ ] Compatibility testing framework
- [ ] Correctness testing
- [ ] Performance benchmarking
- [ ] Migration testing

---

## 12. Benefits of This Architecture

1. **Pluggability**: Swap algorithms via configuration, no code changes
2. **Testability**: Each algorithm can be tested in isolation
3. **Extensibility**: Add new algorithms without modifying existing code
4. **Compatibility**: Clear compatibility guarantees across chains/protocols
5. **Performance**: Optimize algorithms per chain/protocol
6. **A/B Testing**: Test new algorithms in production safely
7. **Maintainability**: Clear separation of concerns
8. **Upgradeability**: Migrate algorithms with zero downtime

---

## Conclusion

This architecture provides a robust framework for algorithm abstraction while maintaining compatibility across chains and protocols. The trait-based design enables pluggability, the registry pattern enables runtime selection, and the compatibility system ensures safe deployments.

**Next Steps:**
1. Implement core trait hierarchy
2. Create algorithm registry
3. Migrate existing algorithms to new trait system
4. Add configuration-driven selection
5. Implement testing framework

