# White paper status

The downloadable [10-slide PowerPoint](../apps/launchpad/public/whitepaper/ai-computing-credit-launchpad.pptx) is a **product concept**, not implementation documentation. Reviewed 2026-11-07. This review preserves the presentation file and records where it differs from the application; it does not claim the slides have been rewritten.

| Slides | Interpretation / discrepancy |
| --- | --- |
| 1–2 | Intended computing-credit product and participants. Credit token issuance, redemption and pool replacement are not live. |
| 3 | Supplier journey omits the implemented draft → wallet binding → finalized stake → Launch ID → 24h RPC window. The statement about releasing PRL after a dispute period predates the agreed proportional-release rule. |
| 4 | Buyer payment escrow, accepted-usage burn and refund policy are target behavior; only browser simulations exist. |
| 5 | Pool qualification, readiness fees, rerouting and payments are planned, not operational services. |
| 6 | Stake/credit distinction is a design principle. Actual test escrow has full return only; free/withdrawable and progressive release need a new accounting path. |
| 7–8 | Successful settlement, replacement service and collateral penalties are planned. No integrated metering/settlement/slashing exists. |
| 9 | The 100m-token/$1,000 example is illustrative, not an actual payment or validated pricing model. Under the newly agreed rule, only the provider-delivered and settled portion counts toward PRL unlocking; pool completion does not. |
| 10 | Describes launch prerequisites. The actual implementation is a Sepolia test stake prototype, not an operational credit sale. |

Agreed amendment for the next white paper revision: cumulative unlocked PRL equals initial stake multiplied by provider-delivered and settled credits divided by the fixed committed credit supply. Failed, unsettled and pool-served requests do not unlock supplier collateral. Free collateral is separately withdrawable; this is not implemented in the current escrow.

Use [implementation status](IMPLEMENTATION-STATUS.md) and [the active staking guide](RUST-STAKING.md) for current technical behavior. A future presentation revision should incorporate this rule and the implemented publishing sequence while retaining the ten-slide limit and visual user journeys.
