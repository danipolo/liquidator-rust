# Profit Calculation Fix for Staged Liquidations

## Problem Statement

The liquidation system is incorrectly calculating profit for staged liquidations on Arbitrum, causing profitable opportunities to be skipped. The issue manifests as:

```
profit=gross=$0.00 (5% bonus) - gas=$0.03 - slippage=$0.00 = net=$-0.03
```

This occurs even for large positions (e.g., $1M collateral, $872K debt) where the liquidation bonus alone should be ~$43,600 (5% of $872K).

## Root Cause Analysis

### Current Implementation

The `estimate_staged_profit()` function in `crates/core/src/liquidator.rs` (lines 670-682) relies on USD values from the swap route:

```rust
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
```

### The Problem

1. **Swap routes don't populate USD values**: The swap router implementations (e.g., `UniswapV3Router` in `crates/api/src/swap/uniswap_v3.rs:350-351`) set `expected_input_usd` and `expected_output_usd` to `None`:

   ```rust
   expected_input_usd: None,
   expected_output_usd: None,
   ```

2. **Fallback to zero**: When these values are `None`, the code uses `unwrap_or(0.0)`, resulting in:
   - `collateral_value_usd = 0.0`
   - `swap_output_usd = 0.0`
   - `gross_profit = 0.0 * 0.05 = 0.0`

3. **Available data is ignored**: The `StagedLiquidation` struct contains all the necessary data to calculate USD values:
   - `expected_collateral: U256` - the actual collateral amount
   - `price_snapshot: SmallVec<[(Address, U256); 4]>` - prices at staging time
   - `swap_route.expected_output: U256` - the expected swap output amount

### Why This Wasn't Caught Earlier

- The non-staged liquidation path (`build_and_execute`) correctly calculates USD values from position data
- Staged liquidations are a performance optimization (fast path) that was added later
- The swap routers were never updated to populate USD values (they focus on token amounts)

## Solution

### Approach

Instead of relying on swap route USD values, calculate them from the available data in `StagedLiquidation`:

1. **Calculate collateral USD value**:
   - Use `staged.expected_collateral` (U256 amount)
   - Get price from `staged.price_snapshot` for `staged.collateral_asset`
   - Get decimals from `REGISTRY.get_by_token()` (default to 18 if not found)
   - Use `u256_math::calculate_usd_f64(amount, price, decimals)`

2. **Calculate swap output USD value**:
   - Use `staged.swap_route.expected_output` (U256 amount)
   - Get price from `staged.price_snapshot` for `staged.debt_asset`
   - Get decimals from `REGISTRY.get_by_token()` (default to 18 if not found)
   - Use `u256_math::calculate_usd_f64(amount, price, decimals)`

3. **Fallback behavior**:
   - If price snapshot is missing prices, log a warning and use a conservative estimate
   - If decimals are unknown, default to 18 (standard for most tokens)

### Implementation Details

#### Modified Function Signature

The function will need access to the asset registry to get decimals. Since `REGISTRY` is already a global static, this is straightforward.

#### Code Changes

```rust
pub fn estimate_staged_profit(&self, staged: &StagedLiquidation) -> ProfitEstimate {
    // Get collateral USD value from expected_collateral and price snapshot
    let collateral_value_usd = {
        let collateral_price = staged.price_snapshot
            .iter()
            .find(|(addr, _)| *addr == staged.collateral_asset)
            .map(|(_, price)| *price);
        
        let collateral_decimals = REGISTRY
            .get_by_token(&staged.collateral_asset)
            .map(|a| a.decimals)
            .unwrap_or(18); // Default to 18 decimals
        
        match collateral_price {
            Some(price) => u256_math::calculate_usd_f64(
                staged.expected_collateral,
                price,
                collateral_decimals,
            ),
            None => {
                warn!(
                    user = %staged.user,
                    collateral = %staged.collateral_asset,
                    "Missing collateral price in snapshot, using 0.0"
                );
                0.0
            }
        }
    };

    // Get swap output USD value from swap route and price snapshot
    let swap_output_usd = {
        let debt_price = staged.price_snapshot
            .iter()
            .find(|(addr, _)| *addr == staged.debt_asset)
            .map(|(_, price)| *price);
        
        let debt_decimals = REGISTRY
            .get_by_token(&staged.debt_asset)
            .map(|a| a.decimals)
            .unwrap_or(18); // Default to 18 decimals
        
        match debt_price {
            Some(price) => u256_math::calculate_usd_f64(
                staged.swap_route.expected_output,
                price,
                debt_decimals,
            ),
            None => {
                warn!(
                    user = %staged.user,
                    debt = %staged.debt_asset,
                    "Missing debt price in snapshot, using 0.0"
                );
                0.0
            }
        }
    };

    self.estimate_profit(
        staged.collateral_asset,
        collateral_value_usd,
        collateral_value_usd,
        swap_output_usd,
    )
}
```

### Edge Cases to Handle

1. **Missing price in snapshot**: Log warning, use 0.0 (will fail profit check, which is safe)
2. **Missing asset in registry**: Default to 18 decimals (standard for most tokens)
3. **Zero amounts**: `calculate_usd_f64` already handles this and returns 0.0
4. **Price staleness**: The price snapshot is taken at staging time, which is acceptable for profit estimation

### Comparison with Non-Staged Path

The non-staged liquidation path (`build_and_execute`) calculates profit like this:

```rust
let collateral_value_usd = collateral.value_usd * self.params.close_factor;
let swap_output_usd = match swap_route.expected_output_usd {
    Some(usd) => usd,
    None => {
        warn!("Swap route missing expected_output_usd, using 1% slippage estimate");
        collateral_value_usd * 0.99
    }
};
```

Our fix aligns the staged path with this approach, but uses the actual swap output amount and price snapshot instead of estimating.

## Expected Impact

### Before Fix
- Gross profit: $0.00 (incorrect)
- Net profit: -$0.03 (always unprofitable)
- Result: All staged liquidations skipped

### After Fix
- Gross profit: ~$43,600 (5% of $872K debt) for the example case
- Net profit: ~$43,600 - $0.03 (gas) - slippage = highly profitable
- Result: Profitable liquidations will execute

## Testing Strategy

### Unit Tests

1. **Test with valid price snapshot**:
   - Create `StagedLiquidation` with price snapshot
   - Verify USD values are calculated correctly
   - Verify profit estimate is positive for liquidatable positions

2. **Test with missing prices**:
   - Create `StagedLiquidation` with incomplete price snapshot
   - Verify warnings are logged
   - Verify profit estimate uses 0.0 (fails profit check safely)

3. **Test with unknown assets**:
   - Use asset not in registry
   - Verify defaults to 18 decimals
   - Verify calculation still works

### Integration Tests

1. **End-to-end staged liquidation**:
   - Stage a position
   - Execute staged liquidation
   - Verify profit calculation is correct
   - Verify liquidation executes when profitable

2. **Compare with non-staged path**:
   - Same position, staged vs non-staged
   - Verify profit estimates are similar (within reasonable tolerance)

### Manual Testing

1. Monitor logs for the fixed position:
   ```
   user=0xEcC0e46F64458ae70DCa30e298d20e3c993D9541
   ```
   - Before: Should show gross=$0.00
   - After: Should show gross=$43,600+ (approximately)

2. Verify liquidation executes when profitable

## Risk Assessment

### Low Risk
- The fix only changes profit calculation, not execution logic
- If calculation fails, it defaults to 0.0 (unprofitable), which is safe
- Uses existing, tested utility functions (`calculate_usd_f64`)

### Potential Issues
1. **Price staleness**: Prices in snapshot may be stale, but this is acceptable for estimation
2. **Decimal mismatch**: Defaulting to 18 decimals may be wrong for some tokens, but:
   - Most tokens use 18 decimals
   - Asset registry should have correct decimals
   - Small decimal errors won't affect profitability decision for large positions

## Alternative Approaches Considered

### Option 1: Populate USD values in swap routers
- **Pros**: Fixes root cause, benefits all code paths
- **Cons**: Requires changes to multiple router implementations, may need price oracle access

### Option 2: Calculate USD values during staging
- **Pros**: Pre-computed, faster at execution time
- **Cons**: Requires storing USD values in `StagedLiquidation`, adds complexity

### Option 3: Use position data (current approach)
- **Pros**: Uses available data, minimal changes, aligns with non-staged path
- **Cons**: Requires price snapshot to be complete (already required for staleness checks)

**Selected**: Option 3 (current approach) - simplest, uses existing data structures, aligns with design

## Implementation Checklist

- [ ] Update `estimate_staged_profit()` to calculate USD values from price snapshot
- [ ] Add proper error handling for missing prices
- [ ] Add logging for debugging
- [ ] Add unit tests for all edge cases
- [ ] Add integration test comparing staged vs non-staged
- [ ] Update documentation if needed
- [ ] Test on Arbitrum mainnet with real positions
- [ ] Monitor logs to verify fix works in production

## Related Files

- `crates/core/src/liquidator.rs` - Main implementation
- `crates/core/src/pre_staging.rs` - StagedLiquidation struct
- `crates/core/src/u256_math.rs` - USD calculation utilities
- `crates/core/src/assets.rs` - Asset registry
- `crates/api/src/swap/uniswap_v3.rs` - Swap router (reference for USD value population)

## Conclusion

The profit calculation bug is caused by relying on USD values that swap routers don't populate. The fix calculates USD values from available data (amounts and price snapshots) in the `StagedLiquidation` struct. This aligns the staged liquidation path with the non-staged path and ensures profitable opportunities are correctly identified and executed.

