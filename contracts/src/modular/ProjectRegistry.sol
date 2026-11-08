// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {GovernedAccess} from "./GovernedAccess.sol";

/// @notice Stable project terms. Module changes cannot edit an existing commitment.
contract ProjectRegistry is GovernedAccess {
    bytes32 public constant REGISTRAR_ROLE = keccak256("REGISTRAR_ROLE");
    bytes32 public constant ACCEPTANCE_ROLE = keccak256("ACCEPTANCE_ROLE");
    bytes32 public constant GUARDIAN_ROLE = keccak256("GUARDIAN_ROLE");
    enum Status {
        None,
        Registered,
        AwaitingRPC,
        Verified,
        Refunded
    }

    struct Project {
        address provider;
        uint256 stakeAmount;
        uint256 committedCredits;
        uint256 weiPerCredit;
        bytes32 modelHash;
        uint64 stakedAt;
        uint64 deadline;
        Status status;
        bool salesPaused;
        bool servicePaused;
    }
    mapping(bytes32 => Project) private projects;
    address public stakeVault;
    address private immutable bootstrap;
    event ProjectRegistered(
        bytes32 indexed project,
        address indexed provider,
        uint256 stake,
        uint256 supply,
        uint256 price,
        bytes32 modelHash
    );
    event ProjectStaked(bytes32 indexed project, uint64 stakedAt, uint64 deadline);
    event ProjectVerified(bytes32 indexed project, address indexed verifier);
    event ProjectRefunded(bytes32 indexed project);
    event ProjectPaused(bytes32 indexed project, bool sales, bool service);

    constructor(address admin) GovernedAccess(admin) {
        require(admin != address(0), "Zero admin");
        bootstrap = msg.sender;
    }

    function bindStakeVault(address vault) external {
        require(msg.sender == bootstrap && stakeVault == address(0) && vault.code.length > 0, "Binding sealed");
        stakeVault = vault;
    }

    function register(bytes32 id, address provider, uint256 stake, uint256 supply, uint256 price, bytes32 model)
        external
        onlyRole(REGISTRAR_ROLE)
    {
        require(
            id != 0 && provider != address(0) && stake > 0 && supply > 0 && supply <= 1e9 && price > 0
                && price <= 1 ether && model != 0,
            "Invalid terms"
        );
        require(projects[id].status == Status.None, "Already registered");
        projects[id] = Project(provider, stake, supply, price, model, 0, 0, Status.Registered, false, false);
        emit ProjectRegistered(id, provider, stake, supply, price, model);
    }

    function markStaked(bytes32 id) external {
        require(msg.sender == stakeVault, "Only vault");
        Project storage p = projects[id];
        require(p.status == Status.Registered, "Invalid state");
        p.stakedAt = uint64(block.timestamp);
        p.deadline = p.stakedAt + 24 hours;
        p.status = Status.AwaitingRPC;
        emit ProjectStaked(id, p.stakedAt, p.deadline);
    }

    function verifyProject(bytes32 id) external onlyRole(ACCEPTANCE_ROLE) {
        Project storage p = projects[id];
        require(p.status == Status.AwaitingRPC && block.timestamp < p.deadline, "Not eligible");
        p.status = Status.Verified;
        emit ProjectVerified(id, msg.sender);
    }

    function markRefunded(bytes32 id) external {
        require(msg.sender == stakeVault, "Only vault");
        Project storage p = projects[id];
        require(p.status == Status.AwaitingRPC && block.timestamp >= p.deadline, "Not expired");
        p.status = Status.Refunded;
        emit ProjectRefunded(id);
    }

    /// @notice Guardian may stop entries, never resume or move funds.
    function emergencyPause(bytes32 id, bool sales, bool service) external onlyRole(GUARDIAN_ROLE) {
        Project storage p = projects[id];
        require(p.status != Status.None, "Unknown project");
        p.salesPaused = p.salesPaused || sales;
        p.servicePaused = p.servicePaused || service;
        emit ProjectPaused(id, p.salesPaused, p.servicePaused);
    }

    function setPause(bytes32 id, bool sales, bool service) external onlyRole(DEFAULT_ADMIN_ROLE) {
        Project storage p = projects[id];
        require(p.status != Status.None, "Unknown project");
        p.salesPaused = sales;
        p.servicePaused = service;
        emit ProjectPaused(id, sales, service);
    }

    function project(bytes32 id) external view returns (Project memory) {
        return projects[id];
    }

    function requireService(bytes32 id) external view {
        Project storage p = projects[id];
        require(p.status == Status.Verified && !p.servicePaused, "Service unavailable");
    }
}
