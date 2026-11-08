// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
interface ModularDeployVm {
 function envAddress(string calldata) external returns(address);
 function envBytes32(string calldata) external returns(bytes32);
 function envOr(string calldata,bool) external returns(bool);
 function getCode(string calldata) external returns(bytes memory);
 function startBroadcast(address) external;function stopBroadcast() external;
}
/// @notice Deploys an INACTIVE migration target only; no gate roles, funding, retirement or cutover.
contract DeployModular {
 ModularDeployVm constant vm=ModularDeployVm(address(uint160(uint256(keccak256("hevm cheat code")))));
 struct Deployment {address governance;address modules;address factory;address gate;}
 function deploy(string memory artifact,bytes memory args) private returns(address target){
  bytes memory init=abi.encodePacked(vm.getCode(artifact),args);require(init.length<=49152,"Initcode limit exceeded");
  assembly{target:=create(0,add(init,32),mload(init))}require(target.code.length>0,"Deployment failed");
 }
 function run() external returns(Deployment memory d){
  require(block.chainid==11155111 || (block.chainid==31337 && vm.envOr("ALLOW_LOCAL_DEPLOY",false)),"Test deployment only");
  address proposer=vm.envAddress("GOVERNANCE_PROPOSER");require(proposer!=address(0),"Missing governance proposer");
  require(proposer.code.length>0 || vm.envOr("ALLOW_TEST_GOVERNANCE_EOA",false),"Reviewed multisig required; explicit test-only EOA exception needed");
  address[] memory participants=new address[](1);participants[0]=proposer;
  address prl=vm.envAddress("STAKE_TOKEN");address source=vm.envAddress("LEGACY_STAKE");address credits=vm.envAddress("LEGACY_CREDITS");
  require(prl.code.length>0 && source.code.length>0 && credits.code.length>0,"Missing deployed source contracts");
  address deployer=vm.envAddress("DEPLOYER_ADDRESS");
  vm.startBroadcast(deployer);
  d.governance=deploy("LaunchpadTimelock.sol:LaunchpadTimelock",abi.encode(participants,participants));
  d.modules=deploy("ModuleDeployer.sol:ModuleDeployer","");
  d.factory=deploy("TestnetProtocolFactory.sol:TestnetProtocolFactory",abi.encode(prl,d.governance,vm.envAddress("STAKE_REGISTRAR"),vm.envAddress("STAKE_VERIFIER"),vm.envAddress("EMERGENCY_GUARDIAN"),vm.envAddress("CREDIT_SETTLER"),d.modules));
  d.gate=deploy("LegacyMigrationGate.sol:LegacyMigrationGate",abi.encode(d.factory,source,credits,vm.envBytes32("MIGRATION_PROJECT_KEY")));
  vm.stopBroadcast();
 }
}
