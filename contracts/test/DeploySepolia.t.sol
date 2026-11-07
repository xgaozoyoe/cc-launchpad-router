// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {DeploySepolia} from "../script/DeploySepolia.s.sol";
import {TestPRL} from "../src/TestPRL.sol";
import {LaunchStake} from "../src/LaunchStake.sol";
interface VmDeployTest {
    function setEnv(string calldata name, string calldata value) external;
    function chainId(uint256 chainId) external;
    function expectRevert() external;
    function prank(address sender) external;
}
contract DeploySepoliaTest {
    VmDeployTest constant vm=VmDeployTest(address(uint160(uint256(keccak256("hevm cheat code")))));
    address constant DEPLOYER=0x0000000000000000000000000000000000001001;
    address constant ADMIN=0x0000000000000000000000000000000000001002;
    address constant VERIFIER=0x0000000000000000000000000000000000001003;
    address constant SETTLER=0x0000000000000000000000000000000000001004;
    function setUp() public {
        vm.setEnv("DEPLOYER_ADDRESS","0x0000000000000000000000000000000000001001");
        vm.setEnv("STAKE_ADMIN","0x0000000000000000000000000000000000001002");
        vm.setEnv("STAKE_VERIFIER","0x0000000000000000000000000000000000001003");
        vm.setEnv("STAKE_SETTLER","0x0000000000000000000000000000000000001004");
        vm.setEnv("ALLOW_LOCAL_DEPLOY","false");
        vm.setEnv("TEST_PRL_INITIAL_SUPPLY","1000000000000000000000000");
    }
    function testDeploymentAssignsRolesAndRenouncesTemporaryAdmin() public {
        vm.chainId(11155111);
        (TestPRL token,LaunchStake escrow)=new DeploySepolia().run();
        require(address(escrow.stakeToken())==address(token));
        require(token.owner()==ADMIN && token.balanceOf(ADMIN)==1_000_000 ether);
        require(escrow.hasRole(escrow.DEFAULT_ADMIN_ROLE(),ADMIN));
        require(!escrow.hasRole(escrow.DEFAULT_ADMIN_ROLE(),DEPLOYER));
        require(escrow.hasRole(escrow.VERIFIER_ROLE(),VERIFIER));
        require(escrow.hasRole(escrow.SETTLER_ROLE(),SETTLER));
        vm.expectRevert();vm.prank(DEPLOYER);token.mint(DEPLOYER,1 ether);
    }
    function testMainnetDeploymentRejected() public {
        vm.chainId(1);DeploySepolia script=new DeploySepolia();vm.expectRevert();script.run();
    }
    function testLocalDeploymentRejectedByDefault() public {
        vm.chainId(31337);DeploySepolia script=new DeploySepolia();vm.expectRevert();script.run();
    }
}
