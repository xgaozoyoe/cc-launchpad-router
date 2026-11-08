// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {LaunchStakeV2} from "../src/LaunchStakeV2.sol";
import {ComputeCreditsV2} from "../src/ComputeCreditsV2.sol";
interface VmProgressive {function envAddress(string calldata) external returns(address);function startBroadcast(address) external;function stopBroadcast() external;}
/// @notice New-project V2 deployment ONLY. Does not import/release legacy stake, balances or ETH.
contract DeployProgressive {
 VmProgressive constant vm=VmProgressive(address(uint160(uint256(keccak256("hevm cheat code")))));
 function run() external returns(LaunchStakeV2 stake,ComputeCreditsV2 credits){require(block.chainid==11155111||block.chainid==31337,"Test only");address deployer=vm.envAddress("DEPLOYER_ADDRESS");address token=vm.envAddress("STAKE_TOKEN");address verifier=vm.envAddress("STAKE_VERIFIER");address settler=vm.envAddress("CREDIT_SETTLER");address manager=vm.envAddress("CREDIT_MANAGER");require(verifier!=address(0)&&settler!=address(0)&&manager!=address(0),"Zero role");vm.startBroadcast(deployer);stake=new LaunchStakeV2(IERC20(token),deployer);credits=new ComputeCreditsV2(address(stake),deployer,settler,manager);stake.bindCreditContract(address(credits));stake.grantRole(stake.VERIFIER_ROLE(),verifier);vm.stopBroadcast();}
}
