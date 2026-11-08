// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {LaunchpadTimelock} from "./LaunchpadTimelock.sol";
import {ProjectRegistry} from "./ProjectRegistry.sol";
import {CreditToken} from "./CreditToken.sol";
import {UsageEscrow} from "./UsageEscrow.sol";
import {PaymentEscrow} from "./PaymentEscrow.sol";
import {StakeVault} from "./StakeVault.sol";
import {ModuleDeployer} from "./ModuleDeployer.sol";
import {SaleRouter, SettlementModule} from "./Modules.sol";

/// @notice Constructor-only bootstrap. No live role or rebind function survives deployment.
/// @dev Testnet implementation; migration portal is installed once through timelocked governance.
contract TestnetProtocolFactory {
    // Reviewed helper runtime for solc 0.8.30 / optimizer 200 / Cancun; tests fail on drift.
    bytes32 public constant MODULE_DEPLOYER_CODEHASH =
        0xb393b359fdb43eb02ba27b38476a9586c039caceb26883d361d20b39aec49fa8;
    address public immutable governance;
    address public migrationGate;
    ProjectRegistry public immutable registry;
    CreditToken public immutable creditToken;
    UsageEscrow public immutable usageEscrow;
    PaymentEscrow public immutable paymentEscrow;
    StakeVault public immutable stakeVault;
    SaleRouter public immutable saleRouter;
    SettlementModule public immutable settlementModule;
    event ProtocolDeployed(
        address indexed governance,
        address registry,
        address token,
        address usage,
        address payment,
        address stake,
        address sale,
        address settlement
    );

    constructor(
        IERC20 prl,
        LaunchpadTimelock governor,
        address registrar,
        address verifier,
        address guardian,
        address operator,
        ModuleDeployer modules
    ) {
        require(block.chainid == 11155111 || block.chainid == 31337, "Test networks only");
        require(
            address(governor).code.length > 0 && governor.getMinDelay() >= 48 hours
                && governor.MINIMUM_DELAY() == 48 hours,
            "48h governance required"
        );
        require(
            registrar != address(0) && verifier != address(0) && guardian != address(0) && operator != address(0),
            "Zero operator"
        );
        governance = address(governor);
        registry = new ProjectRegistry(address(this));
        creditToken = new CreditToken(registry);
        usageEscrow = new UsageEscrow(registry, creditToken, address(this));
        paymentEscrow = new PaymentEscrow(registry, creditToken, address(usageEscrow), address(this));
        stakeVault = new StakeVault(registry, usageEscrow, prl);
        registry.bindStakeVault(address(stakeVault));
        creditToken.bindEscrows(address(paymentEscrow), address(usageEscrow));
        usageEscrow.bindPaymentEscrow(paymentEscrow);
        require(address(modules).codehash == MODULE_DEPLOYER_CODEHASH, "Unreviewed module deployer");
        (saleRouter, settlementModule) = modules.deploy(paymentEscrow, usageEscrow, address(governor), operator);
        registry.grantRole(registry.REGISTRAR_ROLE(), registrar);
        registry.grantRole(registry.ACCEPTANCE_ROLE(), verifier);
        registry.grantRole(registry.GUARDIAN_ROLE(), guardian);
        paymentEscrow.grantRole(paymentEscrow.SALE_ROLE(), address(saleRouter));
        usageEscrow.grantRole(usageEscrow.SETTLEMENT_ROLE(), address(settlementModule));
        registry.sealGovernance(address(governor));
        paymentEscrow.sealGovernance(address(governor));
        usageEscrow.sealGovernance(address(governor));
        emit ProtocolDeployed(
            address(governor),
            address(registry),
            address(creditToken),
            address(usageEscrow),
            address(paymentEscrow),
            address(stakeVault),
            address(saleRouter),
            address(settlementModule)
        );
    }

    /// @notice Timelocked one-time configuration; no later gate replacement is possible.
    function configureMigration(address gate) external {
        require(
            msg.sender == governance && migrationGate == address(0) && gate.code.length > 0,
            "Migration configuration sealed"
        );
        (bool ok, bytes memory result) = gate.staticcall(abi.encodeWithSignature("protocol()"));
        require(ok && result.length == 32 && abi.decode(result, (address)) == address(this), "Wrong migration protocol");
        migrationGate = gate;
        registry.bindMigrationGate(gate);
        creditToken.bindMigrationGate(gate);
        usageEscrow.bindMigrationGate(gate);
        paymentEscrow.bindMigrationGate(gate);
        stakeVault.bindMigrationGate(gate);
    }
}
