// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {MigrationBound} from "./MigrationBound.sol";
import {AccessControl} from "@openzeppelin/contracts/access/AccessControl.sol";
import {GovernedAccess} from "./GovernedAccess.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {ERC1155Holder} from "@openzeppelin/contracts/token/ERC1155/utils/ERC1155Holder.sol";
import {EIP712} from "@openzeppelin/contracts/utils/cryptography/EIP712.sol";
import {SignatureChecker} from "@openzeppelin/contracts/utils/cryptography/SignatureChecker.sol";
import {ProjectRegistry} from "./ProjectRegistry.sol";
import {CreditToken} from "./CreditToken.sol";
import {PaymentEscrow} from "./PaymentEscrow.sol";

/// @notice Stable buyer authorizations, reservations, burns and delivery accounting.
contract UsageEscrow is MigrationBound, GovernedAccess, ReentrancyGuard, ERC1155Holder, EIP712 {
    bytes32 public constant SETTLEMENT_ROLE = keccak256("SETTLEMENT_ROLE");
    bytes32 public constant RESERVATION_TYPEHASH = keccak256(
        "Reservation(bytes32 project,address buyer,bytes32 reservationId,uint256 maximum,uint256 nonce,uint64 expiresAt)"
    );

    struct Account {
        uint256 locked;
        uint256 reserved;
        uint256 consumed;
        uint256 deposited;
        uint256 withdrawn;
    }

    struct Reservation {
        bytes32 project;
        address buyer;
        uint256 maximum;
        uint64 expiresAt;
        bool closed;
    }
    mapping(bytes32 => mapping(address => Account)) public accounts;
    mapping(bytes32 => Reservation) public reservations;
    mapping(address => uint256) public nonces;
    mapping(bytes32 => bool) public usageSettled;
    mapping(bytes32 => uint256) public providerDelivered;
    mapping(bytes32 => bool) public baselineImported;
    mapping(bytes32 => mapping(address => bool)) public accountImported;
    ProjectRegistry public immutable registry;
    CreditToken public immutable token;
    PaymentEscrow public paymentEscrow;
    address private immutable bootstrap;
    event CreditsDeposited(bytes32 indexed project, address indexed buyer, uint256 amount);
    event CreditsWithdrawn(bytes32 indexed project, address indexed buyer, uint256 amount);
    event Reserved(
        bytes32 indexed reservationId,
        bytes32 indexed project,
        address indexed buyer,
        uint256 maximum,
        uint256 nonce,
        uint64 expiresAt
    );
    event ReservationReleased(bytes32 indexed reservationId);
    event ProviderSettled(
        bytes32 indexed usageId,
        bytes32 indexed reservationId,
        bytes32 indexed project,
        address buyer,
        address provider,
        uint256 credits
    );

    constructor(ProjectRegistry r, CreditToken t, address admin)
        GovernedAccess(admin)
        EIP712("LaunchpadUsageEscrow", "1")
    {
        registry = r;
        token = t;
        bootstrap = msg.sender;
    }

    function bindPaymentEscrow(PaymentEscrow payment) external {
        require(
            msg.sender == bootstrap && address(paymentEscrow) == address(0) && address(payment).code.length > 0,
            "Binding sealed"
        );
        require(
            payment.usageEscrow() == address(this) && address(payment.token()) == address(token)
                && address(payment.registry()) == address(registry),
            "Wrong peers"
        );
        paymentEscrow = payment;
    }

    function importDeliveryBaseline(bytes32 project, uint256 delivered) external onlyMigrationGate {
        require(
            !baselineImported[project] && providerDelivered[project] == 0
                && delivered <= registry.project(project).committedCredits,
            "Invalid delivery baseline"
        );
        baselineImported[project] = true;
        providerDelivered[project] = delivered;
    }

    /// @dev Token mint callback has already credited exactly the unconsumed amount.
    function importAccountHistory(
        bytes32 project,
        address buyer,
        uint256 locked,
        uint256 consumed,
        uint256 deposited,
        uint256 withdrawn
    ) external onlyMigrationGate {
        Account storage a = accounts[project][buyer];
        require(
            baselineImported[project] && !accountImported[project][buyer] && a.locked == locked && a.reserved == 0
                && a.consumed == 0 && a.withdrawn == 0 && a.deposited == locked
                && deposited == locked + consumed + withdrawn,
            "Invalid account import"
        );
        accountImported[project][buyer] = true;
        a.consumed = consumed;
        a.deposited = deposited;
        a.withdrawn = withdrawn;
    }

    function reservationDigest(
        bytes32 project,
        address buyer,
        bytes32 id,
        uint256 maximum,
        uint256 nonce,
        uint64 expiry
    ) public view returns (bytes32) {
        return _hashTypedDataV4(keccak256(abi.encode(RESERVATION_TYPEHASH, project, buyer, id, maximum, nonce, expiry)));
    }

    function reserve(bytes32 project, bytes32 id, uint256 maximum, uint64 expiry) external nonReentrant {
        _reserve(project, msg.sender, id, maximum, nonces[msg.sender], expiry);
    }

    function reserveWithSignature(
        bytes32 project,
        address buyer,
        bytes32 id,
        uint256 maximum,
        uint256 nonce,
        uint64 expiry,
        bytes calldata signature
    ) external nonReentrant {
        require(
            SignatureChecker.isValidSignatureNow(
                buyer, reservationDigest(project, buyer, id, maximum, nonce, expiry), signature
            ),
            "Invalid buyer signature"
        );
        _reserve(project, buyer, id, maximum, nonce, expiry);
    }

    function _reserve(bytes32 project, address buyer, bytes32 id, uint256 maximum, uint256 nonce, uint64 expiry)
        private
    {
        registry.requireService(project);
        require(buyer != address(0) && id != 0 && reservations[id].buyer == address(0), "Invalid reservation ID");
        require(
            maximum > 0 && expiry > block.timestamp && expiry <= block.timestamp + 1 days && nonce == nonces[buyer],
            "Invalid authorization"
        );
        Account storage a = accounts[project][buyer];
        require(maximum <= a.locked - a.reserved, "Insufficient available");
        nonces[buyer] = nonce + 1;
        a.reserved += maximum;
        reservations[id] = Reservation(project, buyer, maximum, expiry, false);
        emit Reserved(id, project, buyer, maximum, nonce, expiry);
    }

    function settleProvider(bytes32 id, bytes32 usageId, uint256 actual)
        external
        onlyRole(SETTLEMENT_ROLE)
        nonReentrant
    {
        Reservation storage r = reservations[id];
        require(r.buyer != address(0) && !r.closed && block.timestamp < r.expiresAt, "Reservation unavailable");
        registry.requireService(r.project);
        require(usageId != 0 && !usageSettled[usageId] && actual > 0 && actual <= r.maximum, "Invalid receipt");
        usageSettled[usageId] = true;
        r.closed = true;
        Account storage a = accounts[r.project][r.buyer];
        a.reserved -= r.maximum;
        a.locked -= actual;
        a.consumed += actual;
        providerDelivered[r.project] += actual;
        require(
            providerDelivered[r.project] <= registry.project(r.project).committedCredits, "Delivery exceeds commitment"
        );
        token.burnEscrow(r.project, actual);
        paymentEscrow.settleProvider(r.project, usageId, actual);
        emit ProviderSettled(usageId, id, r.project, r.buyer, registry.project(r.project).provider, actual);
    }

    /// @notice Known failure: no burn, no revenue, no collateral release.
    function releaseFailed(bytes32 id) external onlyRole(SETTLEMENT_ROLE) nonReentrant {
        _release(id);
    }

    /// @notice Anyone can release an expired maximum; late settlement is then rejected.
    function releaseExpired(bytes32 id) external nonReentrant {
        require(block.timestamp >= reservations[id].expiresAt, "Not expired");
        _release(id);
    }

    function _release(bytes32 id) private {
        Reservation storage r = reservations[id];
        require(r.buyer != address(0) && !r.closed, "Reservation unavailable");
        r.closed = true;
        accounts[r.project][r.buyer].reserved -= r.maximum;
        emit ReservationReleased(id);
    }

    function withdraw(bytes32 project, uint256 amount) external nonReentrant {
        Account storage a = accounts[project][msg.sender];
        require(amount > 0 && amount <= a.locked - a.reserved, "Reserved or insufficient");
        a.locked -= amount;
        a.withdrawn += amount;
        token.safeTransferFrom(address(this), msg.sender, uint256(project), amount, "");
        emit CreditsWithdrawn(project, msg.sender, amount);
    }

    function _deposit(address from, uint256 id, uint256 amount, bytes memory data) private {
        require(msg.sender == address(token) && amount > 0, "Invalid token deposit");
        registry.requireService(bytes32(id));
        address buyer = from == address(0) ? abi.decode(data, (address)) : from;
        require(buyer != address(0) && buyer != address(this), "Invalid beneficiary");
        Account storage a = accounts[bytes32(id)][buyer];
        a.locked += amount;
        a.deposited += amount;
        emit CreditsDeposited(bytes32(id), buyer, amount);
    }

    function onERC1155Received(address, address from, uint256 id, uint256 amount, bytes memory data)
        public
        override
        nonReentrant
        returns (bytes4)
    {
        _deposit(from, id, amount, data);
        return this.onERC1155Received.selector;
    }

    function onERC1155BatchReceived(
        address,
        address from,
        uint256[] memory ids,
        uint256[] memory amounts,
        bytes memory data
    ) public override nonReentrant returns (bytes4) {
        require(from != address(0) && ids.length == amounts.length, "Invalid batch");
        for (uint256 i; i < ids.length; i++) {
            _deposit(from, ids[i], amounts[i], data);
        }
        return this.onERC1155BatchReceived.selector;
    }

    function supportsInterface(bytes4 id) public view override(AccessControl, ERC1155Holder) returns (bool) {
        return super.supportsInterface(id);
    }
}
