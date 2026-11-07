// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {AccessControl} from "@openzeppelin/contracts/access/AccessControl.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
/// @notice Registered project stake escrow. Credit sales and slashing are outside this contract.
contract LaunchStake is AccessControl, ReentrancyGuard {
    using SafeERC20 for IERC20;
    bytes32 public constant VERIFIER_ROLE = keccak256("VERIFIER_ROLE");
    bytes32 public constant SETTLER_ROLE = keccak256("SETTLER_ROLE");
    uint64 public constant VERIFICATION_WINDOW = 24 hours;
    IERC20 public immutable stakeToken;
    enum Status { None, AwaitingStake, AwaitingRPC, Verified, Refunded, Released }
    struct Project { address provider; uint256 amount; uint64 stakedAt; uint64 deadline; Status status; }
    mapping(bytes32 => Project) private _projects;
    event ProjectRegistered(bytes32 indexed projectId, address indexed provider, uint256 amount);
    event ProjectStaked(bytes32 indexed projectId, address indexed provider, uint256 amount, uint64 stakedAt, uint64 verificationDeadline);
    event ProjectVerified(bytes32 indexed projectId, address indexed verifier);
    event StakeRefunded(bytes32 indexed projectId, address indexed provider, uint256 amount);
    event StakeReleased(bytes32 indexed projectId, address indexed provider, uint256 amount, bytes32 reason);
    error InvalidProject(); error InvalidState(); error NotProvider();
    error DeadlinePassed(); error DeadlineNotPassed(); error UnsupportedToken();
    constructor(IERC20 token, address admin) {
        if (address(token).code.length == 0 || admin == address(0)) revert InvalidProject();
        stakeToken = token; _grantRole(DEFAULT_ADMIN_ROLE, admin);
    }
    /// @dev ID is SHA-256 of backend project UUID UTF-8, supplied as bytes32.
    function registerProject(bytes32 projectId, address provider, uint256 amount) external onlyRole(DEFAULT_ADMIN_ROLE) {
        if (projectId == bytes32(0) || provider == address(0) || amount == 0) revert InvalidProject();
        if (_projects[projectId].status != Status.None) revert InvalidState();
        _projects[projectId] = Project(provider, amount, 0, 0, Status.AwaitingStake);
        emit ProjectRegistered(projectId, provider, amount);
    }
    function stake(bytes32 projectId) external nonReentrant {
        Project storage p = _projects[projectId];
        if (p.status != Status.AwaitingStake) revert InvalidState();
        if (msg.sender != p.provider) revert NotProvider();
        uint256 beforeBalance = stakeToken.balanceOf(address(this));
        p.stakedAt = uint64(block.timestamp); p.deadline = p.stakedAt + VERIFICATION_WINDOW; p.status = Status.AwaitingRPC;
        stakeToken.safeTransferFrom(msg.sender, address(this), p.amount);
        if (stakeToken.balanceOf(address(this)) - beforeBalance != p.amount) revert UnsupportedToken();
        emit ProjectStaked(projectId, msg.sender, p.amount, p.stakedAt, p.deadline);
    }
    /// @notice Real RPC/model/capacity acceptance must be mined before deadline.
    function verifyProject(bytes32 projectId) external onlyRole(VERIFIER_ROLE) {
        Project storage p = _projects[projectId];
        if (p.status != Status.AwaitingRPC) revert InvalidState();
        if (block.timestamp >= p.deadline) revert DeadlinePassed();
        p.status = Status.Verified; emit ProjectVerified(projectId, msg.sender);
    }
    function refundExpired(bytes32 projectId) external nonReentrant {
        Project storage p = _projects[projectId];
        if (msg.sender != p.provider) revert NotProvider();
        if (p.status != Status.AwaitingRPC) revert InvalidState();
        if (block.timestamp < p.deadline) revert DeadlineNotPassed();
        p.status = Status.Refunded; stakeToken.safeTransfer(p.provider, p.amount);
        emit StakeRefunded(projectId, p.provider, p.amount);
    }
    /// @notice Trusted settlement decision after delivery and disputes. No supplier unilateral withdrawal.
    function releaseStake(bytes32 projectId, bytes32 reason) external onlyRole(SETTLER_ROLE) nonReentrant {
        Project storage p = _projects[projectId];
        if (p.status != Status.Verified || reason == bytes32(0)) revert InvalidState();
        p.status = Status.Released; stakeToken.safeTransfer(p.provider, p.amount);
        emit StakeReleased(projectId, p.provider, p.amount, reason);
    }
    function stakeState(bytes32 projectId) external view returns (uint8 status, uint256 amount, uint64 stakedAt, uint64 deadline, address provider) {
        Project storage p = _projects[projectId]; return (uint8(p.status), p.amount, p.stakedAt, p.deadline, p.provider);
    }
}
