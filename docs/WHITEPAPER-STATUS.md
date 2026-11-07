# White paper status

The [10-slide PowerPoint](../apps/launchpad/public/whitepaper/ai-computing-credit-launchpad.pptx) was revised on 2026-11-07. It describes the product design and the current Sepolia prototype separately. Planned flows are labeled on-slide; the last slide lists connected capabilities and remaining work. It is not a claim that inference credit sales are operational.

| Slide | Content |
| --- | --- |
| 1 | Product name and launch/delivery proposition |
| 2 | PRL collateral versus launch-specific compute credits |
| 3 | Six-step provider publishing timeline and original stake-based 24h window |
| 4 | Provider/platform/buyer API lanes; mandatory FRPC and launch-scoped URL |
| 5 | Buyer choose → buy → use → settle journey |
| 6 | Verified delivery, credit burn, payment and collateral release |
| 7 | Editable proportional-release chart: 2,000 PRL, 60% provider-settled credits, 1,200 free and 800 locked |
| 8 | Failure, capacity/funding-limited pool replacement and responsibility review |
| 9 | Locked → free → returned collateral states |
| 10 | Current Sepolia implementation versus remaining integrations |

The proportional-release rule counts only successful, settled delivery by the original provider. Failed, unsettled and pool-served requests do not count. The denominator is a fixed issuance commitment; cumulative settlement must be capped and idempotent. Speaker notes explain these constraints and identify repository evidence. The existing deployed escrow does not implement partial unlocking or a withdrawable ledger.

The visual redesign uses deep green and warm ivory, editorial typography, timeline/lane diagrams and a native editable chart with embedded workbook data. It preserves the ten-slide limit and 16:9 dimensions. Text and diagrams remain editable. Package/layout/import checks and individual slide rendering were performed; no native PowerPoint execution is claimed. Technical behavior is maintained in [implementation status](IMPLEMENTATION-STATUS.md) and [the active staking guide](RUST-STAKING.md).
