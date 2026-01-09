// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Script.sol";
import "../src/BalancesReader.sol";

contract TestBalancesReaderScript is Script {
    function run() external {
        // Deploy BalancesReader with Optimism addresses
        BalancesReader reader = new BalancesReader(
            0x69FA688f1Dc47d4B5d8029D5a35FB7a548310654,  // PoolDataProvider
            0xD81eb3728a631871a7eBBaD631b5f424909f0c77   // Oracle
        );

        console.log("BalancesReader deployed at:", address(reader));

        // Test with a known user that has positions (from LiFi router)
        address user = 0x1231DEB6f5749EF6cE6943a275A1D3E7486F4EaE;
        address pool = 0x794a61358D6845594F94dc1DB02A252b5b4814aD;

        console.log("Testing user:", user);

        // Get supplies
        try reader.getAllSuppliedBalancesWithPrices(pool, user) returns (BalancesReader.BalanceEntry[] memory supplies) {
            console.log("Supply positions:", supplies.length);
            for (uint256 i = 0; i < supplies.length && i < 3; i++) {
                console.log("  Token:", supplies[i].underlying);
                console.log("  Amount:", supplies[i].amount);
            }
        } catch Error(string memory reason) {
            console.log("Supply call failed:", reason);
        } catch {
            console.log("Supply call failed with unknown error");
        }

        // Get borrows
        try reader.getAllBorrowedBalancesWithPrices(pool, user) returns (BalancesReader.BalanceEntry[] memory borrows) {
            console.log("Borrow positions:", borrows.length);
            for (uint256 i = 0; i < borrows.length && i < 3; i++) {
                console.log("  Token:", borrows[i].underlying);
                console.log("  Amount:", borrows[i].amount);
            }
        } catch Error(string memory reason) {
            console.log("Borrow call failed:", reason);
        } catch {
            console.log("Borrow call failed with unknown error");
        }
    }
}
