// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title BalancesReader
/// @notice Gas-efficient batch reader for AAVE V3 user positions with prices
/// @dev Works with AAVE V3 and forks (HyperLend, etc.)
contract BalancesReader {
    struct BalanceEntry {
        address underlying;
        uint256 amount;
        uint256 price;
        uint256 decimals;
    }

    /// @notice AAVE V3 Pool Data Provider
    IPoolDataProvider public immutable dataProvider;

    /// @notice AAVE Oracle for price feeds
    IAaveOracle public immutable oracle;

    constructor(address _dataProvider, address _oracle) {
        dataProvider = IPoolDataProvider(_dataProvider);
        oracle = IAaveOracle(_oracle);
    }

    /// @notice Get all supplied balances with prices for a user
    /// @param pool The AAVE pool address (unused, kept for interface compatibility)
    /// @param user The user address to query
    /// @return entries Array of balance entries for assets with non-zero supply
    function getAllSuppliedBalancesWithPrices(
        address pool,
        address user
    ) external view returns (BalanceEntry[] memory entries) {
        // Silence unused parameter warning
        pool;

        // Get all reserve tokens
        IPoolDataProvider.TokenData[] memory reserves = dataProvider.getAllReservesTokens();
        uint256 count = reserves.length;

        // First pass: count non-zero balances
        uint256 nonZeroCount = 0;
        uint256[] memory balances = new uint256[](count);

        for (uint256 i = 0; i < count; i++) {
            (uint256 aTokenBalance, , , , , , , , ) = dataProvider.getUserReserveData(
                reserves[i].tokenAddress,
                user
            );
            balances[i] = aTokenBalance;
            if (aTokenBalance > 0) {
                nonZeroCount++;
            }
        }

        // Second pass: populate results
        entries = new BalanceEntry[](nonZeroCount);
        uint256 idx = 0;

        for (uint256 i = 0; i < count; i++) {
            if (balances[i] > 0) {
                address underlying = reserves[i].tokenAddress;
                entries[idx] = BalanceEntry({
                    underlying: underlying,
                    amount: balances[i],
                    price: oracle.getAssetPrice(underlying),
                    decimals: _getDecimals(underlying)
                });
                idx++;
            }
        }
    }

    /// @notice Get all borrowed balances with prices for a user
    /// @param pool The AAVE pool address (unused, kept for interface compatibility)
    /// @param user The user address to query
    /// @return entries Array of balance entries for assets with non-zero debt
    function getAllBorrowedBalancesWithPrices(
        address pool,
        address user
    ) external view returns (BalanceEntry[] memory entries) {
        // Silence unused parameter warning
        pool;

        // Get all reserve tokens
        IPoolDataProvider.TokenData[] memory reserves = dataProvider.getAllReservesTokens();
        uint256 count = reserves.length;

        // First pass: count non-zero balances and get total debt
        uint256 nonZeroCount = 0;
        uint256[] memory debts = new uint256[](count);

        for (uint256 i = 0; i < count; i++) {
            (, uint256 stableDebt, uint256 variableDebt, , , , , , ) = dataProvider.getUserReserveData(
                reserves[i].tokenAddress,
                user
            );
            uint256 totalDebt = stableDebt + variableDebt;
            debts[i] = totalDebt;
            if (totalDebt > 0) {
                nonZeroCount++;
            }
        }

        // Second pass: populate results
        entries = new BalanceEntry[](nonZeroCount);
        uint256 idx = 0;

        for (uint256 i = 0; i < count; i++) {
            if (debts[i] > 0) {
                address underlying = reserves[i].tokenAddress;
                entries[idx] = BalanceEntry({
                    underlying: underlying,
                    amount: debts[i],
                    price: oracle.getAssetPrice(underlying),
                    decimals: _getDecimals(underlying)
                });
                idx++;
            }
        }
    }

    /// @notice Get decimals for a token
    function _getDecimals(address token) internal view returns (uint256) {
        try IERC20Metadata(token).decimals() returns (uint8 decimals) {
            return uint256(decimals);
        } catch {
            return 18; // Default to 18 decimals
        }
    }
}

/// @notice Minimal AAVE V3 Pool Data Provider interface
interface IPoolDataProvider {
    struct TokenData {
        string symbol;
        address tokenAddress;
    }

    function getAllReservesTokens() external view returns (TokenData[] memory);

    function getUserReserveData(
        address asset,
        address user
    ) external view returns (
        uint256 currentATokenBalance,
        uint256 currentStableDebt,
        uint256 currentVariableDebt,
        uint256 principalStableDebt,
        uint256 scaledVariableDebt,
        uint256 stableBorrowRate,
        uint256 liquidityRate,
        uint40 stableRateLastUpdated,
        bool usageAsCollateralEnabled
    );
}

/// @notice Minimal AAVE Oracle interface
interface IAaveOracle {
    function getAssetPrice(address asset) external view returns (uint256);
}

/// @notice Minimal ERC20 interface for decimals
interface IERC20Metadata {
    function decimals() external view returns (uint8);
}
