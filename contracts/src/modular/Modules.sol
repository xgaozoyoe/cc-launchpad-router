// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {GovernedAccess} from "./GovernedAccess.sol";
import {PaymentEscrow} from "./PaymentEscrow.sol";
import {UsageEscrow} from "./UsageEscrow.sol";

/// @notice Replaceable, stateless purchase entry. All value leaves atomically.
contract SaleRouter {
    PaymentEscrow public immutable paymentEscrow;

    constructor(PaymentEscrow payment) {
        paymentEscrow = payment;
    }

    function buyAndLock(bytes32 project, uint256 amount) external payable {
        paymentEscrow.purchase{value: msg.value}(project, msg.sender, amount);
    }
}

/// @notice Replaceable trusted receipt submitter, with no asset custody.
contract SettlementModule is GovernedAccess {
    bytes32 public constant OPERATOR_ROLE = keccak256("OPERATOR_ROLE");
    UsageEscrow public immutable usageEscrow;

    constructor(UsageEscrow usage, address admin, address operator) GovernedAccess(admin) {
        usageEscrow = usage;
        _sealGovernance(admin);
        _grantRole(OPERATOR_ROLE, operator);
    }

    function settleProvider(bytes32 reservation, bytes32 usageId, uint256 actual) external onlyRole(OPERATOR_ROLE) {
        usageEscrow.settleProvider(reservation, usageId, actual);
    }

    function releaseFailed(bytes32 reservation) external onlyRole(OPERATOR_ROLE) {
        usageEscrow.releaseFailed(reservation);
    }
}
