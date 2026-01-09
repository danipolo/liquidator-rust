// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test} from "forge-std/Test.sol";
import {Liquidator} from "../../../src/Liquidator.sol";
import {MockERC20, MockSwapAdapter} from "../../utils/MockContracts.sol";
import {TestConstants} from "../../utils/TestConstants.sol";

/// @title LiquidatorHandler
/// @notice Handler contract for bounded invariant testing
contract LiquidatorHandler is Test {
    Liquidator public liquidator;
    MockERC20 public collateral;
    MockERC20 public debt;
    MockSwapAdapter public adapter;

    // Ghost variables for state tracking
    uint256 public ghost_totalLiquidations;
    uint256 public ghost_totalProfit;
    uint256 public ghost_adapterUpdates;
    uint256 public ghost_rescueOperations;
    mapping(uint8 => uint256) public ghost_adapterUsageCount;

    // Actor management
    address[] public actors;
    address internal currentActor;
    address public owner;

    modifier useActor(uint256 actorSeed) {
        currentActor = actors[bound(actorSeed, 0, actors.length - 1)];
        vm.startPrank(currentActor);
        _;
        vm.stopPrank();
    }

    modifier useOwner() {
        vm.startPrank(owner);
        _;
        vm.stopPrank();
    }

    constructor(
        Liquidator _liquidator,
        MockERC20 _collateral,
        MockERC20 _debt,
        MockSwapAdapter _adapter,
        address _owner
    ) {
        liquidator = _liquidator;
        collateral = _collateral;
        debt = _debt;
        adapter = _adapter;
        owner = _owner;

        // Initialize actors
        for (uint256 i = 0; i < 5; i++) {
            address actor = makeAddr(string(abi.encodePacked("actor", i)));
            actors.push(actor);
        }
    }

    /// @notice Handler for setting adapters
    function setAdapter(uint8 adapterType, address adapterAddress, uint256 actorSeed) external useActor(actorSeed) {
        adapterType = uint8(bound(adapterType, 0, 2));

        // Only owner can set adapters - this should revert for non-owners
        try liquidator.setAdapter(adapterType, adapterAddress) {
            ghost_adapterUpdates++;
            ghost_adapterUsageCount[adapterType]++;
        } catch {
            // Expected for non-owners
        }
    }

    /// @notice Handler for owner setting adapters (should succeed)
    function ownerSetAdapter(uint8 adapterType, address adapterAddress) external useOwner {
        adapterType = uint8(bound(adapterType, 0, 2));

        liquidator.setAdapter(adapterType, adapterAddress);
        ghost_adapterUpdates++;
        ghost_adapterUsageCount[adapterType]++;
    }

    /// @notice Handler for rescuing ERC20 tokens
    function rescueTokensERC20(uint96 amount, bool max, uint256 actorSeed) external useActor(actorSeed) {
        amount = uint96(bound(amount, 0, debt.balanceOf(address(liquidator))));

        try liquidator.rescueTokens(address(debt), amount, max, currentActor) {
            ghost_rescueOperations++;
        } catch {
            // Expected for non-owners
        }
    }

    /// @notice Handler for owner rescuing tokens
    function ownerRescueTokens(uint96 amount, bool max) external useOwner {
        uint256 balance = debt.balanceOf(address(liquidator));
        if (balance == 0) return;

        amount = uint96(bound(amount, 1, balance));

        liquidator.rescueTokens(address(debt), amount, max, owner);
        ghost_rescueOperations++;
    }

    /// @notice Handler for rescuing native tokens
    function rescueNativeTokens(uint96 amount, bool max, uint256 actorSeed) external useActor(actorSeed) {
        amount = uint96(bound(amount, 0, address(liquidator).balance));

        try liquidator.rescueTokens(address(0), amount, max, currentActor) {
            ghost_rescueOperations++;
        } catch {
            // Expected for non-owners
        }
    }

    /// @notice Sends tokens to liquidator (simulating stuck tokens)
    function sendTokensToLiquidator(uint96 amount) external {
        amount = uint96(bound(amount, 1, 1e24));
        debt.mint(address(liquidator), amount);
    }

    /// @notice Sends native tokens to liquidator
    function sendNativeToLiquidator(uint96 amount) external {
        amount = uint96(bound(amount, 1, 1e18));
        vm.deal(address(liquidator), address(liquidator).balance + amount);
    }

    /// @notice Returns the number of actors
    function getActorCount() external view returns (uint256) {
        return actors.length;
    }
}
