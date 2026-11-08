// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {LaunchpadTimelock} from "./LaunchpadTimelock.sol";
import {ProjectRegistry} from "./ProjectRegistry.sol";
import {CreditToken} from "./CreditToken.sol";
import {UsageEscrow} from "./UsageEscrow.sol";
import {PaymentEscrow} from "./PaymentEscrow.sol";
import {StakeVault} from "./StakeVault.sol";
import {SaleRouter, SettlementModule} from "./Modules.sol";

/// @notice Constructor-only bootstrap. No live role or rebind function survives deployment.
/// @dev Testnet implementation; migration/import functionality intentionally absent.
contract TestnetProtocolFactory {
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
        LaunchpadTimelock governance,
        address registrar,
        address verifier,
        address guardian,
        address operator
    ) {
        require(block.chainid == 11155111 || block.chainid == 31337, "Test networks only");
        require(
            address(governance).code.length > 0 && governance.getMinDelay() >= 48 hours
                && governance.MINIMUM_DELAY() == 48 hours,
            "48h governance required"
        );
        require(
            registrar != address(0) && verifier != address(0) && guardian != address(0) && operator != address(0),
            "Zero operator"
        );
        ProjectRegistry r = new ProjectRegistry(address(this));
        CreditToken t = new CreditToken(r);
        UsageEscrow u = new UsageEscrow(r, t, address(this));
        PaymentEscrow e = new PaymentEscrow(r, t, address(u), address(this));
        StakeVault s = new StakeVault(r, u, prl);
        r.bindStakeVault(address(s));
        t.bindEscrows(address(e), address(u));
        u.bindPaymentEscrow(e);
        SaleRouter sale = new SaleRouter(e);
        SettlementModule settlement = new SettlementModule(u, address(governance), operator);
        r.grantRole(r.REGISTRAR_ROLE(), registrar);
        r.grantRole(r.ACCEPTANCE_ROLE(), verifier);
        r.grantRole(r.GUARDIAN_ROLE(), guardian);
        e.grantRole(e.SALE_ROLE(), address(sale));
        u.grantRole(u.SETTLEMENT_ROLE(), address(settlement));
        r.sealGovernance(address(governance));
        e.sealGovernance(address(governance));
        u.sealGovernance(address(governance));
        registry = r;
        creditToken = t;
        usageEscrow = u;
        paymentEscrow = e;
        stakeVault = s;
        saleRouter = sale;
        settlementModule = settlement;
        emit ProtocolDeployed(
            address(governance),
            address(r),
            address(t),
            address(u),
            address(e),
            address(s),
            address(sale),
            address(settlement)
        );
    }
}
