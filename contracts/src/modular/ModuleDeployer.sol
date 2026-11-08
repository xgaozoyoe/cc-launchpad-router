// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {SaleRouter, SettlementModule} from "./Modules.sol";
import {PaymentEscrow} from "./PaymentEscrow.sol";
import {UsageEscrow} from "./UsageEscrow.sol";

/// @notice Stateless helper keeps protocol constructor below the EVM initcode limit.
contract ModuleDeployer {
    function deploy(PaymentEscrow payment, UsageEscrow usage, address governance, address operator)
        external
        returns (SaleRouter sale, SettlementModule settlement)
    {
        sale = new SaleRouter(payment);
        settlement = new SettlementModule(usage, governance, operator);
    }
}
