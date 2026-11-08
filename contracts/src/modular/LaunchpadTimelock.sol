// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {TimelockController} from "@openzeppelin/contracts/governance/TimelockController.sol";

/// @notice No bootstrap EOA admin; the normal-change delay is fixed at 48h.
/// @dev Proposer/executor should be a reviewed multisig in a real deployment.
contract LaunchpadTimelock is TimelockController {
    uint256 public constant MINIMUM_DELAY = 48 hours;

    constructor(address[] memory proposers, address[] memory executors)
        TimelockController(MINIMUM_DELAY, proposers, executors, address(0))
    {
        require(proposers.length > 0 && executors.length > 0, "Missing governance");
        for (uint256 i; i < proposers.length; i++) {
            require(proposers[i] != address(0), "Zero proposer");
        }
        for (uint256 i; i < executors.length; i++) {
            require(executors[i] != address(0), "Zero executor");
        }
    }

    function updateDelay(uint256) external pure override {
        revert("Fixed 48h delay");
    }
}
