// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {EIP712} from "@openzeppelin/contracts/utils/cryptography/EIP712.sol";
import {SignatureChecker} from "@openzeppelin/contracts/utils/cryptography/SignatureChecker.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {TestnetProtocolFactory} from "./TestnetProtocolFactory.sol";
import {ProjectRegistry} from "./ProjectRegistry.sol";

interface ILegacyStake {
    function stakeToken() external view returns (address);
    function stakeState(bytes32) external view returns (uint8, uint256, uint64, uint64, address);
    function releaseStake(bytes32, bytes32) external;
}

interface ILegacyCredits {
    struct Offering {
        address provider;
        uint256 supply;
        uint256 issued;
        uint256 weiPerCredit;
        bool active;
        bytes32 modelHash;
    }

    struct Account {
        uint256 locked;
        uint256 deposited;
        uint256 burned;
        uint256 withdrawn;
        uint256 withdrawAmount;
        uint256 nonce;
        bool approved;
        bool pending;
    }
    function stakeContract() external view returns (address);
    function offerState(bytes32) external view returns (Offering memory);
    function accountState(bytes32, address) external view returns (Account memory);
    function balanceOf(address, uint256) external view returns (uint256);
    function closeSales(bytes32) external;
    function settleBatch(bytes32, bytes32[] calldata, address[] calldata, uint256[] calldata, bytes32[] calldata)
        external;
}

/// @notice Fixed project/source/target, single-use handover. Old ETH requires provider-prefunded replacement backing.
contract LegacyMigrationGate is EIP712, ReentrancyGuard {
    bytes32 public constant CONSENT_TYPEHASH = keccak256("MigrationConsent(bytes32 planHash,uint64 expiresAt)");
    TestnetProtocolFactory public immutable protocol;
    ILegacyStake public immutable legacyStake;
    ILegacyCredits public immutable legacyCredits;
    bytes32 public immutable projectId;
    address public immutable provider;
    bool public committed;
    uint256 public prefunded;
    bytes32 public committedPlan;

    struct Snapshot {
        uint256 stakeAmount;
        uint64 stakedAt;
        uint64 deadline;
        ILegacyCredits.Offering offering;
        uint256 remaining;
        uint256 delivered;
        bytes32 accountDigest;
    }
    event MigrationFunded(address indexed provider, uint256 amount);
    event FundingReturned(address indexed provider, uint256 amount);
    event LegacyRetirement(bytes32 indexed planHash, bytes32 indexed project, address indexed holder, uint256 amount);
    event MigrationCommitted(
        bytes32 indexed planHash,
        bytes32 indexed project,
        uint256 historicalDelivered,
        uint256 importedCredits,
        uint256 prl,
        uint256 reserve
    );

    constructor(TestnetProtocolFactory target, ILegacyStake stake, ILegacyCredits credits, bytes32 id)
        EIP712("LaunchpadLegacyMigration", "1")
    {
        require(block.chainid == 11155111 || block.chainid == 31337, "Test networks only");
        require(
            address(target).code.length > 0 && address(stake).code.length > 0 && address(credits).code.length > 0
                && id != 0,
            "Invalid migration source"
        );
        require(
            credits.stakeContract() == address(stake)
                && stake.stakeToken() == address(target.stakeVault().stakeToken()),
            "Source/token mismatch"
        );
        (uint8 status,,,, address who) = stake.stakeState(id);
        require(status == 3 && who != address(0), "Source not verified");
        require(credits.offerState(id).provider == who, "Provider mismatch");
        protocol = target;
        legacyStake = stake;
        legacyCredits = credits;
        projectId = id;
        provider = who;
    }

    function fund() external payable nonReentrant {
        require(msg.sender == provider && !committed && msg.value > 0, "Only provider prefunding");
        prefunded += msg.value;
        emit MigrationFunded(msg.sender, msg.value);
    }

    function withdrawFunding(uint256 amount) external nonReentrant {
        require(msg.sender == provider && !committed && amount > 0 && amount <= prefunded, "Invalid funding return");
        prefunded -= amount;
        (bool ok,) = payable(provider).call{value: amount}("");
        require(ok, "Transfer failed");
        emit FundingReturned(provider, amount);
    }

    function _snapshot(address[] calldata holders) private view returns (Snapshot memory s) {
        require(!committed && holders.length > 0 && holders.length <= 32, "Invalid migration state/holders");
        (uint8 status, uint256 amount, uint64 at, uint64 deadline, address who) = legacyStake.stakeState(projectId);
        require(status == 3 && who == provider, "Source stake changed");
        s.stakeAmount = amount;
        s.stakedAt = at;
        s.deadline = deadline;
        s.offering = legacyCredits.offerState(projectId);
        require(
            s.offering.provider == provider && s.offering.supply > 0 && s.offering.issued <= s.offering.supply,
            "Invalid source offering"
        );
        address previous;
        for (uint256 i; i < holders.length; i++) {
            require(holders[i] > previous, "Holders must be sorted and unique");
            previous = holders[i];
            ILegacyCredits.Account memory a = legacyCredits.accountState(projectId, holders[i]);
            require(!a.pending && !a.approved && a.withdrawAmount == 0, "Resolve pending withdrawals");
            require(a.deposited == a.locked + a.burned + a.withdrawn, "Legacy account mismatch");
            s.remaining += a.locked;
            s.delivered += a.burned;
            s.accountDigest = keccak256(abi.encode(s.accountDigest, holders[i], a));
        }
        require(
            s.remaining == legacyCredits.balanceOf(address(legacyCredits), uint256(projectId)), "Missing custody holder"
        );
        require(s.remaining + s.delivered == s.offering.issued, "Wallet tokens or missing history");
    }

    function _plan(Snapshot memory s, uint256 checkpoint, bytes32 checkpointHash) private view returns (bytes32) {
        require(
            checkpoint < block.number && block.number - checkpoint <= 256 && checkpointHash != 0
                && blockhash(checkpoint) == checkpointHash,
            "Checkpoint changed/expired"
        );
        return keccak256(
            abi.encode(
                block.chainid,
                address(this),
                address(protocol),
                address(legacyStake),
                address(legacyCredits),
                projectId,
                provider,
                s,
                checkpoint,
                checkpointHash
            )
        );
    }

    function preview(address[] calldata holders, uint256 checkpoint, bytes32 checkpointHash)
        external
        view
        returns (bytes32 planHash, uint256 remaining, uint256 historicalDelivered, uint256 requiredReserve)
    {
        Snapshot memory s = _snapshot(holders);
        return (_plan(s, checkpoint, checkpointHash), s.remaining, s.delivered, s.remaining * s.offering.weiPerCredit);
    }

    function consentDigest(bytes32 planHash, uint64 expiresAt) public view returns (bytes32) {
        return _hashTypedDataV4(keccak256(abi.encode(CONSENT_TYPEHASH, planHash, expiresAt)));
    }

    function commit(
        address[] calldata holders,
        uint256 checkpoint,
        bytes32 checkpointHash,
        uint64 expiresAt,
        bytes[] calldata signatures
    ) external nonReentrant {
        require(
            protocol.migrationGate() == address(this) && signatures.length == holders.length
                && expiresAt > block.timestamp && expiresAt <= block.timestamp + 1 hours,
            "Gate/consent unavailable"
        );
        Snapshot memory s = _snapshot(holders);
        bytes32 planHash = _plan(s, checkpoint, checkpointHash);
        bytes32 digest = consentDigest(planHash, expiresAt);
        for (uint256 i; i < holders.length; i++) {
            require(SignatureChecker.isValidSignatureNow(holders[i], digest, signatures[i]), "Invalid holder consent");
        }
        require(protocol.registry().project(projectId).status == ProjectRegistry.Status.None, "Target already exists");
        uint256 reserve = s.remaining * s.offering.weiPerCredit;
        require(prefunded == reserve, "Exact new reserve required");
        committed = true;
        committedPlan = planHash;
        prefunded = 0;
        ILegacyCredits.Account[] memory histories = new ILegacyCredits.Account[](holders.length);
        for (uint256 i; i < holders.length; i++) {
            histories[i] = legacyCredits.accountState(projectId, holders[i]);
        }
        _retire(holders, histories, planHash);
        legacyStake.releaseStake(projectId, planHash);
        _import(s, holders, histories, planHash);
    }

    function _import(
        Snapshot memory s,
        address[] calldata holders,
        ILegacyCredits.Account[] memory histories,
        bytes32 planHash
    ) private {
        uint256 reserve = s.remaining * s.offering.weiPerCredit;
        protocol.registry()
            .importProject(
                projectId,
                ProjectRegistry.Project(
                    provider,
                    s.stakeAmount,
                    s.offering.supply,
                    s.offering.weiPerCredit,
                    s.offering.modelHash,
                    s.stakedAt,
                    s.deadline,
                    ProjectRegistry.Status.Verified,
                    !s.offering.active,
                    false
                )
            );
        protocol.stakeVault().importStake(projectId);
        protocol.creditToken().importIssuance(projectId, s.offering.issued, s.delivered);
        protocol.usageEscrow().importDeliveryBaseline(projectId, s.delivered);
        protocol.paymentEscrow().importReserve{value: reserve}(projectId, s.remaining);
        for (uint256 i; i < holders.length; i++) {
            ILegacyCredits.Account memory a = histories[i];
            if (a.locked > 0) protocol.creditToken().mintMigrated(projectId, holders[i], a.locked);
            protocol.usageEscrow()
                .importAccountHistory(projectId, holders[i], a.locked, a.burned, a.deposited, a.withdrawn);
        }
        require(protocol.creditToken().migrationMintRemaining(projectId) == 0, "Incomplete credit import");
        emit MigrationCommitted(planHash, projectId, s.delivered, s.remaining, s.stakeAmount, reserve);
    }

    function _retire(address[] calldata holders, ILegacyCredits.Account[] memory histories, bytes32 planHash) private {
        legacyCredits.closeSales(projectId);
        uint256 count;
        for (uint256 i; i < holders.length; i++) {
            if (histories[i].locked > 0) count++;
        }
        if (count == 0) return;
        bytes32[] memory projects = new bytes32[](count);
        address[] memory buyers = new address[](count);
        uint256[] memory amounts = new uint256[](count);
        bytes32[] memory ids = new bytes32[](count);
        uint256 j;
        for (uint256 i; i < holders.length; i++) {
            if (histories[i].locked > 0) {
                projects[j] = projectId;
                buyers[j] = holders[i];
                amounts[j] = histories[i].locked;
                ids[j] = keccak256(abi.encode("MIGRATION_RETIREMENT", address(this), planHash, holders[i]));
                emit LegacyRetirement(planHash, projectId, holders[i], amounts[j]);
                j++;
            }
        }
        legacyCredits.settleBatch(planHash, projects, buyers, amounts, ids);
    }
}
