// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {TimelockController} from "@openzeppelin/contracts/governance/TimelockController.sol";

/// @notice No bootstrap EOA admin; the delay is fixed per deployment: Sepolia 60s; other chains 48h.
/// @dev Proposer/executor should be a reviewed multisig in a real deployment.
contract LaunchpadTimelock is TimelockController {
    uint256 public immutable MINIMUM_DELAY;

    constructor(address[] memory proposers, address[] memory executors)
        TimelockController(block.chainid == 11155111 ? 60 seconds : 48 hours, proposers, executors, address(0))
    {
        MINIMUM_DELAY = block.chainid == 11155111 ? 60 seconds : 48 hours;
        require(proposers.length > 0 && executors.length > 0, "Missing governance");
        for (uint256 i; i < proposers.length; i++) {
            require(proposers[i] != address(0), "Zero proposer");
        }
        for (uint256 i; i < executors.length; i++) {
            require(executors[i] != address(0), "Zero executor");
        }
    }

    function updateDelay(uint256) external pure override {
        revert("Fixed deployment delay");
    }
}
