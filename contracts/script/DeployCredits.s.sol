// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {ComputeCredits} from "../src/ComputeCredits.sol";
interface VmCredits {function envAddress(string calldata) external returns(address);function startBroadcast(address) external;function stopBroadcast() external;}
contract DeployCredits {
 VmCredits constant vm=VmCredits(address(uint160(uint256(keccak256("hevm cheat code")))));
 function run() external returns(ComputeCredits credits){require(block.chainid==11155111||block.chainid==31337,"Test only");address deployer=vm.envAddress("DEPLOYER_ADDRESS");vm.startBroadcast(deployer);credits=new ComputeCredits(vm.envAddress("STAKING_CONTRACT"),deployer,vm.envAddress("CREDIT_SETTLER"),vm.envAddress("CREDIT_MANAGER"));vm.stopBroadcast();}
}
