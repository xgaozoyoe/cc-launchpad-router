// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {AccessControl} from "@openzeppelin/contracts/access/AccessControl.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
interface IProviderDeliveryCredits {
    function COLLATERAL_ACCOUNTING_VERSION() external view returns(uint256);
    function stakeContract() external view returns(address);
    function providerSettledCredits(bytes32) external view returns(uint256);
    function offerState(bytes32) external view returns(address,uint256,uint256,uint256,bool,bytes32);
}
/// @notice Test-only proportional collateral escrow for NEW projects. No legacy fund migration.

contract LaunchStakeV2 is AccessControl, ReentrancyGuard {
    using SafeERC20 for IERC20;
    bytes32 public constant VERIFIER_ROLE = keccak256("VERIFIER_ROLE");
    bytes32 public constant SETTLER_ROLE = keccak256("SETTLER_ROLE");
    uint64 public constant VERIFICATION_WINDOW = 24 hours;
    IERC20 public immutable stakeToken;
    enum Status { None, AwaitingStake, AwaitingRPC, Verified, Refunded, Released }
    struct Project { address provider; uint256 amount; uint64 stakedAt; uint64 deadline; Status status; }
    mapping(bytes32 => Project) private _projects;
    IProviderDeliveryCredits public creditContract;
    mapping(bytes32=>uint256) public collateralWithdrawn;
    event CreditContractBound(address indexed credits);
    event CollateralWithdrawn(bytes32 indexed projectId,address indexed provider,uint256 amount,uint256 cumulativeWithdrawn);
    error UseProgressiveWithdrawal();
    function COLLATERAL_VERSION() external pure returns(uint256){return 2;}
    /// @notice One-time binding to a V2 credit contract anchored to this escrow.
    function bindCreditContract(address credits) external onlyRole(DEFAULT_ADMIN_ROLE) {
        if(address(creditContract)!=address(0)||credits.code.length==0) revert InvalidProject();
        IProviderDeliveryCredits target=IProviderDeliveryCredits(credits);
        if(target.stakeContract()!=address(this)||target.COLLATERAL_ACCOUNTING_VERSION()!=2) revert InvalidProject();
        creditContract=target;emit CreditContractBound(credits);
    }
    /// @notice Total unlocked is cumulative; withdrawals cannot make it unlock twice.
    function collateralState(bytes32 projectId) public view returns(uint256 initial,uint256 locked,uint256 free,uint256 returned,uint256 delivered,uint256 supply) {
        Project storage p=_projects[projectId];
        if(p.status==Status.None||p.status==Status.AwaitingStake)return(0,0,0,0,0,0);
        initial=p.amount;
        if(p.status==Status.Refunded)return(initial,0,0,initial,0,0);
        returned=collateralWithdrawn[projectId];uint256 unlocked;
        if(p.status==Status.Verified&&address(creditContract)!=address(0)) {
            (address provider,uint256 committed,,,,)=creditContract.offerState(projectId);
            if(committed>0) {
                if(provider!=p.provider) revert InvalidProject();
                supply=committed;delivered=creditContract.providerSettledCredits(projectId);
                if(delivered>supply) revert InvalidState();
                unlocked=Math.mulDiv(initial,delivered,supply);
            }
        }
        locked=initial-unlocked;free=unlocked-returned;
    }
    function withdrawCollateral(bytes32 projectId,uint256 amount) external nonReentrant {
        Project storage p=_projects[projectId];
        if(msg.sender!=p.provider)revert NotProvider();
        if(p.status!=Status.Verified)revert InvalidState();
        (,,uint256 free,,,)=collateralState(projectId);
        if(amount==0||amount>free)revert InvalidState();
        collateralWithdrawn[projectId]+=amount;
        stakeToken.safeTransfer(p.provider,amount);
        // Keep Verified so partial/full fulfilled withdrawals do not break launch/credit identity.
        emit CollateralWithdrawn(projectId,p.provider,amount,collateralWithdrawn[projectId]);
    }
    event ProjectRegistered(bytes32 indexed projectId, address indexed provider, uint256 amount);
    event ProjectStaked(bytes32 indexed projectId, address indexed provider, uint256 amount, uint64 stakedAt, uint64 verificationDeadline);
    event ProjectVerified(bytes32 indexed projectId, address indexed verifier);
    event StakeRefunded(bytes32 indexed projectId, address indexed provider, uint256 amount);
    event StakeReleased(bytes32 indexed projectId, address indexed provider, uint256 amount, bytes32 reason);
    error InvalidProject(); error InvalidState(); error NotProvider();
    error DeadlinePassed(); error DeadlineNotPassed(); error UnsupportedToken();
    constructor(IERC20 token, address admin) {
        require(block.chainid==11155111||block.chainid==31337,"Test networks only");
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
    /// @notice Disable the legacy whole-deposit release path: even an admin/settler cannot skip delivery.
    function releaseStake(bytes32,bytes32) external pure {revert UseProgressiveWithdrawal();}
    function stakeState(bytes32 projectId) external view returns (uint8 status, uint256 amount, uint64 stakedAt, uint64 deadline, address provider) {
        Project storage p = _projects[projectId]; return (uint8(p.status), p.amount, p.stakedAt, p.deadline, p.provider);
    }
}
