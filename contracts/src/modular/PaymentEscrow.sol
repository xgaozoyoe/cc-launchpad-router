// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {GovernedAccess} from "./GovernedAccess.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {ProjectRegistry} from "./ProjectRegistry.sol";
import {CreditToken} from "./CreditToken.sol";

/// @notice Stable ETH custody and project liabilities; no admin sweep or arbitrary payout.
contract PaymentEscrow is GovernedAccess, ReentrancyGuard {
    bytes32 public constant SALE_ROLE = keccak256("SALE_ROLE");

    struct Funds {
        uint256 paid;
        uint256 reserve;
        uint256 earned;
        uint256 claimed;
    }
    mapping(bytes32 => Funds) public funds;
    ProjectRegistry public immutable registry;
    CreditToken public immutable token;
    address public immutable usageEscrow;
    event Purchase(bytes32 indexed project, address indexed buyer, uint256 credits, uint256 paid);
    event RevenueEarned(bytes32 indexed project, bytes32 indexed usageId, uint256 amount);
    event RevenueClaimed(bytes32 indexed project, address indexed provider, uint256 amount);

    constructor(ProjectRegistry r, CreditToken t, address usage, address admin) GovernedAccess(admin) {
        registry = r;
        token = t;
        usageEscrow = usage;
    }

    function purchase(bytes32 id, address buyer, uint256 amount) external payable onlyRole(SALE_ROLE) nonReentrant {
        registry.requireService(id);
        ProjectRegistry.Project memory p = registry.project(id);
        require(
            !p.salesPaused && buyer != address(0) && amount > 0 && msg.value == amount * p.weiPerCredit,
            "Invalid purchase"
        );
        Funds storage f = funds[id];
        f.paid += msg.value;
        f.reserve += msg.value;
        // The payment escrow issues immediately. Routers never receive a reusable mint permission.
        token.issuePaid(id, buyer, amount);
        emit Purchase(id, buyer, amount, msg.value);
    }

    function settleProvider(bytes32 id, bytes32 usageId, uint256 credits) external {
        require(msg.sender == usageEscrow, "Only usage escrow");
        uint256 value = credits * registry.project(id).weiPerCredit;
        Funds storage f = funds[id];
        require(credits > 0 && value <= f.reserve, "Insufficient project reserve");
        f.reserve -= value;
        f.earned += value;
        emit RevenueEarned(id, usageId, value);
    }

    function claimRevenue(bytes32 id, uint256 amount) external nonReentrant {
        require(msg.sender == registry.project(id).provider, "Only provider");
        Funds storage f = funds[id];
        require(amount > 0 && amount <= f.earned, "Insufficient earned");
        f.earned -= amount;
        f.claimed += amount;
        (bool ok,) = payable(msg.sender).call{value: amount}("");
        require(ok, "Transfer failed");
        emit RevenueClaimed(id, msg.sender, amount);
    }
}
