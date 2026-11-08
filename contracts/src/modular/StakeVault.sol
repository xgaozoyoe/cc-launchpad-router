// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {ProjectRegistry} from "./ProjectRegistry.sol";
import {UsageEscrow} from "./UsageEscrow.sol";

/// @notice Stable PRL custody. No governance release, ledger replacement or arbitrary transfer.
contract StakeVault is ReentrancyGuard {
    using SafeERC20 for IERC20;
    ProjectRegistry public immutable registry;
    UsageEscrow public immutable usageEscrow;
    IERC20 public immutable stakeToken;
    mapping(bytes32 => uint256) public initial;
    mapping(bytes32 => uint256) public returned;
    event Staked(bytes32 indexed project, address indexed provider, uint256 amount);
    event Withdrawn(bytes32 indexed project, address indexed provider, uint256 amount, uint256 cumulativeReturned);
    event Refunded(bytes32 indexed project, address indexed provider, uint256 amount);

    constructor(ProjectRegistry r, UsageEscrow u, IERC20 t) {
        require(address(t).code.length > 0, "Invalid PRL");
        registry = r;
        usageEscrow = u;
        stakeToken = t;
    }

    function stake(bytes32 project) external nonReentrant {
        ProjectRegistry.Project memory p = registry.project(project);
        require(
            msg.sender == p.provider && p.status == ProjectRegistry.Status.Registered && initial[project] == 0,
            "Not eligible"
        );
        uint256 beforeBalance = stakeToken.balanceOf(address(this));
        initial[project] = p.stakeAmount;
        stakeToken.safeTransferFrom(msg.sender, address(this), p.stakeAmount);
        require(stakeToken.balanceOf(address(this)) - beforeBalance == p.stakeAmount, "Unsupported token");
        registry.markStaked(project);
        emit Staked(project, msg.sender, p.stakeAmount);
    }

    function collateralState(bytes32 project)
        public
        view
        returns (uint256 deposited, uint256 locked, uint256 free, uint256 withdrawn)
    {
        ProjectRegistry.Project memory p = registry.project(project);
        deposited = initial[project];
        withdrawn = returned[project];
        if (p.status == ProjectRegistry.Status.Refunded) return (deposited, 0, 0, withdrawn);
        uint256 unlocked;
        if (p.status == ProjectRegistry.Status.Verified) {
            uint256 delivered = usageEscrow.providerDelivered(project);
            require(delivered <= p.committedCredits, "Invalid delivery");
            unlocked = Math.mulDiv(deposited, delivered, p.committedCredits);
        }
        locked = deposited - unlocked;
        free = unlocked - withdrawn;
    }

    function withdraw(bytes32 project, uint256 amount) external nonReentrant {
        require(msg.sender == registry.project(project).provider, "Only provider");
        (,, uint256 free,) = collateralState(project);
        require(amount > 0 && amount <= free, "Insufficient free stake");
        returned[project] += amount;
        stakeToken.safeTransfer(msg.sender, amount);
        emit Withdrawn(project, msg.sender, amount, returned[project]);
    }

    function refundExpired(bytes32 project) external nonReentrant {
        ProjectRegistry.Project memory p = registry.project(project);
        require(msg.sender == p.provider, "Only provider");
        registry.markRefunded(project);
        uint256 amount = initial[project];
        returned[project] = amount;
        stakeToken.safeTransfer(msg.sender, amount);
        emit Refunded(project, msg.sender, amount);
    }
}
