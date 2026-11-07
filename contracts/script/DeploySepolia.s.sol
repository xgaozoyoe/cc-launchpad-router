// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {TestPRL} from "../src/TestPRL.sol";
import {LaunchStake} from "../src/LaunchStake.sol";

interface VmDeploy {
    function envAddress(string calldata name) external returns (address);
    function envOr(string calldata name, address defaultValue) external returns (address);
    function envOr(string calldata name, uint256 defaultValue) external returns (uint256);
    function envOr(string calldata name, bool defaultValue) external returns (bool);
    function startBroadcast(address signer) external;
    function stopBroadcast() external;
}

/// @notice Run with an encrypted Foundry account or hardware signer; never put a private key in this script.
contract DeploySepolia {
    VmDeploy private constant vm = VmDeploy(address(uint160(uint256(keccak256("hevm cheat code")))));
    event Deployment(uint256 chainId, address token, address escrow, address admin, address verifier, address settler);

    function run() public returns (TestPRL token, LaunchStake escrow) {
        require(block.chainid == 11155111 || (block.chainid == 31337 && vm.envOr("ALLOW_LOCAL_DEPLOY", false)), "Sepolia only");
        address deployer = vm.envAddress("DEPLOYER_ADDRESS");
        address admin = vm.envOr("STAKE_ADMIN", deployer);
        address verifier = vm.envOr("STAKE_VERIFIER", admin);
        address settler = vm.envOr("STAKE_SETTLER", admin);
        uint256 supply = vm.envOr("TEST_PRL_INITIAL_SUPPLY", uint256(1_000_000 ether));
        require(deployer != address(0) && admin != address(0) && verifier != address(0) && settler != address(0), "Zero role address");
        require(supply > 0, "Zero test supply");
        vm.startBroadcast(deployer);
        token = new TestPRL(admin, supply);
        escrow = new LaunchStake(token, deployer);
        escrow.grantRole(escrow.VERIFIER_ROLE(), verifier);
        escrow.grantRole(escrow.SETTLER_ROLE(), settler);
        if (admin != deployer) {
            escrow.grantRole(escrow.DEFAULT_ADMIN_ROLE(), admin);
            escrow.renounceRole(escrow.DEFAULT_ADMIN_ROLE(), deployer);
        }
        vm.stopBroadcast();
        require(address(escrow.stakeToken()) == address(token), "Wrong stake token");
        require(escrow.hasRole(escrow.DEFAULT_ADMIN_ROLE(), admin), "Missing admin");
        require(escrow.hasRole(escrow.VERIFIER_ROLE(), verifier), "Missing verifier");
        require(escrow.hasRole(escrow.SETTLER_ROLE(), settler), "Missing settler");
        require(token.owner() == admin && token.balanceOf(admin) == supply, "Wrong token allocation");
        emit Deployment(block.chainid, address(token), address(escrow), admin, verifier, settler);
    }
}
