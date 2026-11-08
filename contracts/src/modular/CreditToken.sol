// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {ERC1155} from "@openzeppelin/contracts/token/ERC1155/ERC1155.sol";
import {ProjectRegistry} from "./ProjectRegistry.sol";

/// @notice Stable token. The fixed payment escrow is the only issuer.
contract CreditToken is ERC1155 {
    ProjectRegistry public immutable registry;
    address private immutable bootstrap;
    address public paymentEscrow;
    address public usageEscrow;
    mapping(bytes32 => uint256) public issued;

    constructor(ProjectRegistry r) ERC1155("") {
        registry = r;
        bootstrap = msg.sender;
    }

    function bindEscrows(address payment, address usage) external {
        require(
            msg.sender == bootstrap && paymentEscrow == address(0) && payment.code.length > 0 && usage.code.length > 0,
            "Binding sealed"
        );
        paymentEscrow = payment;
        usageEscrow = usage;
    }

    function issuePaid(bytes32 id, address buyer, uint256 amount) external {
        require(msg.sender == paymentEscrow && buyer != address(0) && amount > 0, "Only paid issuance");
        ProjectRegistry.Project memory p = registry.project(id);
        require(issued[id] + amount <= p.committedCredits, "Supply exceeded");
        issued[id] += amount;
        _mint(usageEscrow, uint256(id), amount, abi.encode(buyer));
    }

    function burnEscrow(bytes32 id, uint256 amount) external {
        require(msg.sender == usageEscrow, "Only usage escrow");
        _burn(usageEscrow, uint256(id), amount);
    }

    /// @notice Voluntary destruction earns no provider revenue or collateral release.
    function burnOwn(bytes32 id, uint256 amount) external {
        _burn(msg.sender, uint256(id), amount);
    }
}
