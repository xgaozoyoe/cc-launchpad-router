# White paper status

The [10-slide PowerPoint](../apps/launchpad/public/whitepaper/ai-computing-credit-launchpad.pptx) was revised on 2026-11-07. It describes the product design and the current Sepolia prototype separately. Planned flows are labeled on-slide; the last slide lists connected capabilities and remaining work. The deck predates the test credit-purchase integration. For the current Sepolia purchase/usage/burn and withdrawal implementation, use [credit spending](CREDIT-SPENDING.md); pool fallback and proportional PRL release remain planned.

| Slide | Content |
| --- | --- |
| 1 | Provider/developer needs and the reason for a future-compute launchpad |
| 2 | Running example: 100m credits committed, 2,000 PRL collateral, one developer buys 10m |
| 3 | Complete publish → reserve → deliver → settle loop |
| 4 | Why compute credits and PRL collateral are distinct |
| 5 | Provider project/stake/identity/setup/acceptance/sales journey |
| 6 | Developer choose/buy/call/track journey for the same example |
| 7 | Across buyers, 60m original-provider credits settled: 1,200 PRL free, 800 locked |
| 8 | Pool replacement of the other 40m adds no original-provider PRL unlock |
| 9 | Provider and developer records; real My launches versus simulated buyer data |
| 10 | Connected Sepolia prototype versus the remaining sales/delivery integrations |

The proportional-release rule counts only successful, settled delivery by the original provider. Failed, unsettled and pool-served requests do not count. The denominator is a fixed issuance commitment; cumulative settlement must be capped and idempotent. Speaker notes explain these constraints and identify repository evidence. The existing deployed escrow does not implement partial unlocking or a withdrawable ledger.

The narrative starts with user needs and follows one concrete example before explaining assets and implementation. Every slide uses the same warm ivory background, deep green headings and muted green body text; there are no alternating dark pages. Timelines and the native editable chart retain embedded workbook data. It preserves the ten-slide limit and 16:9 dimensions. Text and diagrams remain editable. Package/layout/import checks and individual slide rendering were performed; no native PowerPoint execution is claimed. Technical behavior is maintained in [implementation status](IMPLEMENTATION-STATUS.md) and [the active staking guide](RUST-STAKING.md).
