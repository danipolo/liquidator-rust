// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {IPool} from "../../src/interfaces/IPool.sol";
import {ISwapAdapter} from "../../src/interfaces/ISwapAdapter.sol";

/// @title MockERC20
/// @notice Simple ERC20 token for testing
contract MockERC20 is ERC20 {
    uint8 private _decimals;

    constructor(string memory name, string memory symbol, uint8 decimals_) ERC20(name, symbol) {
        _decimals = decimals_;
    }

    function mint(address to, uint256 amount) external {
        _mint(to, amount);
    }

    function burn(address from, uint256 amount) external {
        _burn(from, amount);
    }

    function decimals() public view override returns (uint8) {
        return _decimals;
    }
}

/// @title MockWETH
/// @notice Mock wrapped native token for testing
contract MockWETH is ERC20 {
    constructor() ERC20("Wrapped Ether", "WETH") {}

    function deposit() external payable {
        _mint(msg.sender, msg.value);
    }

    function withdraw(uint256 amount) external {
        _burn(msg.sender, amount);
        payable(msg.sender).transfer(amount);
    }

    receive() external payable {
        _mint(msg.sender, msg.value);
    }
}

/// @title MockPool
/// @notice Mock AAVE V3 Pool for testing
contract MockPool is IPool {
    mapping(address => ReserveData) private reserves;
    uint128 public constant FLASHLOAN_PREMIUM_TOTAL_VALUE = 9; // 0.09%

    bool public shouldFailLiquidation;
    uint256 public collateralToReturn;

    function setReserveData(address asset, address variableDebtToken) external {
        reserves[asset].variableDebtTokenAddress = variableDebtToken;
    }

    function setLiquidationBehavior(bool shouldFail, uint256 collateralAmount) external {
        shouldFailLiquidation = shouldFail;
        collateralToReturn = collateralAmount;
    }

    function getReserveData(address asset) external view override returns (ReserveData memory) {
        return reserves[asset];
    }

    function flashLoanSimple(
        address receiverAddress,
        address asset,
        uint256 amount,
        bytes calldata params,
        uint16 /* referralCode */
    ) external override {
        // Transfer tokens to receiver
        IERC20(asset).transfer(receiverAddress, amount);

        // Calculate premium
        uint256 premium = (amount * FLASHLOAN_PREMIUM_TOTAL_VALUE) / 10000;

        // Call executeOperation on receiver
        // Note: initiator is msg.sender (the contract that called flashLoanSimple)
        (bool success, bytes memory result) = receiverAddress.call(
            abi.encodeWithSignature(
                "executeOperation(address,uint256,uint256,address,bytes)", asset, amount, premium, msg.sender, params
            )
        );

        require(success, string(result));

        // Pull back amount + premium (receiver must have approved this contract)
        IERC20(asset).transferFrom(receiverAddress, address(this), amount + premium);
    }

    function flashLoan(
        address receiverAddress,
        address[] calldata assets,
        uint256[] calldata amounts,
        uint256[] calldata, /* interestRateModes */
        address, /* onBehalfOf */
        bytes calldata params,
        uint16 /* referralCode */
    ) external override {
        // Simplified: delegate to internal implementation for testing
        _executeFlashLoan(receiverAddress, assets[0], amounts[0], params);
    }

    function _executeFlashLoan(address receiver, address asset, uint256 amount, bytes calldata params) internal {
        IERC20(asset).transfer(receiver, amount);
        uint256 premium = (amount * FLASHLOAN_PREMIUM_TOTAL_VALUE) / 10000;
        bytes memory callData = abi.encodeWithSignature(
            "executeOperation(address,uint256,uint256,address,bytes)", asset, amount, premium, receiver, params
        );
        (bool success,) = receiver.call(callData);
        require(success, "flash loan callback failed");
        IERC20(asset).transferFrom(receiver, address(this), amount + premium);
    }

    function liquidationCall(
        address collateralAsset,
        address, /* debtAsset */
        address, /* user */
        uint256 debtToCover,
        bool /* receiveAToken */
    ) external override {
        require(!shouldFailLiquidation, "liquidation failed");

        // Simulate liquidation: transfer collateral to caller
        uint256 collateralAmount = collateralToReturn > 0 ? collateralToReturn : debtToCover;
        MockERC20(collateralAsset).mint(msg.sender, collateralAmount);
    }

    function FLASHLOAN_PREMIUM_TOTAL() external pure override returns (uint128) {
        return FLASHLOAN_PREMIUM_TOTAL_VALUE;
    }
}

/// @title MockSwapAdapter
/// @notice Mock swap adapter for testing
contract MockSwapAdapter is ISwapAdapter {
    uint256 public swapRate = 1e18; // 1:1 by default
    bool public shouldFail;

    function setSwapRate(uint256 rate) external {
        swapRate = rate;
    }

    function setShouldFail(bool fail) external {
        shouldFail = fail;
    }

    function swap(address tokenIn, address tokenOut, uint256 amountIn, uint256 minAmountOut, bytes calldata /* data */ )
        external
        override
        returns (uint256 amountOut)
    {
        require(!shouldFail, "swap failed");

        // Calculate output based on swap rate
        amountOut = (amountIn * swapRate) / 1e18;

        require(amountOut >= minAmountOut, "insufficient output");

        // Pull input tokens from caller (liquidator approved us)
        IERC20(tokenIn).transferFrom(msg.sender, address(this), amountIn);

        // Mint output tokens to caller
        MockERC20(tokenOut).mint(msg.sender, amountOut);

        return amountOut;
    }
}

/// @title MockDebtToken
/// @notice Mock variable debt token for testing
contract MockDebtToken is ERC20 {
    constructor() ERC20("Variable Debt Token", "vToken") {}

    function mint(address to, uint256 amount) external {
        _mint(to, amount);
    }

    function burn(address from, uint256 amount) external {
        _burn(from, amount);
    }
}

/// @title MockUniswapV3Pool
/// @notice Mock Uniswap V3 Pool for flash swap testing
contract MockUniswapV3Pool {
    address public token0;
    address public token1;
    uint24 public fee;

    // Flash callback data
    address public lastFlashRecipient;
    uint256 public lastAmount0;
    uint256 public lastAmount1;

    constructor(address _token0, address _token1, uint24 _fee) {
        // Sort tokens like Uniswap does
        if (_token0 < _token1) {
            token0 = _token0;
            token1 = _token1;
        } else {
            token0 = _token1;
            token1 = _token0;
        }
        fee = _fee;
    }

    function flash(
        address recipient,
        uint256 amount0,
        uint256 amount1,
        bytes calldata data
    ) external {
        lastFlashRecipient = recipient;
        lastAmount0 = amount0;
        lastAmount1 = amount1;

        // Calculate fees (same as Uniswap V3)
        uint256 fee0 = amount0 > 0 ? (amount0 * fee) / 1e6 + 1 : 0;
        uint256 fee1 = amount1 > 0 ? (amount1 * fee) / 1e6 + 1 : 0;

        // Transfer requested amounts to recipient
        if (amount0 > 0) {
            IERC20(token0).transfer(recipient, amount0);
        }
        if (amount1 > 0) {
            IERC20(token1).transfer(recipient, amount1);
        }

        // Call flash callback
        (bool success, bytes memory result) = recipient.call(
            abi.encodeWithSignature(
                "uniswapV3FlashCallback(uint256,uint256,bytes)",
                fee0,
                fee1,
                data
            )
        );
        require(success, string(result));

        // Verify repayment (amount + fee)
        if (amount0 > 0) {
            uint256 balance0 = IERC20(token0).balanceOf(address(this));
            require(balance0 >= amount0 + fee0, "Flash not repaid: token0");
        }
        if (amount1 > 0) {
            uint256 balance1 = IERC20(token1).balanceOf(address(this));
            require(balance1 >= amount1 + fee1, "Flash not repaid: token1");
        }
    }
}

/// @title MockUniswapV3Factory
/// @notice Mock Uniswap V3 Factory for testing
contract MockUniswapV3Factory {
    mapping(address => mapping(address => mapping(uint24 => address))) public pools;

    /// @notice Create a mock pool for testing
    function createPool(address tokenA, address tokenB, uint24 fee) external returns (address pool) {
        (address token0, address token1) = tokenA < tokenB ? (tokenA, tokenB) : (tokenB, tokenA);

        pool = address(new MockUniswapV3Pool(token0, token1, fee));
        pools[token0][token1][fee] = pool;
        pools[token1][token0][fee] = pool;

        return pool;
    }

    /// @notice Set an existing pool address (for manual setup)
    function setPool(address tokenA, address tokenB, uint24 fee, address pool) external {
        pools[tokenA][tokenB][fee] = pool;
        pools[tokenB][tokenA][fee] = pool;
    }

    function getPool(address tokenA, address tokenB, uint24 fee) external view returns (address) {
        return pools[tokenA][tokenB][fee];
    }
}
