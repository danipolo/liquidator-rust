// SPDX-License-Identifier: AGPL-3.0
pragma solidity ^0.8.20;

/// @title IPool
/// @notice Minimal AAVE V3 Pool interface for liquidations
/// @dev Only includes functions needed for flash loan liquidations
interface IPool {
    /// @notice Returns the state and configuration data of the reserve
    /// @param asset The address of the underlying asset of the reserve
    /// @return The reserve data struct
    function getReserveData(address asset) external view returns (ReserveData memory);

    /// @notice Flash loan for a single asset
    /// @param receiverAddress The address receiving the flash loan
    /// @param asset The address of the asset being flash-borrowed
    /// @param amount The amount being flash-borrowed
    /// @param params Variadic packed params to pass to the receiver
    /// @param referralCode The referral code used
    function flashLoanSimple(
        address receiverAddress,
        address asset,
        uint256 amount,
        bytes calldata params,
        uint16 referralCode
    ) external;

    /// @notice Flash loan for multiple assets
    /// @param receiverAddress The address receiving the flash loan
    /// @param assets The addresses of the assets being flash-borrowed
    /// @param amounts The amounts being flash-borrowed
    /// @param interestRateModes Types of the debt to open if flash loan is not returned
    /// @param onBehalfOf The address that will receive the debt
    /// @param params Variadic packed params to pass to the receiver
    /// @param referralCode The referral code used
    function flashLoan(
        address receiverAddress,
        address[] calldata assets,
        uint256[] calldata amounts,
        uint256[] calldata interestRateModes,
        address onBehalfOf,
        bytes calldata params,
        uint16 referralCode
    ) external;

    /// @notice Liquidates an underwater position
    /// @param collateralAsset The collateral asset address
    /// @param debtAsset The debt asset address
    /// @param user The address of the borrower to liquidate
    /// @param debtToCover The amount of debt to cover
    /// @param receiveAToken True to receive aTokens, false for underlying
    function liquidationCall(
        address collateralAsset,
        address debtAsset,
        address user,
        uint256 debtToCover,
        bool receiveAToken
    ) external;

    /// @notice Returns the total fee on flash loans
    /// @return The total fee expressed in bps
    function FLASHLOAN_PREMIUM_TOTAL() external view returns (uint128);

    /// @notice Reserve data structure
    struct ReserveData {
        ReserveConfigurationMap configuration;
        uint128 liquidityIndex;
        uint128 currentLiquidityRate;
        uint128 variableBorrowIndex;
        uint128 currentVariableBorrowRate;
        uint128 currentStableBorrowRate;
        uint40 lastUpdateTimestamp;
        uint16 id;
        address aTokenAddress;
        address stableDebtTokenAddress;
        address variableDebtTokenAddress;
        address interestRateStrategyAddress;
        uint128 accruedToTreasury;
        uint128 unbacked;
        uint128 isolationModeTotalDebt;
    }

    /// @notice Reserve configuration bitmap
    struct ReserveConfigurationMap {
        uint256 data;
    }
}
