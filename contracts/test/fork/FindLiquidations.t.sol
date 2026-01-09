// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test, console} from "forge-std/Test.sol";

interface IPool {
    function getUserAccountData(address user) external view returns (
        uint256 totalCollateralBase,
        uint256 totalDebtBase,
        uint256 availableBorrowsBase,
        uint256 currentLiquidationThreshold,
        uint256 ltv,
        uint256 healthFactor
    );
}

/// @title FindLiquidationsTest
/// @notice Fork test to find liquidation events by rolling back to different blocks
contract FindLiquidationsTest is Test {
    address constant AAVE_POOL = 0x794a61358D6845594F94dc1DB02A252b5b4814aD;

    // Known addresses that have had positions on AAVE Arbitrum
    address[] public knownAddresses;

    bool public forkEnabled;
    uint256 public forkId;

    function setUp() external {
        try vm.envString("ARBITRUM_RPC_URL") returns (string memory rpcUrl) {
            if (bytes(rpcUrl).length > 0) {
                forkId = vm.createSelectFork(rpcUrl);
                forkEnabled = true;
            }
        } catch {
            forkEnabled = false;
        }
    }

    modifier onlyFork() {
        if (!forkEnabled) return;
        _;
    }

    /// @notice Search for positions at current block
    function testFork_FindPositionsAtCurrentBlock() external onlyFork {
        console.log("Current block:", block.number);
        console.log("Searching for positions with debt...");

        // Real at-risk positions from BlockAnalitica
        address[10] memory candidates = [
            0x308A31d418f62711D5D71d71fDBFcd74968883F8, // HF: 0.965 - LIQUIDATABLE
            0xf6a52dAFf4a81202A04864dA210A431f081f6bB0, // HF: 0.996 - at risk
            0x1f79618e870fd5b5C3320106cb368125723B6245, // Your address
            0x0000000000000000000000000000000000000001,
            0x0000000000000000000000000000000000000002,
            0x0000000000000000000000000000000000000003,
            0x0000000000000000000000000000000000000004,
            0x0000000000000000000000000000000000000005,
            0x0000000000000000000000000000000000000006,
            0x0000000000000000000000000000000000000007
        ];

        IPool pool = IPool(AAVE_POOL);
        uint256 foundPositions = 0;

        for (uint256 i = 0; i < candidates.length; i++) {
            try pool.getUserAccountData(candidates[i]) returns (
                uint256 collateral,
                uint256 debt,
                uint256,
                uint256,
                uint256,
                uint256 hf
            ) {
                if (debt > 0) {
                    foundPositions++;
                    console.log("---");
                    console.log("User:", candidates[i]);
                    console.log("Collateral (USD):", collateral / 1e8);
                    console.log("Debt (USD):", debt / 1e8);
                    console.log("Health Factor:", hf / 1e16, "%");
                    if (hf < 1e18) {
                        console.log(">>> LIQUIDATABLE <<<");
                    }
                }
            } catch {}
        }

        console.log("---");
        console.log("Found", foundPositions, "positions with debt");
    }

    /// @notice Roll back to a specific block and check for liquidatable positions
    function testFork_RollBackAndCheck() external onlyFork {
        // Roll back 10000 blocks
        uint256 targetBlock = block.number - 10000;
        vm.rollFork(targetBlock);

        console.log("Rolled back to block:", block.number);

        // Check same addresses at earlier block
        // Positions might have been liquidatable then
    }

    /// @notice Check a specific address at current and past blocks
    function testFork_CheckAddressHistory() external onlyFork {
        address target = 0x489ee077994B6658eAfA855C308275EAd8097C4A;
        IPool pool = IPool(AAVE_POOL);

        console.log("Checking address:", target);
        console.log("Current block:", block.number);

        // Check current
        (uint256 c1, uint256 d1,,,, uint256 hf1) = pool.getUserAccountData(target);
        console.log("Now - Collateral:", c1 / 1e8);
        console.log("Now - Debt:", d1 / 1e8);
        console.log("Now - HF:", hf1);

        // Roll back 50000 blocks (~2 days on Arbitrum)
        vm.rollFork(block.number - 50000);
        console.log("At block:", block.number);

        (uint256 c2, uint256 d2,,,, uint256 hf2) = pool.getUserAccountData(target);
        console.log("Past - Collateral:", c2 / 1e8);
        console.log("Past - Debt:", d2 / 1e8);
        console.log("Past - HF:", hf2);
    }

    /// @notice Placeholder when no fork
    function test_Placeholder() external pure {
        assertTrue(true);
    }
}
