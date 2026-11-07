// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {LaunchStake} from "../src/LaunchStake.sol";
import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
interface Vm { function prank(address) external; function warp(uint256) external; function expectRevert() external; }
contract MockPRL is ERC20 { constructor() ERC20("Mock PRL", "mPRL") {} function mint(address to,uint256 n) external { _mint(to,n); } }
contract FeePRL is MockPRL { function _update(address f,address t,uint256 n) internal override { if(f!=address(0)&&t!=address(0)){super._update(f,address(0),n/10);super._update(f,t,n-n/10);}else super._update(f,t,n); } }
contract LaunchStakeTest {
    Vm constant vm = Vm(address(uint160(uint256(keccak256("hevm cheat code")))));
    MockPRL token; LaunchStake escrow; address provider=address(0x1234); bytes32 id=sha256("project-1"); uint256 amount=2000 ether;
    function setUp() public {
        vm.warp(1000); token=new MockPRL(); escrow=new LaunchStake(token,address(this));
        escrow.grantRole(escrow.VERIFIER_ROLE(),address(this)); escrow.grantRole(escrow.SETTLER_ROLE(),address(this));
        token.mint(provider,amount); escrow.registerProject(id,provider,amount);
        vm.prank(provider);token.approve(address(escrow),amount);
    }
    function deposit() internal { vm.prank(provider);escrow.stake(id); }
    function testStakeRecordsBlockDeadlineAndAmount() public {
        deposit();(uint8 status,uint256 n,uint64 at,uint64 deadline,address who)=escrow.stakeState(id);
        require(status==2&&n==amount&&at==1000&&deadline==87400&&who==provider);require(token.balanceOf(address(escrow))==amount);
    }
    function testUnauthorizedAndDuplicateStakeFail() public {
        vm.expectRevert();escrow.stake(id); deposit();vm.expectRevert();vm.prank(provider);escrow.stake(id);
    }
    function testOnlyRegisteredProjectsAndAdmin() public {
        bytes32 other=sha256("other");bytes32 unknown=sha256("unknown");
        vm.expectRevert();vm.prank(provider);escrow.registerProject(other,provider,amount);
        vm.expectRevert();vm.prank(provider);escrow.stake(unknown);
        vm.expectRevert();escrow.registerProject(id,provider,amount);
    }
    function testVerifyMustBeAuthorizedAndBeforeDeadline() public {
        deposit();vm.expectRevert();vm.prank(provider);escrow.verifyProject(id);
        vm.warp(87400);vm.expectRevert();escrow.verifyProject(id);
    }
    function testRefundAtDeadlineExactlyOnce() public {
        deposit();vm.expectRevert();vm.prank(provider);escrow.refundExpired(id);
        vm.warp(87400);vm.prank(provider);escrow.refundExpired(id);
        require(token.balanceOf(provider)==amount);vm.expectRevert();vm.prank(provider);escrow.refundExpired(id);
    }
    function testVerifiedStakeCannotRefundButCanSettle() public {
        deposit();escrow.verifyProject(id);vm.warp(999999);vm.expectRevert();vm.prank(provider);escrow.refundExpired(id);
        bytes32 reason=sha256("complete");
        vm.expectRevert();vm.prank(provider);escrow.releaseStake(id,reason);
        escrow.releaseStake(id,reason);require(token.balanceOf(provider)==amount);
    }
    function testFeeTokenIsRejectedWithoutRecordingStake() public {
        FeePRL fee=new FeePRL();LaunchStake other=new LaunchStake(fee,address(this));other.registerProject(id,provider,amount);
        fee.mint(provider,amount);vm.prank(provider);fee.approve(address(other),amount);
        vm.expectRevert();vm.prank(provider);other.stake(id);(uint8 status,,,,)=other.stakeState(id);require(status==1);
    }
}
