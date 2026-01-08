//! High-performance U256 arithmetic for liquidation calculations.
//!
//! This module provides native U256 operations that avoid expensive
//! String conversions (U256 -> String -> f64), providing 2-5x speedup
//! in hot paths like position evaluation and health factor calculation.

use alloy::primitives::U256;

/// WAD constant: 1e18 for 18-decimal fixed-point arithmetic
pub const WAD: U256 = U256::from_limbs([1_000_000_000_000_000_000u64, 0, 0, 0]);

/// RAY constant: 1e27 for 27-decimal fixed-point arithmetic
pub const RAY: U256 = U256::from_limbs([1000000000000000000000000000u128 as u64, 0, 0, 0]);

/// Basis points denominator (10000 = 100%)
pub const BPS_DENOMINATOR: U256 = U256::from_limbs([10000u64, 0, 0, 0]);

/// Oracle price decimals (8)
pub const PRICE_DECIMALS: u8 = 8;

/// Pre-computed powers of 10 for fast decimal conversion
const POW10: [u128; 39] = [
    1,
    10,
    100,
    1_000,
    10_000,
    100_000,
    1_000_000,
    10_000_000,
    100_000_000,
    1_000_000_000,
    10_000_000_000,
    100_000_000_000,
    1_000_000_000_000,
    10_000_000_000_000,
    100_000_000_000_000,
    1_000_000_000_000_000,
    10_000_000_000_000_000,
    100_000_000_000_000_000,
    1_000_000_000_000_000_000,
    10_000_000_000_000_000_000,
    100_000_000_000_000_000_000,
    1_000_000_000_000_000_000_000,
    10_000_000_000_000_000_000_000,
    100_000_000_000_000_000_000_000,
    1_000_000_000_000_000_000_000_000,
    10_000_000_000_000_000_000_000_000,
    100_000_000_000_000_000_000_000_000,
    1_000_000_000_000_000_000_000_000_000,
    10_000_000_000_000_000_000_000_000_000,
    100_000_000_000_000_000_000_000_000_000,
    1_000_000_000_000_000_000_000_000_000_000,
    10_000_000_000_000_000_000_000_000_000_000,
    100_000_000_000_000_000_000_000_000_000_000,
    1_000_000_000_000_000_000_000_000_000_000_000,
    10_000_000_000_000_000_000_000_000_000_000_000,
    100_000_000_000_000_000_000_000_000_000_000_000,
    1_000_000_000_000_000_000_000_000_000_000_000_000,
    10_000_000_000_000_000_000_000_000_000_000_000_000,
    100_000_000_000_000_000_000_000_000_000_000_000_000,
];

/// Fast power of 10 lookup (up to 10^38)
#[inline(always)]
pub fn pow10(exp: u8) -> U256 {
    if exp < 39 {
        U256::from(POW10[exp as usize])
    } else {
        U256::from(10u64).pow(U256::from(exp))
    }
}

/// Apply basis points reduction (e.g., for slippage).
/// Returns: value * (10000 - basis_points) / 10000
///
/// Example: apply_basis_points(1000, 100) = 990 (1% reduction)
#[inline(always)]
pub fn apply_basis_points(value: U256, basis_points: u16) -> U256 {
    let factor = U256::from(10000u16.saturating_sub(basis_points));
    (value * factor) / BPS_DENOMINATOR
}

/// Apply basis points increase (e.g., for gas buffer).
/// Returns: value * (10000 + basis_points) / 10000
///
/// Example: apply_basis_points_up(1000, 2000) = 1200 (20% increase)
#[inline(always)]
pub fn apply_basis_points_up(value: U256, basis_points: u16) -> U256 {
    let factor = U256::from(10000u16.saturating_add(basis_points));
    (value * factor) / BPS_DENOMINATOR
}

/// Calculate USD value from token amount and oracle price.
/// Returns value in 18-decimal WAD format.
///
/// Formula: (amount * price * 10^18) / (10^decimals * 10^8)
///
/// Example: 1000 USDC (6 decimals) at $1 price = 1000 * 10^18 WAD
#[inline(always)]
pub fn calculate_usd_wad(amount: U256, price: U256, decimals: u8) -> U256 {
    if amount.is_zero() || price.is_zero() {
        return U256::ZERO;
    }

    // Scale: we want result in 18 decimals
    // amount has `decimals` decimals, price has 8 decimals
    // result = amount * price * 10^(18 - decimals - 8)
    let target_decimals = 18i32;
    let scale_adjustment = target_decimals - decimals as i32 - PRICE_DECIMALS as i32;

    if scale_adjustment >= 0 {
        amount * price * pow10(scale_adjustment as u8)
    } else {
        (amount * price) / pow10((-scale_adjustment) as u8)
    }
}

/// Calculate USD value as f64 (for display/logging only, not computation).
/// This is faster than the old String parsing method but still uses f64.
#[inline(always)]
pub fn calculate_usd_f64(amount: U256, price: U256, decimals: u8) -> f64 {
    let wad = calculate_usd_wad(amount, price, decimals);
    wad_to_f64(wad)
}

/// Convert WAD (18 decimals) to f64.
/// Use only for display/logging, not for computation.
#[inline(always)]
pub fn wad_to_f64(wad: U256) -> f64 {
    // For values that fit in u128, use direct conversion
    if wad <= U256::from(u128::MAX) {
        let value: u128 = wad.to();
        value as f64 / 1e18
    } else {
        // For larger values, use limbs
        let limbs = wad.as_limbs();
        let high = limbs[1] as f64 * (u64::MAX as f64 + 1.0);
        let low = limbs[0] as f64;
        (high + low) / 1e18
    }
}

/// Convert f64 to WAD (18 decimals).
/// Use for converting user input to U256.
#[inline(always)]
pub fn f64_to_wad(value: f64) -> U256 {
    if value <= 0.0 {
        return U256::ZERO;
    }
    U256::from((value * 1e18) as u128)
}

/// Calculate health factor in WAD (18 decimals).
/// HF = (total_collateral_adjusted * 10^18) / total_debt
///
/// Returns U256::MAX if debt is zero.
#[inline(always)]
pub fn calculate_hf_wad(collateral_adjusted_wad: U256, debt_wad: U256) -> U256 {
    if debt_wad.is_zero() {
        return U256::MAX;
    }
    (collateral_adjusted_wad * WAD) / debt_wad
}

/// Check if health factor indicates liquidatable position (HF < 1.0).
#[inline(always)]
pub fn is_liquidatable_wad(hf_wad: U256) -> bool {
    hf_wad < WAD
}

/// Calculate percentage difference in basis points.
/// Returns: ((new - old) * 10000) / old
/// Positive = increase, negative = decrease
#[inline(always)]
pub fn pct_diff_bps(old: U256, new: U256) -> i64 {
    if old.is_zero() {
        return 0;
    }

    if new >= old {
        let diff = new - old;
        let bps = (diff * BPS_DENOMINATOR) / old;
        bps.to::<i64>()
    } else {
        let diff = old - new;
        let bps = (diff * BPS_DENOMINATOR) / old;
        -(bps.to::<i64>())
    }
}

/// Calculate percentage as f64 (for display).
/// Returns: (value * 100) / total as percentage
#[inline(always)]
pub fn pct_f64(value: U256, total: U256) -> f64 {
    if total.is_zero() {
        return 0.0;
    }
    let bps = (value * BPS_DENOMINATOR) / total;
    bps.to::<u64>() as f64 / 100.0
}

/// Multiply two WAD values: (a * b) / WAD
#[inline(always)]
pub fn wad_mul(a: U256, b: U256) -> U256 {
    (a * b) / WAD
}

/// Divide two WAD values: (a * WAD) / b
#[inline(always)]
pub fn wad_div(a: U256, b: U256) -> U256 {
    if b.is_zero() {
        return U256::MAX;
    }
    (a * WAD) / b
}

/// Calculate trigger price for liquidation.
/// Given current price and distance to liquidation, calculate the price
/// at which HF = 1.0.
///
/// For collateral: trigger_price = current_price * (1 - distance_pct)
/// For debt: trigger_price = current_price * (1 + distance_pct)
#[inline(always)]
pub fn trigger_price_collateral(current_price: U256, distance_bps: u16) -> U256 {
    apply_basis_points(current_price, distance_bps)
}

#[inline(always)]
pub fn trigger_price_debt(current_price: U256, distance_bps: u16) -> U256 {
    apply_basis_points_up(current_price, distance_bps)
}

/// Safe minimum of two U256 values
#[inline(always)]
pub fn min(a: U256, b: U256) -> U256 {
    if a < b {
        a
    } else {
        b
    }
}

/// Safe maximum of two U256 values
#[inline(always)]
pub fn max(a: U256, b: U256) -> U256 {
    if a > b {
        a
    } else {
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_basis_points() {
        // 1% reduction (100 bps)
        let value = U256::from(1000u64);
        let result = apply_basis_points(value, 100);
        assert_eq!(result, U256::from(990u64));

        // 10% reduction (1000 bps)
        let result = apply_basis_points(value, 1000);
        assert_eq!(result, U256::from(900u64));

        // 0% reduction
        let result = apply_basis_points(value, 0);
        assert_eq!(result, U256::from(1000u64));
    }

    #[test]
    fn test_calculate_usd_wad() {
        // 1000 USDC (6 decimals) at $1.00 (1e8 price)
        let amount = U256::from(1000_000000u64); // 1000 USDC
        let price = U256::from(100_000_000u64); // $1.00
        let decimals = 6u8;

        let usd_wad = calculate_usd_wad(amount, price, decimals);
        // Expected: 1000 * 10^18
        let expected = U256::from(1000u64) * WAD;
        assert_eq!(usd_wad, expected);
    }

    #[test]
    fn test_calculate_usd_wad_eth() {
        // 1.5 ETH (18 decimals) at $2000 price
        let amount = U256::from(1_500_000_000_000_000_000u128); // 1.5 ETH
        let price = U256::from(200_000_000_000u64); // $2000.00
        let decimals = 18u8;

        let usd_wad = calculate_usd_wad(amount, price, decimals);
        // Expected: 3000 * 10^18
        let expected = U256::from(3000u64) * WAD;
        assert_eq!(usd_wad, expected);
    }

    #[test]
    fn test_wad_to_f64() {
        let wad = U256::from(1000u64) * WAD;
        let f64_val = wad_to_f64(wad);
        assert!((f64_val - 1000.0).abs() < 0.001);
    }

    #[test]
    fn test_calculate_hf_wad() {
        // Collateral: 1000 USD adjusted, Debt: 500 USD
        // HF = 1000 / 500 = 2.0
        let collateral = U256::from(1000u64) * WAD;
        let debt = U256::from(500u64) * WAD;

        let hf = calculate_hf_wad(collateral, debt);
        let expected = U256::from(2u64) * WAD;
        assert_eq!(hf, expected);
    }

    #[test]
    fn test_is_liquidatable() {
        // HF = 0.9 (liquidatable)
        let hf_low = (WAD * U256::from(9u64)) / U256::from(10u64);
        assert!(is_liquidatable_wad(hf_low));

        // HF = 1.1 (not liquidatable)
        let hf_high = (WAD * U256::from(11u64)) / U256::from(10u64);
        assert!(!is_liquidatable_wad(hf_high));

        // HF = 1.0 (not liquidatable, boundary)
        assert!(!is_liquidatable_wad(WAD));
    }

    #[test]
    fn test_pct_diff_bps() {
        // 10% increase
        let old = U256::from(100u64);
        let new = U256::from(110u64);
        assert_eq!(pct_diff_bps(old, new), 1000); // 10% = 1000 bps

        // 10% decrease
        let new = U256::from(90u64);
        assert_eq!(pct_diff_bps(old, new), -1000);
    }

    #[test]
    fn test_pow10_lookup() {
        assert_eq!(pow10(0), U256::from(1u64));
        assert_eq!(pow10(6), U256::from(1_000_000u64));
        assert_eq!(pow10(18), U256::from(1_000_000_000_000_000_000u64));
    }
}
