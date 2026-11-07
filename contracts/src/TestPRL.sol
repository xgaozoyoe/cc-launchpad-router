// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";

/// @notice Test-only PRL substitute. Not the real PRL token and has no monetary backing.
contract TestPRL is ERC20, Ownable {
    constructor(address owner, uint256 initialSupply) ERC20("Launchpad Test PRL", "tPRL") Ownable(owner) {
        require(block.chainid == 11155111 || block.chainid == 31337, "Test networks only");
        _mint(owner, initialSupply);
    }
    function mint(address recipient, uint256 amount) external onlyOwner { _mint(recipient, amount); }
}
