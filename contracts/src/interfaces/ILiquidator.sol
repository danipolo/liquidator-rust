// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title ILiquidator
/// @notice Interface for the multi-chain liquidation contract
/// @dev Supports pluggable swap adapters via encoded swap data
interface ILiquidator {
    /// @notice Executes a liquidation on an underwater AAVE position
    /// @dev Uses flash loan to borrow debt, liquidate, swap collateral, repay
    /// @param user The address of the position owner to liquidate
    /// @param collateral The collateral asset address
    /// @param debt The debt asset address
    /// @param debtAmount Amount of debt to cover (type(uint256).max for 50% of debt)
    /// @param minAmountOut Minimum collateral to receive after swap (slippage protection)
    /// @param swapData Encoded swap routing data (see WrappedSwapData)
    /// @return profit The profit amount in debt tokens
    function liquidate(
        address user,
        address collateral,
        address debt,
        uint256 debtAmount,
        uint256 minAmountOut,
        bytes calldata swapData
    ) external returns (uint256 profit);

    /// @notice Rescues tokens stuck in the contract
    /// @dev Only callable by owner, supports both ERC20 and native tokens
    /// @param token Token address (address(0) for native)
    /// @param amount Amount to rescue (ignored if max is true)
    /// @param max If true, rescue entire balance
    /// @param to Recipient address
    function rescueTokens(
        address token,
        uint256 amount,
        bool max,
        address to
    ) external;

    /// @notice Updates the adapter for a given type
    /// @param adapterType The adapter type identifier
    /// @param adapter The adapter contract address
    function setAdapter(uint8 adapterType, address adapter) external;

    /// @notice Returns the adapter address for a given type
    /// @param adapterType The adapter type identifier
    /// @return The adapter contract address
    function adapters(uint8 adapterType) external view returns (address);

    /// @notice Emitted on successful liquidation
    /// @param user The liquidated user
    /// @param collateral The collateral asset
    /// @param debt The debt asset
    /// @param debtAmount Amount of debt covered
    /// @param collateralReceived Amount of collateral received
    /// @param profit Net profit after repaying flash loan
    event Liquidation(
        address indexed user,
        address indexed collateral,
        address indexed debt,
        uint256 debtAmount,
        uint256 collateralReceived,
        uint256 profit
    );

    /// @notice Emitted when an adapter is updated
    /// @param adapterType The adapter type identifier
    /// @param adapter The new adapter address
    event AdapterUpdated(uint8 indexed adapterType, address adapter);

    /// @dev Thrown when caller is not the owner
    error Unauthorized();

    /// @dev Thrown when adapter type is not registered
    error UnknownAdapter(uint8 adapterType);

    /// @dev Thrown when slippage protection is triggered
    error SlippageExceeded(uint256 received, uint256 minimum);

    /// @dev Thrown when flash loan callback validation fails
    error InvalidFlashLoanCallback();
}
