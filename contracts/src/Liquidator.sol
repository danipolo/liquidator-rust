// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

import {ILiquidator} from "./interfaces/ILiquidator.sol";
import {ISwapAdapter} from "./interfaces/ISwapAdapter.sol";
import {IPool} from "./interfaces/IPool.sol";
import {IWETH} from "./interfaces/IWETH.sol";
import {SwapDataDecoder} from "./libraries/SwapDataDecoder.sol";

/// @notice Uniswap V3 Pool interface for flash swaps
interface IUniswapV3Pool {
    function flash(address recipient, uint256 amount0, uint256 amount1, bytes calldata data) external;
    function token0() external view returns (address);
    function token1() external view returns (address);
}

/// @notice Uniswap V3 Factory interface
interface IUniswapV3Factory {
    function getPool(address tokenA, address tokenB, uint24 fee) external view returns (address);
}

/// @title Liquidator
/// @notice Multi-chain AAVE liquidation contract with flexible flash loan sources
/// @dev Supports both Uniswap V3 flash swaps and AAVE flash loans
///      - Uniswap V3: Used on chains with Uniswap (Arbitrum, Base, Optimism)
///      - AAVE: Used on chains without Uniswap V3 (HyperLiquid/HyperLend)
contract Liquidator is ILiquidator, Ownable, ReentrancyGuard {
    using SafeERC20 for IERC20;

    // ============ Flash Loan Sources ============
    enum FlashSource {
        UNISWAP_V3,  // Use Uniswap V3 flash swaps
        AAVE         // Use AAVE pool flash loans
    }

    // ============ Immutables ============

    /// @notice The AAVE V3 Pool contract
    IPool public immutable pool;

    /// @notice The Uniswap V3 Factory (address(0) if not available)
    IUniswapV3Factory public immutable uniswapFactory;

    /// @notice The wrapped native token (WETH, WHYPE, etc.)
    IWETH public immutable wrappedNative;

    /// @notice Which flash loan source to use
    FlashSource public immutable flashSource;

    // ============ State ============

    /// @notice Mapping of adapter type to adapter contract address
    mapping(uint8 => address) public adapters;

    /// @notice Default Uniswap V3 pool fee tier for flash loans (500 = 0.05%)
    uint24 public defaultFlashPoolFee = 500;

    // ============ Flash Callback State ============

    /// @notice Parameters passed through flash callback
    struct FlashParams {
        address user;
        address collateral;
        address debt;
        uint256 debtToCover;
        uint256 minAmountOut;
        bytes swapData;
        // Uniswap V3 specific
        address flashPool;
        bool debtIsToken0;
        // AAVE specific
        uint256 premium;
    }

    /// @dev Currently executing flash params (for callback validation)
    FlashParams private _currentFlash;

    // ============ Errors ============

    error NoPoolFound(address tokenA, address tokenB);
    error FlashLoanFailed();

    // ============ Constructor ============

    /// @notice Constructs the Liquidator contract
    /// @param _pool The AAVE V3 Pool address
    /// @param _uniswapFactory The Uniswap V3 Factory address (address(0) to use AAVE flash loans)
    /// @param _wrappedNative The wrapped native token address (WETH/WHYPE)
    constructor(
        address _pool,
        address _uniswapFactory,
        address _wrappedNative
    ) Ownable(msg.sender) {
        pool = IPool(_pool);
        uniswapFactory = IUniswapV3Factory(_uniswapFactory);
        wrappedNative = IWETH(_wrappedNative);

        // Determine flash source based on factory availability
        flashSource = _uniswapFactory != address(0) ? FlashSource.UNISWAP_V3 : FlashSource.AAVE;
    }

    /// @notice Allows contract to receive native tokens (for wrapping)
    receive() external payable {}

    // ============ External Functions ============

    /// @inheritdoc ILiquidator
    function liquidate(
        address user,
        address collateral,
        address debt,
        uint256 debtAmount,
        uint256 minAmountOut,
        bytes calldata swapData
    ) external onlyOwner nonReentrant returns (uint256 profit) {
        return _liquidate(user, collateral, debt, debtAmount, minAmountOut, swapData, defaultFlashPoolFee);
    }

    /// @notice Execute a flash loan liquidation with explicit pool fee (Uniswap V3 only)
    /// @param user The address of the user to liquidate
    /// @param collateral The collateral asset to seize
    /// @param debt The debt asset to repay
    /// @param debtAmount The amount of debt to cover (use type(uint256).max for 50%)
    /// @param minAmountOut Minimum amount of debt token to receive after swap
    /// @param swapData Encoded swap parameters for the adapter
    /// @param flashPoolFee The Uniswap V3 pool fee tier (500, 3000, 10000)
    /// @return profit The profit from the liquidation in debt tokens
    function liquidateWithFee(
        address user,
        address collateral,
        address debt,
        uint256 debtAmount,
        uint256 minAmountOut,
        bytes calldata swapData,
        uint24 flashPoolFee
    ) external onlyOwner nonReentrant returns (uint256 profit) {
        return _liquidate(user, collateral, debt, debtAmount, minAmountOut, swapData, flashPoolFee);
    }

    /// @inheritdoc ILiquidator
    function setAdapter(uint8 adapterType, address adapter) external onlyOwner {
        adapters[adapterType] = adapter;
        emit AdapterUpdated(adapterType, adapter);
    }

    /// @notice Set the default flash pool fee tier (Uniswap V3 only)
    /// @param fee The Uniswap V3 pool fee tier (100, 500, 3000, 10000)
    function setDefaultFlashPoolFee(uint24 fee) external onlyOwner {
        require(fee == 100 || fee == 500 || fee == 3000 || fee == 10000, "Invalid fee tier");
        defaultFlashPoolFee = fee;
    }

    /// @inheritdoc ILiquidator
    function rescueTokens(address token, uint256 amount, bool max, address to) external onlyOwner {
        if (token == address(0)) {
            if (max) amount = address(this).balance;
            (bool success,) = payable(to).call{value: amount}("");
            require(success, "transfer failed");
        } else {
            if (max) amount = IERC20(token).balanceOf(address(this));
            IERC20(token).safeTransfer(to, amount);
        }
    }

    // ============ Flash Callbacks ============

    /// @notice Uniswap V3 flash callback
    /// @dev Called by the Uniswap V3 pool after flash
    function uniswapV3FlashCallback(uint256 fee0, uint256 fee1, bytes calldata) external {
        FlashParams memory params = _currentFlash;

        // Validate callback is from expected pool
        if (msg.sender != params.flashPool) {
            revert InvalidFlashLoanCallback();
        }

        // Calculate fee and execute liquidation
        uint256 fee = params.debtIsToken0 ? fee0 : fee1;
        uint256 amountOwed = params.debtToCover + fee;

        _executeLiquidation(params);

        // Verify and repay
        uint256 debtBalance = IERC20(params.debt).balanceOf(address(this));
        require(debtBalance >= amountOwed, "insufficient output");
        IERC20(params.debt).safeTransfer(msg.sender, amountOwed);

        _emitProfit(params, debtBalance - amountOwed);
    }

    /// @notice AAVE flash loan callback
    /// @dev Called by the AAVE pool after flash loan
    function executeOperation(
        address asset,
        uint256 amount,
        uint256 premium,
        address initiator,
        bytes calldata
    ) external returns (bool) {
        require(msg.sender == address(pool), "caller != pool");
        require(initiator == address(this), "initiator != this");

        FlashParams memory params = _currentFlash;
        require(asset == params.debt, "asset mismatch");

        uint256 amountOwed = amount + premium;

        _executeLiquidation(params);

        // Verify output and approve repayment
        uint256 debtBalance = IERC20(params.debt).balanceOf(address(this));
        require(debtBalance >= amountOwed, "insufficient output");

        // Approve pool to pull repayment
        IERC20(params.debt).forceApprove(address(pool), amountOwed);

        _emitProfit(params, debtBalance - amountOwed);

        return true;
    }

    // ============ Internal Functions ============

    /// @dev Internal liquidation implementation - routes to appropriate flash source
    function _liquidate(
        address user,
        address collateral,
        address debt,
        uint256 debtAmount,
        uint256 minAmountOut,
        bytes calldata swapData,
        uint24 flashPoolFee
    ) internal returns (uint256 profit) {
        // Find required debt amount (50% of debt if max)
        if (debtAmount == type(uint256).max) {
            address dToken = pool.getReserveData(debt).variableDebtTokenAddress;
            debtAmount = IERC20(dToken).balanceOf(user) / 2;
        }

        uint256 balanceBefore = IERC20(debt).balanceOf(address(this));

        if (flashSource == FlashSource.UNISWAP_V3) {
            _flashViaUniswap(user, collateral, debt, debtAmount, minAmountOut, swapData, flashPoolFee);
        } else {
            _flashViaAave(user, collateral, debt, debtAmount, minAmountOut, swapData);
        }

        // Calculate profit
        uint256 balanceAfter = IERC20(debt).balanceOf(address(this));
        profit = balanceAfter > balanceBefore ? balanceAfter - balanceBefore : 0;

        // Clear flash params
        delete _currentFlash;

        return profit;
    }

    /// @dev Execute flash via Uniswap V3
    function _flashViaUniswap(
        address user,
        address collateral,
        address debt,
        uint256 debtAmount,
        uint256 minAmountOut,
        bytes calldata swapData,
        uint24 flashPoolFee
    ) internal {
        // Find Uniswap V3 pool for flash loan
        address flashPool = uniswapFactory.getPool(collateral, debt, flashPoolFee);
        if (flashPool == address(0)) {
            flashPool = uniswapFactory.getPool(address(wrappedNative), debt, flashPoolFee);
        }
        if (flashPool == address(0)) {
            revert NoPoolFound(collateral, debt);
        }

        // Determine token ordering
        address token0 = IUniswapV3Pool(flashPool).token0();
        bool debtIsToken0 = (token0 == debt);

        // Store flash params for callback
        _currentFlash = FlashParams({
            user: user,
            collateral: collateral,
            debt: debt,
            debtToCover: debtAmount,
            minAmountOut: minAmountOut,
            swapData: swapData,
            flashPool: flashPool,
            debtIsToken0: debtIsToken0,
            premium: 0
        });

        // Execute flash swap
        if (debtIsToken0) {
            IUniswapV3Pool(flashPool).flash(address(this), debtAmount, 0, "");
        } else {
            IUniswapV3Pool(flashPool).flash(address(this), 0, debtAmount, "");
        }
    }

    /// @dev Execute flash via AAVE pool
    function _flashViaAave(
        address user,
        address collateral,
        address debt,
        uint256 debtAmount,
        uint256 minAmountOut,
        bytes calldata swapData
    ) internal {
        // Store flash params for callback
        _currentFlash = FlashParams({
            user: user,
            collateral: collateral,
            debt: debt,
            debtToCover: debtAmount,
            minAmountOut: minAmountOut,
            swapData: swapData,
            flashPool: address(0),
            debtIsToken0: false,
            premium: 0
        });

        // Execute AAVE flash loan
        pool.flashLoanSimple(address(this), debt, debtAmount, "", 0);
    }

    /// @dev Execute the actual liquidation (called from flash callback)
    function _executeLiquidation(FlashParams memory params) internal {
        // Approve AAVE pool for liquidation
        IERC20(params.debt).forceApprove(address(pool), type(uint256).max);

        // Execute liquidation - receive collateral
        pool.liquidationCall(
            params.collateral,
            params.debt,
            params.user,
            params.debtToCover,
            false // receive underlying, not aToken
        );

        // Get collateral balance after liquidation
        uint256 collateralBalance = IERC20(params.collateral).balanceOf(address(this));

        // Wrap any native tokens received
        if (address(this).balance > 0) {
            wrappedNative.deposit{value: address(this).balance}();
        }

        // Swap collateral back to debt token (if different)
        if (params.collateral != params.debt && collateralBalance > 0) {
            (uint8 adapterType, bytes memory adapterData) = SwapDataDecoder.decodeWrappedSwapData(params.swapData);

            address adapter = adapters[adapterType];
            if (adapter == address(0)) {
                revert UnknownAdapter(adapterType);
            }

            // Transfer collateral TO the adapter
            IERC20(params.collateral).safeTransfer(adapter, collateralBalance);

            // Execute swap
            uint256 amountOut = ISwapAdapter(adapter).swap(
                params.collateral,
                params.debt,
                collateralBalance,
                params.minAmountOut,
                adapterData
            );

            if (amountOut < params.minAmountOut) {
                revert SlippageExceeded(amountOut, params.minAmountOut);
            }
        }
    }

    /// @dev Emit liquidation event with profit
    function _emitProfit(FlashParams memory params, uint256 profitAmount) internal {
        uint256 collateralBalance = IERC20(params.collateral).balanceOf(address(this));
        emit Liquidation(
            params.user,
            params.collateral,
            params.debt,
            params.debtToCover,
            collateralBalance,
            profitAmount
        );
    }
}
