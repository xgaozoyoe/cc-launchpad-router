// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {TestPRL} from "../src/TestPRL.sol";
import {LaunchStake} from "../src/LaunchStake.sol";
import {ComputeCredits} from "../src/ComputeCredits.sol";
import {LaunchpadTimelock} from "../src/modular/LaunchpadTimelock.sol";
import {TestnetProtocolFactory} from "../src/modular/TestnetProtocolFactory.sol";
import {LegacyMigrationGate, ILegacyStake, ILegacyCredits} from "../src/modular/LegacyMigrationGate.sol";
import {StakeVault} from "../src/modular/StakeVault.sol";
import {UsageEscrow} from "../src/modular/UsageEscrow.sol";
import {CreditToken} from "../src/modular/CreditToken.sol";
import {PaymentEscrow} from "../src/modular/PaymentEscrow.sol";
import {SettlementModule} from "../src/modular/Modules.sol";
import {ModuleDeployer} from "../src/modular/ModuleDeployer.sol";
import {ProjectRegistry} from "../src/modular/ProjectRegistry.sol";

interface MigrationVm {
    function warp(uint256) external;
    function roll(uint256) external;
    function setBlockhash(uint256, bytes32) external;
    function prank(address) external;
    function deal(address, uint256) external;
    function addr(uint256) external returns (address);
    function sign(uint256, bytes32) external returns (uint8, bytes32, bytes32);
    function expectRevert() external;
    function getCode(string calldata) external returns (bytes memory);
}

contract LegacyMigrationTest {
    MigrationVm constant vm = MigrationVm(address(uint160(uint256(keccak256("hevm cheat code")))));
    TestPRL prl;
    LaunchStake oldStake;
    ComputeCredits oldCredits;
    TestnetProtocolFactory target;
    LegacyMigrationGate gate;
    LaunchpadTimelock governance;
    bytes32 constant P = keccak256("existing launch");
    bytes32 constant MODEL = keccak256("qwen3-8b-awq");
    bytes32 constant CHECKPOINT = keccak256("canonical finalized checkpoint");
    StakeVault newStake;
    UsageEscrow newUsage;
    CreditToken newToken;
    PaymentEscrow newPayment;
    SettlementModule newSettler;
    address provider = address(0x1234);
    address[] holders;
    uint256[] keys;
    uint64 expiry;

    function setUp() public {
        vm.warp(1000);
        vm.deal(address(this), 1 ether);
        vm.deal(provider, 1 ether);
        prl = new TestPRL(address(this), 10000 ether);
        oldStake = new LaunchStake(prl, address(this));
        oldStake.grantRole(oldStake.VERIFIER_ROLE(), address(this));
        oldCredits = new ComputeCredits(address(oldStake), address(this), address(this), address(this));
        oldStake.registerProject(P, provider, 2000 ether);
        require(prl.transfer(provider, 2000 ether));
        vm.prank(provider);
        prl.approve(address(oldStake), 2000 ether);
        vm.prank(provider);
        oldStake.stake(P);
        oldStake.verifyProject(P);
        oldCredits.createOffering(P, 1000000, 1 gwei, MODEL);
        uint256 k1 = 0xA11CE;
        uint256 k2 = 0xB0B;
        address h1 = vm.addr(k1);
        address h2 = vm.addr(k2);
        if (h1 < h2) {
            holders.push(h1);
            holders.push(h2);
            keys.push(k1);
            keys.push(k2);
        } else {
            holders.push(h2);
            holders.push(h1);
            keys.push(k2);
            keys.push(k1);
        }
        for (uint256 i; i < 2; i++) {
            vm.deal(holders[i], 1 ether);
            vm.prank(holders[i]);
            oldCredits.buyAndLock{value: 2500 gwei}(P, 2500);
        }
        _burn(holders[0], 47, keccak256("real1"));
        _burn(holders[1], 2, keccak256("real2"));
        address[] memory people = new address[](1);
        people[0] = address(this);
        governance = new LaunchpadTimelock(people, people);
        bytes memory init = abi.encodePacked(
            vm.getCode("TestnetProtocolFactory.sol:TestnetProtocolFactory"),
            abi.encode(
                address(prl),
                address(governance),
                address(this),
                address(this),
                address(this),
                address(this),
                address(new ModuleDeployer())
            )
        );
        address deployed;
        assembly { deployed := create(0, add(init, 32), mload(init)) }
        require(deployed != address(0), "Factory failed");
        target = TestnetProtocolFactory(deployed);
        newStake = target.stakeVault();
        newUsage = target.usageEscrow();
        newToken = target.creditToken();
        newPayment = target.paymentEscrow();
        newSettler = target.settlementModule();
        gate = new LegacyMigrationGate(target, ILegacyStake(address(oldStake)), ILegacyCredits(address(oldCredits)), P);
        bytes memory data = abi.encodeCall(target.configureMigration, (address(gate)));
        governance.schedule(address(target), 0, data, 0, 0, 48 hours);
        vm.warp(block.timestamp + 48 hours);
        governance.execute(address(target), 0, data, 0, 0);
        oldStake.grantRole(oldStake.SETTLER_ROLE(), address(gate));
        oldCredits.grantRole(oldCredits.SETTLER_ROLE(), address(gate));
        oldCredits.grantRole(oldCredits.OFFER_MANAGER_ROLE(), address(gate));
        vm.prank(provider);
        prl.approve(address(newStake), 2000 ether);
        vm.prank(provider);
        gate.fund{value: 4951 gwei}();
        vm.roll(100);
        vm.setBlockhash(99, CHECKPOINT);
        expiry = uint64(block.timestamp + 1800);
    }

    function _burn(address buyer, uint256 amount, bytes32 id) private {
        bytes32[] memory ps = new bytes32[](1);
        ps[0] = P;
        address[] memory bs = new address[](1);
        bs[0] = buyer;
        uint256[] memory ns = new uint256[](1);
        ns[0] = amount;
        bytes32[] memory us = new bytes32[](1);
        us[0] = id;
        oldCredits.settleBatch(id, ps, bs, ns, us);
    }

    function _signatures() private returns (bytes[] memory sigs) {
        (bytes32 plan,,,) = gate.preview(holders, 99, CHECKPOINT);
        bytes32 digest = gate.consentDigest(plan, expiry);
        sigs = new bytes[](2);
        for (uint256 i; i < 2; i++) {
            (uint8 v, bytes32 r, bytes32 s) = vm.sign(keys[i], digest);
            sigs[i] = abi.encodePacked(r, s, v);
        }
    }

    function _commit() private {
        bytes[] memory sigs = _signatures();
        gate.commit(holders, 99, CHECKPOINT, expiry, sigs);
    }

    function testAtomicMigrationPreservesRightsTimesBackingAndRealDelivery() public {
        _commit();
        require(gate.committed() && address(gate).balance == 0);
        (uint8 status,,,,) = oldStake.stakeState(P);
        require(status == 5 && prl.balanceOf(address(oldStake)) == 0);
        require(!oldCredits.offerState(P).active && oldCredits.balanceOf(address(oldCredits), uint256(P)) == 0);
        require(prl.balanceOf(address(newStake)) == 2000 ether && newToken.issued(P) == 5000);
        require(newUsage.providerDelivered(P) == 49);
        (, uint256 locked, uint256 free, uint256 returned) = newStake.collateralState(P);
        require(locked == 1999902e15 && free == 98e15 && returned == 0);
        ProjectRegistry.Project memory p = target.registry().project(P);
        require(
            p.stakedAt == 1000 && p.deadline == 87400 && p.status == ProjectRegistry.Status.Verified
                && p.modelHash == MODEL && p.committedCredits == 1000000
        );
        for (uint256 i; i < 2; i++) {
            uint256 burned = i == 0 ? 47 : 2;
            (uint256 l, uint256 r, uint256 c, uint256 d, uint256 w) = newUsage.accounts(P, holders[i]);
            require(l == 2500 - burned && r == 0 && c == burned && d == 2500 && w == 0);
        }
        (uint256 paid, uint256 reserve, uint256 earned, uint256 claimed) = newPayment.funds(P);
        require(paid == 4951 gwei && reserve == paid && earned == 0 && claimed == 0);
        require(oldCredits.revenue(provider) == 5000 gwei); // old retirement is reimbursement, NOT new delivery.
        vm.prank(provider);
        newStake.withdraw(P, 98e15);
        require(prl.balanceOf(provider) == 98e15);
    }

    function testLateImportFailureRollsBackOldBurnReleaseAndSalesClosure() public {
        vm.prank(provider);
        prl.approve(address(newStake), 0);
        bytes[] memory sigs = _signatures();
        vm.expectRevert();
        gate.commit(holders, 99, CHECKPOINT, expiry, sigs);
        require(!gate.committed() && gate.prefunded() == 4951 gwei && oldCredits.offerState(P).active);
        require(
            oldCredits.balanceOf(address(oldCredits), uint256(P)) == 4951 && oldCredits.revenue(provider) == 49 gwei
        );
        (uint8 status,,,,) = oldStake.stakeState(P);
        require(status == 3 && prl.balanceOf(address(oldStake)) == 2000 ether);
        require(target.registry().project(P).status == ProjectRegistry.Status.None);
    }

    function testMissingHolderOrInvalidSignatureCannotMigrate() public {
        address[] memory missing = new address[](1);
        missing[0] = holders[0];
        vm.expectRevert();
        gate.preview(missing, 99, CHECKPOINT);
        bytes[] memory sigs = _signatures();
        sigs[1] = sigs[0];
        vm.expectRevert();
        gate.commit(holders, 99, CHECKPOINT, expiry, sigs);
        require(!gate.committed());
    }

    function testChangedBalanceInvalidatesEveryConsent() public {
        bytes[] memory sigs = _signatures();
        vm.prank(holders[0]);
        oldCredits.buyAndLock{value: 1 gwei}(P, 1);
        vm.expectRevert();
        gate.commit(holders, 99, CHECKPOINT, expiry, sigs);
    }

    function testWalletTokensAndPendingWithdrawalsBlockMigration() public {
        vm.prank(holders[0]);
        oldCredits.requestWithdrawal(P, 10);
        vm.expectRevert();
        gate.preview(holders, 99, CHECKPOINT);
        oldCredits.approveWithdrawal(P, holders[0], 1, 10);
        vm.prank(holders[0]);
        oldCredits.completeWithdrawal(P);
        vm.expectRevert();
        gate.preview(holders, 99, CHECKPOINT);
    }

    function testExpiredConsentAndChangedCheckpointBlockMigration() public {
        bytes[] memory sigs = _signatures();
        vm.setBlockhash(99, keccak256("reorg"));
        vm.expectRevert();
        gate.commit(holders, 99, CHECKPOINT, expiry, sigs);
        vm.setBlockhash(99, CHECKPOINT);
        vm.warp(expiry);
        vm.expectRevert();
        gate.commit(holders, 99, CHECKPOINT, expiry, sigs);
    }

    function testPrefundIsProviderOnlyAndRecoverableBeforeCommit() public {
        vm.expectRevert();
        gate.withdrawFunding(1);
        vm.expectRevert();
        gate.fund{value: 1}();
        vm.prank(provider);
        gate.withdrawFunding(1);
        bytes[] memory sigs = _signatures();
        vm.expectRevert();
        gate.commit(holders, 99, CHECKPOINT, expiry, sigs);
        vm.prank(provider);
        gate.fund{value: 1}();
        _commit();
        vm.expectRevert();
        vm.prank(provider);
        gate.withdrawFunding(1);
    }

    function testImportsAndGovernanceBindingCannotBeReplayedOrBypassed() public {
        vm.expectRevert();
        newToken.importIssuance(P, 5000, 49);
        vm.expectRevert();
        newUsage.importDeliveryBaseline(P, 1000000);
        vm.expectRevert();
        target.configureMigration(address(gate));
        bytes[] memory sigs = _signatures();
        gate.commit(holders, 99, CHECKPOINT, expiry, sigs);
        vm.expectRevert();
        gate.commit(holders, 99, CHECKPOINT, expiry, sigs);
    }

    function testAfterMigrationRealUsageUnlocksOnlyIncrementalStake() public {
        _commit();
        bytes32 reservation = keccak256("new live use");
        vm.prank(holders[0]);
        newUsage.reserve(P, reservation, 100, uint64(block.timestamp + 600));
        newSettler.settleProvider(reservation, keccak256("new delivered"), 1);
        require(newUsage.providerDelivered(P) == 50);
        (,, uint256 free,) = newStake.collateralState(P);
        require(free == 100e15);
        (,, uint256 earned,) = newPayment.funds(P);
        require(earned == 1 gwei);
    }
}
