// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title IWETH
/// @notice Interface for Wrapped Native Token (WETH, WHYPE, etc.)
/// @dev Used for wrapping native tokens received during liquidation
interface IWETH {
    /// @notice Deposits native tokens and receives wrapped tokens
    function deposit() external payable;

    /// @notice Withdraws wrapped tokens to native tokens
    /// @param amount The amount to withdraw
    function withdraw(uint256 amount) external;

    /// @notice Returns the balance of wrapped tokens
    /// @param account The address to query
    /// @return The balance of wrapped tokens
    function balanceOf(address account) external view returns (uint256);

    /// @notice Transfers wrapped tokens
    /// @param to The recipient address
    /// @param amount The amount to transfer
    /// @return True if successful
    function transfer(address to, uint256 amount) external returns (bool);

    /// @notice Approves spending of wrapped tokens
    /// @param spender The address allowed to spend
    /// @param amount The amount to approve
    /// @return True if successful
    function approve(address spender, uint256 amount) external returns (bool);
}
