# Smart Contract Specification: Multi-Chain Liquidator

## Overview

This document specifies the refactoring of the existing HyperLiquid-specific liquidator ([old-contracts/contracts/Liquidator.sol](../old-contracts/contracts/Liquidator.sol)) into a multi-chain, adapter-based architecture following Foundry best practices.

---

## Architecture Comparison

| Aspect | Old Contract | New Architecture |
|--------|--------------|------------------|
| **Swap Routing** | Hardcoded `ILiquidSwap` interface | Abstract `bytes _swapData` with adapter selection |
| **DEX Support** | LiquidSwap only | LiquidSwap, UniswapV3, Direct (pluggable) |
| **Chain Support** | HyperLiquid only | HyperLiquid, Arbitrum, Base, Optimism, Celo |
| **Router Address** | Hardcoded `0x744489...` | Configurable per adapter via constructor |
| **Flash Loan** | `flashLoanSimple` only | Support both `flashLoanSimple` and `flashLoan` |
| **Swap Params** | `Swap[][] hops, address[] tokens` in signature | Encoded in `bytes _swapData` |
| **Extensibility** | Requires redeployment for new DEX | Add adapters without core contract changes |

---

## Interface Specifications

### Core Liquidator Interface

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title ILiquidator
/// @author Your Team
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
```

### Swap Adapter Interface

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title ISwapAdapter
/// @notice Interface for pluggable DEX adapters
/// @dev All adapters must implement this interface for compatibility with Liquidator
interface ISwapAdapter {
    /// @notice Executes a swap from tokenIn to tokenOut
    /// @dev Tokens must be approved before calling. Adapter handles routing logic.
    /// @param tokenIn Input token address
    /// @param tokenOut Output token address
    /// @param amountIn Amount of input tokens
    /// @param minAmountOut Minimum output tokens (slippage protection)
    /// @param data Adapter-specific encoded parameters
    /// @return amountOut Actual output amount received
    function swap(
        address tokenIn,
        address tokenOut,
        uint256 amountIn,
        uint256 minAmountOut,
        bytes calldata data
    ) external returns (uint256 amountOut);
}
```

### Swap Data Encoding Format

```solidity
/// @dev Outer wrapper decoded first to route to correct adapter
struct WrappedSwapData {
    uint8 adapterType;    // 0=LiquidSwap, 1=UniswapV3, 2=Direct
    bytes adapterData;    // Adapter-specific payload
}

// Adapter 0: LiquidSwap (HyperLiquid) - matches old contract's ILiquidSwap.Swap
struct SwapAlloc {
    address tokenIn;
    address tokenOut;
    uint8 routerIndex;    // 1=KittenSwap, 2=HyperSwapV2, 3=HyperSwapV3, etc.
    uint24 fee;
    uint256 amountIn;
    bool stable;
}

struct LiquidSwapData {
    address[] tokens;
    SwapAlloc[][] hops;
}

// Adapter 1: UniswapV3
struct UniswapV3SwapData {
    bool isMultiHop;
    bytes pathOrFee;      // Single: abi.encode(uint24), Multi: packed path
}

// Adapter 2: Direct - empty bytes (no swap needed)
```

---

## Project Structure

```
contracts/
├── foundry.toml
├── remappings.txt
├── .env.example
├── src/
│   ├── Liquidator.sol                    # Main contract
│   ├── interfaces/
│   │   ├── ILiquidator.sol
│   │   ├── ISwapAdapter.sol
│   │   ├── IPool.sol                     # AAVE V3 Pool
│   │   ├── ILiquidSwap.sol               # From old-contracts
│   │   └── ISwapRouter.sol               # UniswapV3 SwapRouter02
│   ├── adapters/
│   │   ├── LiquidSwapAdapter.sol         # HyperLiquid DEX
│   │   ├── UniswapV3Adapter.sol          # Uniswap V3
│   │   └── DirectAdapter.sol             # No-op passthrough
│   └── libraries/
│       └── SwapDataDecoder.sol           # Decoding helpers
├── test/
│   ├── unit/
│   │   ├── Liquidator.t.sol
│   │   └── adapters/
│   │       ├── LiquidSwapAdapter.t.sol
│   │       └── UniswapV3Adapter.t.sol
│   ├── fuzz/
│   │   └── FuzzLiquidator.t.sol
│   ├── invariant/
│   │   ├── handlers/
│   │   │   └── LiquidatorHandler.sol
│   │   └── LiquidatorInvariant.t.sol
│   ├── fork/
│   │   ├── HyperLiquidFork.t.sol
│   │   └── ArbitrumFork.t.sol
│   └── utils/
│       └── TestConstants.sol
└── script/
    ├── Deploy.s.sol
    ├── DeployHyperLiquid.s.sol
    └── DeployArbitrum.s.sol
```

---

## Configuration

### foundry.toml

```toml
[profile.default]
src = "src"
out = "out"
libs = ["lib"]
solc_version = "0.8.20"
optimizer = true
optimizer_runs = 200
via_ir = false
dynamic_test_linking = true

# Gas reporting
gas_reports = ["Liquidator", "LiquidSwapAdapter", "UniswapV3Adapter"]

# Testing configuration
ffi = false
fs_permissions = [{ access = "read", path = "./" }]

[fuzz]
runs = 1000
max_test_rejects = 65536

[invariant]
runs = 256
depth = 15
fail_on_revert = false
show_metrics = true

[rpc_endpoints]
hyperliquid = "${HYPERLIQUID_RPC_URL}"
arbitrum = "${ARBITRUM_RPC_URL}"
base = "${BASE_RPC_URL}"
optimism = "${OPTIMISM_RPC_URL}"

[etherscan]
arbitrum = { key = "${ARBISCAN_API_KEY}", url = "https://api.arbiscan.io/api" }
base = { key = "${BASESCAN_API_KEY}", url = "https://api.basescan.org/api" }
optimism = { key = "${OPTIMISTIC_ETHERSCAN_API_KEY}", url = "https://api-optimistic.etherscan.io/api" }
```

### remappings.txt

```
@openzeppelin/contracts/=lib/openzeppelin-contracts/contracts/
forge-std/=lib/forge-std/src/
```

### .env.example

```bash
# Private key for deployment (use hardware wallet in production)
PRIVATE_KEY=

# RPC URLs
HYPERLIQUID_RPC_URL=
ARBITRUM_RPC_URL=
BASE_RPC_URL=
OPTIMISM_RPC_URL=

# Block explorer API keys
ARBISCAN_API_KEY=
BASESCAN_API_KEY=
OPTIMISTIC_ETHERSCAN_API_KEY=

# Chain-specific addresses
AAVE_POOL=
WRAPPED_NATIVE=
```

---

## Testing Requirements

### Unit Tests

```solidity
// test/unit/Liquidator.t.sol
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test} from "forge-std/Test.sol";
import {Liquidator} from "src/Liquidator.sol";

contract LiquidatorTest is Test {
    Liquidator public liquidator;

    function setUp() external {
        // Setup test environment
    }

    // Success cases
    function test_Liquidate_Success() external { }
    function test_Liquidate_MaxDebtAmount() external { }
    function test_RescueTokens_ERC20() external { }
    function test_RescueTokens_Native() external { }
    function test_SetAdapter_Success() external { }

    // Revert cases
    function test_Liquidate_RevertWhen_NotOwner() external { }
    function test_Liquidate_RevertWhen_SlippageExceeded() external { }
    function test_Liquidate_RevertWhen_UnknownAdapter() external { }
    function test_SetAdapter_RevertWhen_NotOwner() external { }
    function test_RescueTokens_RevertWhen_NotOwner() external { }

    // Event emission
    function test_Liquidate_EmitsLiquidationEvent() external { }
    function test_SetAdapter_EmitsAdapterUpdatedEvent() external { }
}
```

### Fuzz Tests

```solidity
// test/fuzz/FuzzLiquidator.t.sol
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test} from "forge-std/Test.sol";
import {Liquidator} from "src/Liquidator.sol";

contract FuzzLiquidatorTest is Test {
    Liquidator public liquidator;

    function setUp() external {
        // Setup with mock adapters
    }

    /// @notice Fuzz test for liquidation with bounded amounts
    function testFuzz_Liquidate_AmountBounds(
        uint96 debtAmount,
        uint96 minAmountOut
    ) external {
        // Use uint96 to avoid overflow issues
        debtAmount = uint96(bound(debtAmount, 1e6, 1e24));
        minAmountOut = uint96(bound(minAmountOut, 0, debtAmount));

        // Use assume for invalid input exclusion
        vm.assume(debtAmount > minAmountOut);

        // Test execution...
    }

    /// @notice Fuzz test for swap data decoding
    function testFuzz_SwapDataDecoding(uint8 adapterType) external {
        adapterType = uint8(bound(adapterType, 0, 2));
        // Verify decoding works for all adapter types
    }

    /// @notice Fuzz test for rescue tokens
    function testFuzz_RescueTokens(uint96 amount, bool useMax) external {
        amount = uint96(bound(amount, 1, type(uint96).max));
        // Test rescue functionality with various amounts
    }
}
```

### Invariant Tests

```solidity
// test/invariant/handlers/LiquidatorHandler.sol
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test} from "forge-std/Test.sol";
import {Liquidator} from "src/Liquidator.sol";

/// @title LiquidatorHandler
/// @notice Handler contract for bounded invariant testing
contract LiquidatorHandler is Test {
    Liquidator public liquidator;

    // Ghost variables for state tracking
    uint256 public ghost_totalLiquidations;
    uint256 public ghost_totalProfit;
    mapping(address => uint256) public ghost_adapterUsageCount;

    // Actor management
    address[] public actors;
    address internal currentActor;

    modifier useActor(uint256 actorSeed) {
        currentActor = actors[bound(actorSeed, 0, actors.length - 1)];
        vm.startPrank(currentActor);
        _;
        vm.stopPrank();
    }

    constructor(Liquidator _liquidator) {
        liquidator = _liquidator;
        // Initialize actors
        for (uint256 i = 0; i < 3; i++) {
            actors.push(makeAddr(string(abi.encode("actor", i))));
        }
    }

    function setAdapter(uint8 adapterType, address adapter, uint256 actorSeed) external useActor(actorSeed) {
        adapterType = uint8(bound(adapterType, 0, 2));
        // Only owner can set adapters - this should revert for non-owners
        try liquidator.setAdapter(adapterType, adapter) {
            // Track successful adapter updates
        } catch {
            // Expected for non-owners
        }
    }
}

// test/invariant/LiquidatorInvariant.t.sol
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test} from "forge-std/Test.sol";
import {Liquidator} from "src/Liquidator.sol";
import {LiquidatorHandler} from "./handlers/LiquidatorHandler.sol";

contract LiquidatorInvariantTest is Test {
    Liquidator public liquidator;
    LiquidatorHandler public handler;

    function setUp() external {
        // Deploy contracts
        // liquidator = new Liquidator(...);
        // handler = new LiquidatorHandler(liquidator);
        // targetContract(address(handler));
    }

    /// @notice Contract should never hold funds after operations complete
    function invariant_NoStuckFunds() external view {
        assertEq(address(liquidator).balance, 0, "Native balance should be zero");
    }

    /// @notice Owner should never change unexpectedly
    function invariant_OwnerConsistent() external view {
        assertEq(liquidator.owner(), address(this), "Owner should not change");
    }

    /// @notice Registered adapters should be valid contracts
    function invariant_AdaptersValid() external view {
        for (uint8 i = 0; i < 3; i++) {
            address adapter = liquidator.adapters(i);
            if (adapter != address(0)) {
                assertTrue(adapter.code.length > 0, "Adapter should be a contract");
            }
        }
    }
}
```

### Fork Tests

```solidity
// test/fork/HyperLiquidFork.t.sol
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test, console} from "forge-std/Test.sol";
import {Liquidator} from "src/Liquidator.sol";
import {LiquidSwapAdapter} from "src/adapters/LiquidSwapAdapter.sol";

contract HyperLiquidForkTest is Test {
    Liquidator public liquidator;
    LiquidSwapAdapter public adapter;

    // HyperLiquid mainnet addresses
    address constant AAVE_POOL = address(0); // TODO: Add actual address
    address constant WHYPE = 0x5555555555555555555555555555555555555555;
    address constant LIQUIDSWAP_ROUTER = 0x744489Ee3d540777A66f2cf297479745e0852f7A;

    function setUp() external {
        string memory rpcUrl = vm.envString("HYPERLIQUID_RPC_URL");
        vm.createSelectFork(rpcUrl);

        // Deploy contracts on fork
        liquidator = new Liquidator(AAVE_POOL, WHYPE);
        adapter = new LiquidSwapAdapter(LIQUIDSWAP_ROUTER);
        liquidator.setAdapter(0, address(adapter));
    }

    function testFork_LiquidSwapAdapter_Swap() external {
        // Test swap against real DEX
    }

    function testFork_Liquidate_RealPosition() external {
        // Test against actual underwater position
        // This requires finding a liquidatable position on-chain
    }
}

// test/fork/ArbitrumFork.t.sol
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test, console} from "forge-std/Test.sol";
import {Liquidator} from "src/Liquidator.sol";
import {UniswapV3Adapter} from "src/adapters/UniswapV3Adapter.sol";

contract ArbitrumForkTest is Test {
    Liquidator public liquidator;
    UniswapV3Adapter public adapter;

    // Arbitrum mainnet addresses
    address constant AAVE_POOL = 0x794a61358D6845594F94dc1DB02A252b5b4814aD;
    address constant WETH = 0x82aF49447D8a07e3bd95BD0d56f35241523fBab1;
    address constant UNISWAP_ROUTER = 0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45;

    function setUp() external {
        string memory rpcUrl = vm.envString("ARBITRUM_RPC_URL");
        vm.createSelectFork(rpcUrl);

        liquidator = new Liquidator(AAVE_POOL, WETH);
        adapter = new UniswapV3Adapter(UNISWAP_ROUTER);
        liquidator.setAdapter(1, address(adapter));
    }

    function testFork_UniswapV3Adapter_SingleHop() external {
        // Test single-hop swap
    }

    function testFork_UniswapV3Adapter_MultiHop() external {
        // Test multi-hop swap
    }
}
```

---

## Security Requirements

### Access Control
- `onlyOwner` modifier on `liquidate()`, `rescueTokens()`, `setAdapter()`
- Use OpenZeppelin's `Ownable` for standardized ownership management

### Reentrancy Protection
- Apply `ReentrancyGuard` on `liquidate()` and `executeOperation()`
- Follow CEI (Checks-Effects-Interactions) pattern in flash loan callback

### Input Validation
- Validate all addresses are non-zero where required
- Validate amounts are positive where required
- Verify adapter exists before executing swap

### Slippage Protection
- Revert if `collateralReceived < minAmountOut`
- Custom error with received/expected amounts for debugging

### Flash Loan Security
- Verify `msg.sender == pool` in callback
- Verify `initiator == address(this)` in callback
- Never approve more than necessary

### Linting
Run `forge lint` before deployment:
```bash
forge lint --severity high --severity medium
```

---

## Chain Configuration

| Chain | Chain ID | Pool Address | WrappedNative | Default Adapter |
|-------|----------|--------------|---------------|-----------------|
| HyperLiquid | 998 | TBD | WHYPE (`0x5555...`) | LiquidSwap (0) |
| Arbitrum | 42161 | `0x794a61358D6845594F94dc1DB02A252b5b4814aD` | WETH | UniswapV3 (1) |
| Base | 8453 | `0xA238Dd80C259a72e81d7e4664a9801593F98d1c5` | WETH | UniswapV3 (1) |
| Optimism | 10 | `0x794a61358D6845594F94dc1DB02A252b5b4814aD` | WETH | UniswapV3 (1) |
| Celo | 42220 | TBD | CELO | UniswapV3 (1) |

---

## Deployment

### Base Deployment Script

```solidity
// script/Deploy.s.sol
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Script, console} from "forge-std/Script.sol";
import {Liquidator} from "src/Liquidator.sol";
import {LiquidSwapAdapter} from "src/adapters/LiquidSwapAdapter.sol";
import {UniswapV3Adapter} from "src/adapters/UniswapV3Adapter.sol";
import {DirectAdapter} from "src/adapters/DirectAdapter.sol";

abstract contract DeployBase is Script {
    Liquidator public liquidator;

    function _deploy(
        address pool,
        address wrappedNative,
        address liquidSwapRouter,
        address uniswapRouter
    ) internal {
        uint256 deployerKey = vm.envUint("PRIVATE_KEY");

        vm.startBroadcast(deployerKey);

        // Deploy core contract
        liquidator = new Liquidator(pool, wrappedNative);
        console.log("Liquidator deployed:", address(liquidator));

        // Deploy adapters
        DirectAdapter directAdapter = new DirectAdapter();
        liquidator.setAdapter(2, address(directAdapter));
        console.log("DirectAdapter:", address(directAdapter));

        if (liquidSwapRouter != address(0)) {
            LiquidSwapAdapter lsAdapter = new LiquidSwapAdapter(liquidSwapRouter);
            liquidator.setAdapter(0, address(lsAdapter));
            console.log("LiquidSwapAdapter:", address(lsAdapter));
        }

        if (uniswapRouter != address(0)) {
            UniswapV3Adapter uniAdapter = new UniswapV3Adapter(uniswapRouter);
            liquidator.setAdapter(1, address(uniAdapter));
            console.log("UniswapV3Adapter:", address(uniAdapter));
        }

        vm.stopBroadcast();

        // Verify deployment
        require(liquidator.owner() == vm.addr(deployerKey), "Owner mismatch");
    }
}
```

### Chain-Specific Scripts

```solidity
// script/DeployHyperLiquid.s.sol
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {DeployBase} from "./Deploy.s.sol";

contract DeployHyperLiquid is DeployBase {
    address constant POOL = address(0); // TODO: Add address
    address constant WHYPE = 0x5555555555555555555555555555555555555555;
    address constant LIQUIDSWAP_ROUTER = 0x744489Ee3d540777A66f2cf297479745e0852f7A;

    function run() external {
        _deploy(POOL, WHYPE, LIQUIDSWAP_ROUTER, address(0));
    }
}

// script/DeployArbitrum.s.sol
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {DeployBase} from "./Deploy.s.sol";

contract DeployArbitrum is DeployBase {
    address constant POOL = 0x794a61358D6845594F94dc1DB02A252b5b4814aD;
    address constant WETH = 0x82aF49447D8a07e3bd95BD0d56f35241523fBab1;
    address constant UNISWAP_ROUTER = 0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45;

    function run() external {
        _deploy(POOL, WETH, address(0), UNISWAP_ROUTER);
    }
}
```

### Deployment Commands

```bash
# Simulate locally first
forge script script/DeployArbitrum.s.sol -vvvv

# Deploy to Arbitrum with verification
forge script script/DeployArbitrum.s.sol \
  --rpc-url arbitrum \
  --broadcast \
  --verify \
  -vvvv \
  --interactives 1

# Resume failed deployment
forge script script/DeployArbitrum.s.sol \
  --rpc-url arbitrum \
  --resume
```

---

## Migration Checklist

### Keep from Old Contract
- [x] `rescueTokens()` function logic
- [x] `ReentrancyGuard` usage
- [x] `SafeERC20` usage
- [x] Flash loan callback structure (`executeOperation`)
- [x] Amount adjustment logic (lines 76-87 for collateral variance)
- [x] Native token wrapping (WHYPE handling)

### Modify
- [ ] Replace `Swap[][] hops, address[] tokens` with `bytes swapData`
- [ ] Extract swap execution to adapter pattern
- [ ] Make router addresses configurable (constructor/setAdapter)
- [ ] Add `Liquidation` event with profit tracking
- [ ] Return `uint256 profit` from `liquidate()`
- [ ] Use custom errors instead of require strings

### Remove
- [ ] Hardcoded `liquidSwapRouter` address
- [ ] Hardcoded `WHYPE` address (pass via constructor)
- [ ] Direct `ILiquidSwap` import in main contract

---

## Deliverables

1. **Core Contracts**
   - `Liquidator.sol` with adapter registry pattern
   - Full NatSpec documentation

2. **Adapters**
   - `LiquidSwapAdapter.sol` (HyperLiquid)
   - `UniswapV3Adapter.sol` (Arbitrum/Base/Optimism)
   - `DirectAdapter.sol` (no-op passthrough)

3. **Tests**
   - Unit tests (>90% coverage)
   - Fuzz tests with bounded inputs
   - Invariant tests with handler pattern
   - Fork tests for HyperLiquid and Arbitrum

4. **Scripts**
   - Base deployment script
   - Per-chain deployment scripts with verification

5. **Documentation**
   - NatSpec on all public/external functions
   - This specification document

---

## Commands Reference

```bash
# Build
forge build

# Test
forge test                              # Run all tests
forge test --match-test testFork_       # Run fork tests only
forge test -vvvv                        # Verbose output

# Coverage
forge coverage --report lcov

# Lint
forge lint

# Gas snapshot
forge snapshot

# Deploy
forge script script/DeployArbitrum.s.sol --rpc-url arbitrum --broadcast --verify
```
