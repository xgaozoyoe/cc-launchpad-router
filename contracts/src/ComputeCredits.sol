// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {IERC165} from "@openzeppelin/contracts/utils/introspection/IERC165.sol";
import {ERC1155} from "@openzeppelin/contracts/token/ERC1155/ERC1155.sol";
import {IERC1155Receiver} from "@openzeppelin/contracts/token/ERC1155/IERC1155Receiver.sol";
import {AccessControl} from "@openzeppelin/contracts/access/AccessControl.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
interface ILaunchStake {function stakeState(bytes32) external view returns(uint8,uint256,uint64,uint64,address);}
/// @notice Sepolia test credit escrow. One credit buys one reported output token. PRL is untouched.
contract ComputeCredits is ERC1155,AccessControl,ReentrancyGuard,IERC1155Receiver {
 bytes32 public constant SETTLER_ROLE=keccak256("SETTLER_ROLE");
 bytes32 public constant OFFER_MANAGER_ROLE=keccak256("OFFER_MANAGER_ROLE");
 ILaunchStake public immutable stakeContract;
 struct Offering {address provider;uint256 supply;uint256 issued;uint256 weiPerCredit;bool active;bytes32 modelHash;}
 struct Account {uint256 locked;uint256 deposited;uint256 burned;uint256 withdrawn;uint256 withdrawAmount;uint256 nonce;bool approved;bool pending;}
 mapping(bytes32=>Offering) private offers;mapping(bytes32=>mapping(address=>Account)) private accounts;
 mapping(bytes32=>bytes32) public batchDigest;mapping(bytes32=>bool) public usageSettled;mapping(address=>uint256) public revenue;
 event OfferingCreated(bytes32 indexed project,address indexed provider,uint256 supply,uint256 weiPerCredit,bytes32 modelHash);
 event CreditsPurchased(bytes32 indexed project,address indexed buyer,uint256 amount,uint256 paid);
 event CreditsDeposited(bytes32 indexed project,address indexed buyer,uint256 amount);
 event UsageSettled(bytes32 indexed batch,bytes32 indexed usageId,bytes32 indexed project,address buyer,uint256 amount);
 event WithdrawalRequested(bytes32 indexed project,address indexed buyer,uint256 amount,uint256 nonce);
 event WithdrawalApproved(bytes32 indexed project,address indexed buyer,uint256 amount,uint256 nonce);
 event CreditsWithdrawn(bytes32 indexed project,address indexed buyer,uint256 amount,uint256 nonce);
 event RevenueClaimed(address indexed provider,uint256 amount);
 constructor(address staking,address admin,address settler,address manager) ERC1155("") {
  require(block.chainid==11155111||block.chainid==31337,"Test networks only");require(staking.code.length>0&&admin!=address(0)&&settler!=address(0)&&manager!=address(0),"Bad config");
  stakeContract=ILaunchStake(staking);_grantRole(DEFAULT_ADMIN_ROLE,admin);_grantRole(SETTLER_ROLE,settler);_grantRole(OFFER_MANAGER_ROLE,manager);
 }
 function _verified(bytes32 project) private view returns(address provider){(uint8 status,,,,address who)=stakeContract.stakeState(project);require(status==3,"Project not verified");return who;}
 function createOffering(bytes32 project,uint256 supply,uint256 price,bytes32 modelHash) external {
  address provider=_verified(project);require(msg.sender==provider||hasRole(OFFER_MANAGER_ROLE,msg.sender),"Not provider/manager");require(offers[project].provider==address(0)&&supply>0&&supply<=1e9&&price>0&&price<=1 ether&&modelHash!=0,"Invalid offering");
  offers[project]=Offering(provider,supply,0,price,true,modelHash);emit OfferingCreated(project,provider,supply,price,modelHash);
 }
 function closeSales(bytes32 project) external {require(msg.sender==offers[project].provider||hasRole(OFFER_MANAGER_ROLE,msg.sender),"Not provider/manager");offers[project].active=false;}
 function buyAndLock(bytes32 project,uint256 amount) external payable nonReentrant {
  _verified(project);Offering storage o=offers[project];require(o.active&&amount>0&&o.issued+amount<=o.supply&&msg.value==amount*o.weiPerCredit,"Invalid purchase");
  o.issued+=amount;Account storage a=accounts[project][msg.sender];a.locked+=amount;a.deposited+=amount;_mint(address(this),uint256(project),amount,"");emit CreditsPurchased(project,msg.sender,amount,msg.value);
 }
 function depositCredits(bytes32 project,uint256 amount) external nonReentrant {_verified(project);require(amount>0,"Zero deposit");_safeTransferFrom(msg.sender,address(this),uint256(project),amount,"");}
 function onERC1155Received(address,address from,uint256 id,uint256 amount,bytes calldata) external returns(bytes4){require(msg.sender==address(this),"Foreign token");if(from!=address(0)){_verified(bytes32(id));accounts[bytes32(id)][from].locked+=amount;accounts[bytes32(id)][from].deposited+=amount;emit CreditsDeposited(bytes32(id),from,amount);}return this.onERC1155Received.selector;}
 function onERC1155BatchReceived(address,address from,uint256[] calldata ids,uint256[] calldata amounts,bytes calldata) external returns(bytes4){require(msg.sender==address(this)&&from!=address(0),"Invalid deposit");for(uint i;i<ids.length;i++){_verified(bytes32(ids[i]));accounts[bytes32(ids[i])][from].locked+=amounts[i];accounts[bytes32(ids[i])][from].deposited+=amounts[i];emit CreditsDeposited(bytes32(ids[i]),from,amounts[i]);}return this.onERC1155BatchReceived.selector;}
 function settleBatch(bytes32 batch,bytes32[] calldata projects,address[] calldata buyers,uint256[] calldata amounts,bytes32[] calldata usages) external onlyRole(SETTLER_ROLE) nonReentrant {
  require(batch!=0&&projects.length>0&&projects.length<=32&&projects.length==buyers.length&&buyers.length==amounts.length&&amounts.length==usages.length,"Bad batch");bytes32 digest=keccak256(abi.encode(projects,buyers,amounts,usages));
  if(batchDigest[batch]!=0){require(batchDigest[batch]==digest,"Batch mismatch");return;}batchDigest[batch]=digest;
  for(uint i;i<projects.length;i++){require(!usageSettled[usages[i]]&&usages[i]!=0&&amounts[i]>0,"Duplicate/zero usage");Offering storage o=offers[projects[i]];require(o.provider!=address(0),"No offering");Account storage a=accounts[projects[i]][buyers[i]];require(a.locked>=amounts[i]+(a.approved?a.withdrawAmount:0),"Insufficient uncommitted balance");usageSettled[usages[i]]=true;a.locked-=amounts[i];a.burned+=amounts[i];_burn(address(this),uint256(projects[i]),amounts[i]);revenue[o.provider]+=amounts[i]*o.weiPerCredit;emit UsageSettled(batch,usages[i],projects[i],buyers[i],amounts[i]);}
 }
 function requestWithdrawal(bytes32 project,uint256 amount) external {Account storage a=accounts[project][msg.sender];require(!a.pending&&amount>0&&amount<=a.locked,"Invalid withdrawal");a.pending=true;a.approved=false;a.withdrawAmount=amount;a.nonce++;emit WithdrawalRequested(project,msg.sender,amount,a.nonce);}
 function approveWithdrawal(bytes32 project,address buyer,uint256 nonce,uint256 amount) external onlyRole(SETTLER_ROLE){Account storage a=accounts[project][buyer];require(a.pending&&!a.approved&&a.nonce==nonce&&amount<=a.withdrawAmount&&amount<=a.locked,"Invalid approval");a.withdrawAmount=amount;a.approved=true;emit WithdrawalApproved(project,buyer,amount,nonce);}
 function cancelWithdrawal(bytes32 project) external {Account storage a=accounts[project][msg.sender];require(a.pending,"No withdrawal");a.pending=false;a.approved=false;a.withdrawAmount=0;a.nonce++;}
 function completeWithdrawal(bytes32 project) external nonReentrant {Account storage a=accounts[project][msg.sender];require(a.pending&&a.approved,"Not reconciled");uint256 amount=a.withdrawAmount;uint256 nonce=a.nonce;a.pending=false;a.approved=false;a.withdrawAmount=0;a.locked-=amount;a.withdrawn+=amount;if(amount>0)_safeTransferFrom(address(this),msg.sender,uint256(project),amount,"");emit CreditsWithdrawn(project,msg.sender,amount,nonce);}
 function claimRevenue() external nonReentrant {uint256 amount=revenue[msg.sender];require(amount>0,"No revenue");revenue[msg.sender]=0;(bool ok,)=payable(msg.sender).call{value:amount}("");require(ok,"Transfer failed");emit RevenueClaimed(msg.sender,amount);}
 function offerState(bytes32 project) external view returns(Offering memory){return offers[project];}
 function accountState(bytes32 project,address buyer) external view returns(Account memory){return accounts[project][buyer];}
 function supportsInterface(bytes4 id) public view override(ERC1155,AccessControl,IERC165) returns(bool){return id==type(IERC1155Receiver).interfaceId||super.supportsInterface(id);}
}
