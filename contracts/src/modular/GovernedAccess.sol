// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {AccessControl} from "@openzeppelin/contracts/access/AccessControl.sol";

/// @notice Bootstrap once, then pin role administration to one governance contract.
/// @dev Prevents a delayed grant of DEFAULT_ADMIN_ROLE to an EOA from removing future delays.
abstract contract GovernedAccess is AccessControl {
    address public governor;
    address private immutable bootstrapAdmin;

    constructor(address admin) {
        require(admin != address(0), "Zero admin");
        bootstrapAdmin = admin;
        _grantRole(DEFAULT_ADMIN_ROLE, admin);
    }

    function sealGovernance(address governance) external {
        require(msg.sender == bootstrapAdmin, "Only bootstrap");
        _sealGovernance(governance);
    }

    function _sealGovernance(address governance) internal {
        require(governor == address(0) && governance.code.length > 0, "Governance sealed");
        governor = governance;
        _revokeRole(DEFAULT_ADMIN_ROLE, bootstrapAdmin);
        _grantRole(DEFAULT_ADMIN_ROLE, governance);
    }

    function grantRole(bytes32 role, address account) public override {
        require(role != DEFAULT_ADMIN_ROLE, "Admin pinned");
        super.grantRole(role, account);
    }

    function revokeRole(bytes32 role, address account) public override {
        require(role != DEFAULT_ADMIN_ROLE, "Admin pinned");
        super.revokeRole(role, account);
    }

    function renounceRole(bytes32 role, address callerConfirmation) public override {
        require(role != DEFAULT_ADMIN_ROLE, "Admin pinned");
        super.renounceRole(role, callerConfirmation);
    }
}
