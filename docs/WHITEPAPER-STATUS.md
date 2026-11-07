# White paper status

The [10-slide PowerPoint](../apps/launchpad/public/whitepaper/ai-computing-credit-launchpad.pptx) was revised on 2026-11-07. It describes the product design and the current Sepolia prototype separately. Planned flows are labeled on-slide; the last slide lists connected capabilities and remaining work. It is not a claim that inference credit sales are operational.

| Slide | Content |
| --- | --- |
| 1 | AI Computing Credit Launchpad; product design plus Sepolia prototype |
| 2 | Provider, launchpad and buyer roles; PRL collateral does not establish GPU capacity |
| 3 | Create project → finalized stake → Launch/Provider IDs → mandatory FRPC/RPC acceptance; deadline is stake block time + 24h |
| 4 | Buy one launch's credits → use inference → settle accepted usage; buyer flow remains planned |
| 5 | Public-pool operator journey; replacement depends on capacity/funding and does not unlock primary-provider PRL |
| 6 | Separate PRL collateral and burned compute credits; locked → free → withdrawal is planned, current escrow has full returns only |
| 7 | Provider delivery/settlement and launch-scoped API; burn, payment and PRL unlocking are not integrated |
| 8 | Supplier failure, attempted replacement and evidence review; failover/slashing are planned |
| 9 | Proportional release example: 2,000 PRL × 60m settled provider credits / 100m fixed committed credits = 1,200 free; 800 locked, before withdrawal or penalties |
| 10 | Google/project/wallet/stake/ID path connected; FRPC acceptance, sales, burn, settlement and partial release not connected |

The proportional-release rule counts only successful, settled delivery by the original provider. Failed, unsettled and pool-served requests do not count. The denominator is a fixed issuance commitment; cumulative settlement must be capped and idempotent. Speaker notes explain these constraints and identify repository evidence. The existing deployed escrow does not implement partial unlocking or a withdrawable ledger.

The revision retains editable text and process diagrams, the original ten-slide limit, dimensions and typography. Package/layout/import checks and individual slide rendering were performed; no native PowerPoint execution is claimed. Technical behavior is maintained in [implementation status](IMPLEMENTATION-STATUS.md) and [the active staking guide](RUST-STAKING.md).
