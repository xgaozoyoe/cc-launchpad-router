// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {LaunchpadTimelock} from "../src/modular/LaunchpadTimelock.sol";
import {TestPRL} from "../src/TestPRL.sol";
import {ProjectRegistry} from "../src/modular/ProjectRegistry.sol";
import {CreditToken} from "../src/modular/CreditToken.sol";
import {PaymentEscrow} from "../src/modular/PaymentEscrow.sol";
import {UsageEscrow} from "../src/modular/UsageEscrow.sol";
import {StakeVault} from "../src/modular/StakeVault.sol";
import {SaleRouter, SettlementModule} from "../src/modular/Modules.sol";
import {TestnetProtocolFactory} from "../src/modular/TestnetProtocolFactory.sol";

interface ModularVm {
    function prank(address) external;
    function deal(address, uint256) external;
    function warp(uint256) external;
    function expectRevert() external;
    function addr(uint256) external returns (address);
    function sign(uint256, bytes32) external returns (uint8, bytes32, bytes32);
    function chainId(uint256) external;
    function getCode(string calldata) external returns (bytes memory);
}

contract ReentrantRevenueProvider {
    PaymentEscrow public immutable payment;
    bool public reentrySucceeded;
    bool public attempted;
    bytes32 private project;

    constructor(PaymentEscrow p) {
        payment = p;
    }

    function deposit(TestPRL token, StakeVault vault, bytes32 id, uint256 amount) external {
        token.approve(address(vault), amount);
        vault.stake(id);
    }

    function claim(bytes32 id, uint256 amount) external {
        project = id;
        payment.claimRevenue(id, amount);
    }

    receive() external payable {
        if (!attempted) attempted = true;
        try payment.claimRevenue(project, 1) {
            reentrySucceeded = true;
        }
            catch {}
    }
}

contract ModularProtocolTest {
    ModularVm constant vm = ModularVm(address(uint160(uint256(keccak256("hevm cheat code")))));
    TestPRL prl;
    LaunchpadTimelock governance;
    TestnetProtocolFactory protocol;
    ProjectRegistry registry;
    CreditToken token;
    UsageEscrow usage;
    PaymentEscrow payment;
    StakeVault stake;
    SaleRouter sale;
    SettlementModule settler;
    address provider = address(0x1234);
    address buyer;
    address stranger = address(0xBAD);
    uint256 constant BUYER_KEY = 0xA11CE;
    bytes32 constant PROJECT = keccak256("real project identity");
    bytes32 constant MODEL = keccak256("qwen3-8b-awq");
    bytes32 constant RES = keccak256("reservation");
    bytes32 constant RECEIPT = keccak256("usage");

    function setUp() public {
        vm.warp(1000);
        vm.deal(address(this), 1 ether);
        buyer = vm.addr(BUYER_KEY);
        prl = new TestPRL(address(this), 10000 ether);
        address[] memory proposers = new address[](1);
        proposers[0] = address(this);
        governance = new LaunchpadTimelock(proposers, proposers);
        bytes memory init = abi.encodePacked(
            vm.getCode("TestnetProtocolFactory.sol:TestnetProtocolFactory"),
            abi.encode(IERC20(address(prl)), governance, address(this), address(this), address(this), address(this))
        );
        address created;
        assembly { created := create(0, add(init, 32), mload(init)) }
        require(created != address(0), "Factory deployment failed");
        protocol = TestnetProtocolFactory(created);
        registry = protocol.registry();
        token = protocol.creditToken();
        usage = protocol.usageEscrow();
        payment = protocol.paymentEscrow();
        stake = protocol.stakeVault();
        sale = protocol.saleRouter();
        settler = protocol.settlementModule();
        registry.register(PROJECT, provider, 2000 ether, 1000000, 1 gwei, MODEL);
        require(prl.transfer(provider, 2000 ether));
        vm.prank(provider);
        prl.approve(address(stake), 2000 ether);
        vm.prank(provider);
        stake.stake(PROJECT);
        registry.verifyProject(PROJECT);
        vm.deal(buyer, 1 ether);
        vm.prank(buyer);
        sale.buyAndLock{value: 1000 gwei}(PROJECT, 1000);
    }

    function reserve(uint256 maximum) private {
        vm.prank(buyer);
        usage.reserve(PROJECT, RES, maximum, uint64(block.timestamp + 1 hours));
    }

    function deliver(uint256 amount) private {
        reserve(amount);
        settler.settleProvider(RES, RECEIPT, amount);
    }

    function available() private view returns (uint256 locked, uint256 reserved, uint256 consumed) {
        (locked, reserved, consumed,,) = usage.accounts(PROJECT, buyer);
    }

    function testAssetsAndLedgersAreSeparated() public view {
        require(prl.balanceOf(address(stake)) == 2000 ether && prl.balanceOf(address(sale)) == 0);
        require(address(payment).balance == 1000 gwei && address(sale).balance == 0);
        require(token.balanceOf(address(usage), uint256(PROJECT)) == 1000);
        (uint256 paid, uint256 reserve_, uint256 earned, uint256 claimed) = payment.funds(PROJECT);
        require(paid == 1000 gwei && reserve_ == paid && earned == 0 && claimed == 0);
        (uint256 i, uint256 l, uint256 f, uint256 r) = stake.collateralState(PROJECT);
        require(i == 2000 ether && l == i && f == 0 && r == 0);
    }

    function testPurchaseRollsBackPaymentIfIssuanceFails() public {
        vm.expectRevert();
        vm.prank(buyer);
        sale.buyAndLock{value: 1000001 gwei}(PROJECT, 1000001);
        require(address(payment).balance == 1000 gwei && token.issued(PROJECT) == 1000);
        vm.expectRevert();
        vm.prank(buyer);
        sale.buyAndLock{value: 1}(PROJECT, 1);
    }

    function testNoFreeMintDirectSettlementOrAdminWithdrawal() public {
        vm.expectRevert();
        token.issuePaid(PROJECT, buyer, 1);
        vm.expectRevert();
        token.burnEscrow(PROJECT, 1);
        vm.expectRevert();
        payment.purchase{value: 1 gwei}(PROJECT, buyer, 1);
        vm.expectRevert();
        payment.settleProvider(PROJECT, RECEIPT, 1);
        vm.expectRevert();
        usage.settleProvider(RES, RECEIPT, 1);
        vm.expectRevert();
        stake.withdraw(PROJECT, 1);
        vm.expectRevert();
        payment.claimRevenue(PROJECT, 1);
    }

    function testProviderBurnUnlocksStakeAndEarnsOnlyActualTokens() public {
        reserve(100);
        settler.settleProvider(RES, RECEIPT, 49);
        (uint256 l, uint256 r, uint256 c) = available();
        require(l == 951 && r == 0 && c == 49);
        require(token.balanceOf(address(usage), uint256(PROJECT)) == 951 && token.issued(PROJECT) == 1000);
        require(usage.providerDelivered(PROJECT) == 49);
        (uint256 paid, uint256 remaining, uint256 earned,) = payment.funds(PROJECT);
        require(paid == 1000 gwei && remaining == 951 gwei && earned == 49 gwei);
        (, uint256 locked, uint256 free,) = stake.collateralState(PROJECT);
        require(locked == 1999902e15 && free == 98e15);
        vm.prank(provider);
        stake.withdraw(PROJECT, 40e15);
        uint256 returned_;
        (,, free, returned_) = stake.collateralState(PROJECT);
        require(free == 58e15 && returned_ == 40e15);
        vm.prank(provider);
        payment.claimRevenue(PROJECT, 49 gwei);
        require(provider.balance == 49 gwei && address(payment).balance == 951 gwei);
        require(registry.project(PROJECT).status == ProjectRegistry.Status.Verified);
    }

    function testKnownFailureAndExpiryConsumeNothingAndCannotLaterSettle() public {
        reserve(100);
        settler.releaseFailed(RES);
        (uint256 l, uint256 r, uint256 c) = available();
        require(l == 1000 && r == 0 && c == 0 && usage.providerDelivered(PROJECT) == 0);
        vm.expectRevert();
        settler.settleProvider(RES, RECEIPT, 1);
        bytes32 second = keccak256("expired");
        vm.prank(buyer);
        usage.reserve(PROJECT, second, 20, uint64(block.timestamp + 1));
        vm.warp(block.timestamp + 1);
        usage.releaseExpired(second);
        vm.expectRevert();
        settler.settleProvider(second, RECEIPT, 1);
    }

    function testReservedTokensCannotBeWithdrawnAndActualCannotExceedMaximum() public {
        reserve(100);
        vm.expectRevert();
        vm.prank(buyer);
        usage.withdraw(PROJECT, 901);
        vm.expectRevert();
        settler.settleProvider(RES, RECEIPT, 101);
        vm.prank(buyer);
        usage.withdraw(PROJECT, 900);
        require(token.balanceOf(buyer, uint256(PROJECT)) == 900);
        settler.settleProvider(RES, RECEIPT, 100);
        require(usage.providerDelivered(PROJECT) == 100);
    }

    function testReceiptAndReservationReplayCannotBurnTwice() public {
        deliver(1);
        vm.expectRevert();
        settler.settleProvider(RES, RECEIPT, 1);
        vm.expectRevert();
        vm.prank(buyer);
        usage.reserve(PROJECT, RES, 1, uint64(block.timestamp + 1 hours));
        bytes32 second = keccak256("another reservation");
        vm.prank(buyer);
        usage.reserve(PROJECT, second, 1, uint64(block.timestamp + 1 hours));
        vm.expectRevert();
        settler.settleProvider(second, RECEIPT, 1);
        require(usage.providerDelivered(PROJECT) == 1);
    }

    function testBuyerSignatureIsBoundToMaximumNonceChainAndEscrow() public {
        uint64 expiry = uint64(block.timestamp + 1 hours);
        bytes32 digest = usage.reservationDigest(PROJECT, buyer, RES, 100, 0, expiry);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(BUYER_KEY, digest);
        bytes memory signature = abi.encodePacked(r, s, v);
        vm.expectRevert();
        usage.reserveWithSignature(PROJECT, buyer, RES, 101, 0, expiry, signature);
        vm.chainId(1);
        vm.expectRevert();
        usage.reserveWithSignature(PROJECT, buyer, RES, 100, 0, expiry, signature);
        vm.chainId(31337);
        UsageEscrow other = new UsageEscrow(registry, token, address(this));
        vm.expectRevert();
        other.reserveWithSignature(PROJECT, buyer, RES, 100, 0, expiry, signature);
        usage.reserveWithSignature(PROJECT, buyer, RES, 100, 0, expiry, signature);
        require(usage.nonces(buyer) == 1);
        vm.expectRevert();
        usage.reserveWithSignature(PROJECT, buyer, RES, 100, 0, expiry, signature);
    }

    function testWalletTokensCanMoveAndRedepositWithoutChangingLiability() public {
        vm.prank(buyer);
        usage.withdraw(PROJECT, 100);
        vm.prank(buyer);
        token.safeTransferFrom(buyer, stranger, uint256(PROJECT), 100, "");
        vm.prank(stranger);
        token.safeTransferFrom(stranger, address(usage), uint256(PROJECT), 100, "");
        (uint256 l,,,,) = usage.accounts(PROJECT, stranger);
        require(l == 100);
        vm.prank(stranger);
        usage.reserve(PROJECT, RES, 100, uint64(block.timestamp + 1 hours));
        settler.settleProvider(RES, RECEIPT, 100);
        (uint256 paid, uint256 remaining, uint256 earned,) = payment.funds(PROJECT);
        require(paid == remaining + earned && earned == 100 gwei);
    }

    function testVoluntaryWalletBurnDoesNotReleaseCollateralOrRevenue() public {
        vm.prank(buyer);
        usage.withdraw(PROJECT, 100);
        vm.prank(buyer);
        token.burnOwn(PROJECT, 100);
        require(usage.providerDelivered(PROJECT) == 0 && token.issued(PROJECT) == 1000);
        (,, uint256 free,) = stake.collateralState(PROJECT);
        require(free == 0);
        (,, uint256 earned,) = payment.funds(PROJECT);
        require(earned == 0);
    }

    function testBindingsAreSealedAndFactoryRetainsNoAdmin() public {
        vm.expectRevert();
        registry.bindStakeVault(address(stake));
        vm.expectRevert();
        token.bindEscrows(address(payment), address(usage));
        vm.expectRevert();
        usage.bindPaymentEscrow(payment);
        require(
            !registry.hasRole(0, address(protocol)) && !payment.hasRole(0, address(protocol))
                && !usage.hasRole(0, address(protocol))
        );
        require(registry.hasRole(0, address(governance)) && payment.hasRole(0, address(governance)));
        bytes32 acceptanceRole = registry.ACCEPTANCE_ROLE();
        vm.expectRevert();
        registry.grantRole(acceptanceRole, stranger);
    }

    function testFixedTermsAndVerificationDeadlineCannotBeOverridden() public {
        vm.expectRevert();
        registry.register(PROJECT, provider, 1, 1, 1, MODEL);
        bytes32 next = keccak256("next");
        registry.register(next, provider, 1 ether, 100, 1 gwei, MODEL);
        require(prl.transfer(provider, 1 ether));
        vm.prank(provider);
        prl.approve(address(stake), 1 ether);
        vm.prank(provider);
        stake.stake(next);
        vm.warp(block.timestamp + 24 hours);
        vm.expectRevert();
        registry.verifyProject(next);
        vm.prank(provider);
        stake.refundExpired(next);
        (uint256 i, uint256 l, uint256 f, uint256 r) = stake.collateralState(next);
        require(i == 1 ether && l == 0 && f == 0 && r == i);
        vm.expectRevert();
        vm.prank(provider);
        stake.refundExpired(next);
    }

    function testSalesPauseDoesNotStopExistingDeliveryOrAvailableWithdrawals() public {
        registry.emergencyPause(PROJECT, true, false);
        vm.expectRevert();
        vm.prank(buyer);
        sale.buyAndLock{value: 1 gwei}(PROJECT, 1);
        deliver(10);
        vm.prank(provider);
        stake.withdraw(PROJECT, 20e15);
        vm.prank(buyer);
        usage.withdraw(PROJECT, 1);
        vm.expectRevert();
        registry.setPause(PROJECT, false, false);
    }

    function testServicePauseStopsNewReservationsButExpiredFundsCanExit() public {
        reserve(100);
        registry.emergencyPause(PROJECT, false, true);
        vm.expectRevert();
        settler.settleProvider(RES, RECEIPT, 1);
        vm.warp(block.timestamp + 1 hours);
        usage.releaseExpired(RES);
        vm.prank(buyer);
        usage.withdraw(PROJECT, 1000);
    }

    function testProjectReservesCannotBeSpentAcrossProjects() public {
        bytes32 other = keccak256("other");
        registry.register(other, provider, 1 ether, 1000, 2 gwei, MODEL);
        require(prl.transfer(provider, 1 ether));
        vm.prank(provider);
        prl.approve(address(stake), 1 ether);
        vm.prank(provider);
        stake.stake(other);
        registry.verifyProject(other);
        vm.expectRevert();
        vm.prank(address(usage));
        payment.settleProvider(other, RECEIPT, 1);
        require(address(payment).balance == 1000 gwei);
        vm.expectRevert();
        vm.prank(buyer);
        usage.reserve(other, RES, 1, uint64(block.timestamp + 1 hours));
    }

    function testTimelockRotationPreservesFundsLedgerAndReplayProtection() public {
        deliver(49);
        SaleRouter replacementSale = new SaleRouter(payment);
        SettlementModule replacementSettler = new SettlementModule(usage, address(governance), address(this));
        bytes32 saleRole = payment.SALE_ROLE();
        bytes32 settleRole = usage.SETTLEMENT_ROLE();
        address[] memory targets = new address[](4);
        uint256[] memory values = new uint256[](4);
        bytes[] memory data = new bytes[](4);
        targets[0] = address(payment);
        data[0] = abi.encodeCall(payment.revokeRole, (saleRole, address(sale)));
        targets[1] = address(payment);
        data[1] = abi.encodeCall(payment.grantRole, (saleRole, address(replacementSale)));
        targets[2] = address(usage);
        data[2] = abi.encodeCall(usage.revokeRole, (settleRole, address(settler)));
        targets[3] = address(usage);
        data[3] = abi.encodeCall(usage.grantRole, (settleRole, address(replacementSettler)));
        bytes32 salt = keccak256("rotation");
        governance.scheduleBatch(targets, values, data, 0, salt, 48 hours);
        vm.expectRevert();
        governance.executeBatch(targets, values, data, 0, salt);
        vm.warp(block.timestamp + 48 hours);
        governance.executeBatch(targets, values, data, 0, salt);
        require(
            protocol.paymentEscrow() == payment && protocol.usageEscrow() == usage && protocol.stakeVault() == stake
        );
        require(address(payment).balance == 1000 gwei && prl.balanceOf(address(stake)) == 2000 ether);
        (uint256 l,, uint256 c) = available();
        require(l == 951 && c == 49 && usage.usageSettled(RECEIPT));
        vm.expectRevert();
        vm.prank(buyer);
        sale.buyAndLock{value: 1 gwei}(PROJECT, 1);
        vm.prank(buyer);
        replacementSale.buyAndLock{value: 1 gwei}(PROJECT, 1);
        bytes32 second = keccak256("post rotation");
        vm.prank(buyer);
        usage.reserve(PROJECT, second, 1, uint64(block.timestamp + 1 hours));
        vm.expectRevert();
        settler.settleProvider(second, keccak256("new receipt"), 1);
        vm.expectRevert();
        replacementSettler.settleProvider(second, RECEIPT, 1);
        replacementSettler.settleProvider(second, keccak256("new receipt"), 1);
        require(usage.providerDelivered(PROJECT) == 50);
    }

    function testGovernanceCannotLowerTheDelayFloor() public {
        bytes memory data = abi.encodeCall(governance.updateDelay, (1));
        bytes32 salt = keccak256("lower delay");
        governance.schedule(address(governance), 0, data, 0, salt, 48 hours);
        vm.warp(block.timestamp + 48 hours);
        vm.expectRevert();
        governance.execute(address(governance), 0, data, 0, salt);
        require(governance.getMinDelay() == 48 hours);
    }

    function testFullCommitmentCanBeDeliveredAndStakeFullyWithdrawn() public {
        vm.prank(buyer);
        sale.buyAndLock{value: 999000 gwei}(PROJECT, 999000);
        reserve(1000000);
        settler.settleProvider(RES, RECEIPT, 1000000);
        vm.prank(provider);
        stake.withdraw(PROJECT, 2000 ether);
        (uint256 i, uint256 l, uint256 f, uint256 r) = stake.collateralState(PROJECT);
        require(i == 2000 ether && l == 0 && f == 0 && r == i && prl.balanceOf(address(stake)) == 0);
        vm.expectRevert();
        vm.prank(buyer);
        sale.buyAndLock{value: 1 gwei}(PROJECT, 1);
        require(registry.project(PROJECT).status == ProjectRegistry.Status.Verified);
    }

    function testExpiredBuyerSignatureCannotReserve() public {
        uint64 expiry = uint64(block.timestamp + 1);
        (uint8 v, bytes32 r, bytes32 s) =
            vm.sign(BUYER_KEY, usage.reservationDigest(PROJECT, buyer, RES, 100, 0, expiry));
        vm.warp(expiry);
        vm.expectRevert();
        usage.reserveWithSignature(PROJECT, buyer, RES, 100, 0, expiry, abi.encodePacked(r, s, v));
        require(usage.nonces(buyer) == 0);
    }

    function testGovernanceCannotDelegateInstantAdminToAnEOA() public {
        bytes memory data = abi.encodeCall(registry.grantRole, (bytes32(0), stranger));
        bytes32 salt = keccak256("admin bypass");
        governance.schedule(address(registry), 0, data, 0, salt, 48 hours);
        vm.warp(block.timestamp + 48 hours);
        vm.expectRevert();
        governance.execute(address(registry), 0, data, 0, salt);
        require(!registry.hasRole(0, stranger) && registry.governor() == address(governance));
        vm.expectRevert();
        registry.sealGovernance(address(settler));
    }

    function testRevenueClaimRejectsReentrancyWithEarnedBalanceRemaining() public {
        ReentrantRevenueProvider receiver = new ReentrantRevenueProvider(payment);
        bytes32 other = keccak256("reentrant provider");
        registry.register(other, address(receiver), 1 ether, 1000, 1 gwei, MODEL);
        require(prl.transfer(address(receiver), 1 ether));
        receiver.deposit(prl, stake, other, 1 ether);
        registry.verifyProject(other);
        vm.prank(buyer);
        sale.buyAndLock{value: 100 gwei}(other, 100);
        vm.prank(buyer);
        usage.reserve(other, RES, 100, uint64(block.timestamp + 1 hours));
        settler.settleProvider(RES, RECEIPT, 49);
        receiver.claim(other, 25 gwei);
        require(receiver.attempted() && !receiver.reentrySucceeded() && address(receiver).balance == 25 gwei);
        (,, uint256 earned, uint256 claimed) = payment.funds(other);
        require(earned == 24 gwei && claimed == 25 gwei);
    }

    function testFuzzCollateralAndPaymentConservation(uint16 rawDelivered, uint16 rawWithdrawn) public {
        uint256 d = uint256(rawDelivered) % 1001;
        if (d > 0) deliver(d);
        uint256 unlocked = 2000 ether * d / 1000000;
        uint256 take = unlocked * (uint256(rawWithdrawn) % 1001) / 1000;
        if (take > 0) {
            vm.prank(provider);
            stake.withdraw(PROJECT, take);
        }
        (uint256 i, uint256 l, uint256 f, uint256 r) = stake.collateralState(PROJECT);
        require(l + f + r == i && r == take && f + r == unlocked && prl.balanceOf(address(stake)) == l + f);
        (uint256 paid, uint256 reserve_, uint256 earned, uint256 claimed) = payment.funds(PROJECT);
        require(paid == reserve_ + earned + claimed && address(payment).balance == reserve_ + earned);
        (uint256 locked,, uint256 consumed) = available();
        require(locked + consumed == 1000 && consumed == d);
    }
}
