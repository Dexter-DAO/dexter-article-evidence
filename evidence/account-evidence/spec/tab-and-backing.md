## 7. Escrow, surplus and the Tab contract

Keep spending permission, accepted guarantee capacity and earned payable as separate state. A Tab is a commercial channel; a delivery is an economic origin; a claim is an enforceable consequence. Opening a logical Tab does not imply a debt or an escrow transfer of its entire authorization ceiling.

| Arrangement | Whose resources become committed? | When cash moves | Result after delivery | Reuse/release and supplier service |
|---|---|---|---|---|
| Cash at delivery | Buyer's finite accepted payment authorization | Buyer wallet to supplier as the accepted purchase executes | Paid receipt; no residual issuer debt/payable | Remaining free cash remains usable; authorized refund is a distinct later right |
| Cash prefunded for a finite order | Buyer's exact order reserve | Buyer wallet to a protected source before the protected promise is accepted | Supplier's funded right is paid against the reserved amount | Only provably unused capacity returns; full mandate ceiling is not funded |
| Supplier-carried credit | Supplier carries the buyer's credit risk | No funding transfer is required at acceptance/capture | One buyer-to-supplier debt | Current creditor collects/assigns; no fictitious issuer guarantee or self-payable |
| Wallet-backed source | Buyer's or issuer's exact wallet reserve under a complete control graph | Cash can remain in the managed wallet until payout | Accepted capacity becomes the earned supplier right defined by that profile | Every spending and management path must preserve the floor; settlement must survive agent rotation |
| Pool cash guarantee | Pool investors' committed source cash | Pool treasury to Core escrow before the guarantee becomes accepted | Buyer debt to issuer plus issuer payable to supplier, created once | Unused reserve can release; payable backing survives agent/issuer outage; supplier can use Core payout |

The previous LockedClaim design kept USDC in Swig under a reservation. Current wallet-backed
financing also reserves accepted capacity without an acceptance/release transfer. Core escrow
holds the protected cash separately. Both implemented profiles have signed local evidence;
the complete customer and backend-off acceptance case remains to qualify. Wallet reservations
depend on the admitted control graph across collection, withdrawals and permitted control changes.

### 7.1 Capital utilization and costs

In the original conformance escrow notation, `G` is accounted beneficial cash in Core escrow;
`R` is accepted unused guarantee capacity and `P` is earned unpaid supplier payable.
Coverage is `G >= R + P`, with equality for an exactly allocated source absent separately
accounted excess commitments. Actual token balance may be larger than `G`. The current
[financier contract](../docs/financing/FINANCIER-CONTRACT.md) uses `G = R + P` for the protected
floor of a custody account and tracks its total accounted cash separately, including free cash.
Use the selected profile's fields when implementing these requirements.

Accepting an exact guarantee allocates capital to protected backing and creates `R`.
Current financed acceptance reserves cash already accounted in the selected custody;
it performs no token transfer for either profile. Escrow must be funded in Core custody
before acceptance, while wallet backing uses cash retained in the governed financier wallet.
Capture converts `R` into buyer debt and `P` without a token transfer. Payout transfers
the exact cash and reduces payable and backing while retaining debt. Repayment goes to
the current creditor and reduces debt independently of any remaining supplier payable.
Unused release respects accepted expiration and delivery rules and preserves earned backing.

Use one escrow per admitted source/partition rather than one ATA per order. Prefer short exact increments before each bounded delivery when the product permits. Open limits are supported, but the portion actually promised to the supplier consumes corresponding source capacity until release. Sponsor network execution separately from the buyer's expenditure budget. Quote protocol fees, issuer commitment/credit fees and investor economics explicitly; fees are not unexplained changes to principal.

Netting releases capital only when actual enforceable mutual residual claims are extinguished under the proper parties and priority rules. Expected revenue, a buyer's unused authorization, an unrelated creditor's receivable or a promised uncommitted lender wallet balance cannot fund a guarantee. A source that invests reserved cash in another instrument is a different liquidity-risk profile, not this cash guarantee.
