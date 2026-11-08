// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {ComputeCredits} from "./ComputeCredits.sol";
/// @notice Counts actual provider-delivered burns; failed/trial/public-pool usage must never be submitted here.
contract ComputeCreditsV2 is ComputeCredits {
    uint256 public constant COLLATERAL_ACCOUNTING_VERSION=2;
    mapping(bytes32=>uint256) public providerSettledCredits;
    event ProviderDeliveryRecorded(bytes32 indexed project,uint256 amount,uint256 cumulative);
    constructor(address staking,address admin,address settler,address manager) ComputeCredits(staking,admin,settler,manager) {}
    function _recordProviderDelivery(bytes32 project,uint256 amount) internal override {
        providerSettledCredits[project]+=amount;
        emit ProviderDeliveryRecorded(project,amount,providerSettledCredits[project]);
    }
}
