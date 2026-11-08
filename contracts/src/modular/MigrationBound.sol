// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @notice A single migration portal, installed once by the creating protocol factory.
abstract contract MigrationBound {
    address private immutable migrationBootstrap = msg.sender;
    address public migrationGate;
    event MigrationGateBound(address indexed gate);
    modifier onlyMigrationGate() {
        require(msg.sender == migrationGate && migrationGate != address(0), "Only migration gate");
        _;
    }

    function bindMigrationGate(address gate) external {
        require(
            msg.sender == migrationBootstrap && migrationGate == address(0) && gate.code.length > 0,
            "Migration binding sealed"
        );
        migrationGate = gate;
        emit MigrationGateBound(gate);
    }
}
