# Liquidator Smart Contracts

Multi-chain AAVE liquidator contracts with adapter-based DEX support.

## Setup

### Install Dependencies

```shell
cd contracts
forge install foundry-rs/forge-std OpenZeppelin/openzeppelin-contracts
```

Or manually:
```shell
git clone --depth 1 https://github.com/foundry-rs/forge-std lib/forge-std
git clone --depth 1 https://github.com/OpenZeppelin/openzeppelin-contracts lib/openzeppelin-contracts
```

### Environment

Copy the root `.env.example` to `.env` and fill in your values:
```shell
cp ../.env.example ../.env
```

This project uses a wrapper script (`forge.sh`) to load environment variables from the parent directory, since Foundry only loads `.env` from the project root.

## Usage

Use `./forge.sh` instead of `forge` for commands that need environment variables (deployment, fork tests). Standard commands like `build` and `test` work with plain `forge`.

### Build

```shell
forge build
```

### Test

```shell
forge test
```

Run with verbosity for more details:
```shell
forge test -vvv
```

### Fork Tests

Run against live networks (requires RPC URLs in `.env`):
```shell
./forge.sh test --match-contract ArbitrumForkTest --fork-url arbitrum -vvv
./forge.sh test --match-contract HyperLiquidForkTest --fork-url hyperliquid -vvv
```

### Deploy

```shell
# Simulate deployment (no broadcast)
./forge.sh script script/DeployArbitrum.s.sol --rpc-url arbitrum -vvv

# Deploy to Arbitrum
./forge.sh script script/DeployArbitrum.s.sol --rpc-url arbitrum --broadcast -vvv

# Deploy to Arbitrum with verification
./forge.sh script script/DeployArbitrum.s.sol --rpc-url arbitrum --broadcast --verify -vvv

# Deploy to HyperLiquid
./forge.sh script script/DeployHyperLiquid.s.sol --rpc-url hyperliquid --broadcast -vvv
```

### Gas Report

```shell
forge test --gas-report
```

## Architecture

- `src/Liquidator.sol` - Main contract with flash loan liquidation logic
- `src/adapters/` - DEX adapters (UniswapV3, LiquidSwap, Direct)
- `src/interfaces/` - Contract interfaces
- `src/libraries/` - Shared utilities (SwapDataDecoder)
