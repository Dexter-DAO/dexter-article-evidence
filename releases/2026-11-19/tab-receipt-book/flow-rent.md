| flow | side | transactions | accounts created | rent paid (SOL) | accounts left | SOL locked after |
|---|---|---|---|---|---|---|
| one-time setup (principals, books, projections, custody, facility, line, budget, mandates, permissions) | main | 56 | 60 | 0.326041200 | 60 | 0.326041200 |
| one-time setup (principals, books, projections, custody, facility, line, budget, mandates, permissions) | branch | 56 | 55 | 0.164339520 | 55 | 0.164339520 |
| order, commit to close: cash repayment settles it, closes appended to the settling repayment | main | 4 | 10 | 0.173297040 | 10 | 0.173297040 |
| order, commit to close: cash repayment settles it, closes appended to the settling repayment | branch | 4 | 3 | 0.016891920 | 0 | 0.000000000 |
| order, commit to close: Swig-governed final payment, keeper sends the closes separately (Tab records; its exact repayment approvals are the next rows) | main | 12 | 14 | 0.320765520 | 14 | 0.320765520 |
| order, commit to close: Swig-governed final payment, keeper sends the closes separately (Tab records; its exact repayment approvals are the next rows) | branch | 11 | 5 | 0.024506160 | 0 | 0.000000000 |
| exact repayment approval: grant, then close_approval after expiry (the payment it authorizes is measured with its order) | main | 1 | 1 | 0.005714160 | 1 | 0.005714160 |
| exact repayment approval: grant, then close_approval after expiry (the payment it authorizes is measured with its order) | branch | 2 | 1 | 0.005547120 | 0 | 0.000000000 |
| evidence order, commit to close: no records, finalized at zero | main | 2 | 10 | 0.146236560 | 10 | 0.146236560 |
| evidence order, commit to close: no records, finalized at zero | branch | 3 | 3 | 0.021847440 | 0 | 0.000000000 |
| wallet send, standing grant | main | 1 | 2 | 0.044133360 | 2 | 0.044133360 |
| wallet send, standing grant | branch | 1 | 0 | 0.000000000 | 0 | 0.000000000 |
| wallet send, exact approval: grant, send, then (branch) close_approval after expiry | main | 2 | 3 | 0.049743120 | 3 | 0.049743120 |
| wallet send, exact approval: grant, send, then (branch) close_approval after expiry | branch | 3 | 1 | 0.005442720 | 0 | 0.000000000 |
| exact spending approval never used: grant, then close_approval after expiry | branch | 2 | 1 | 0.005442720 | 0 | 0.000000000 |
| SPL swap, standing grant | main | 1 | 3 | 0.081000480 | 3 | 0.081000480 |
| SPL swap, standing grant | branch | 1 | 0 | 0.000000000 | 0 | 0.000000000 |
| SPL swap, exact approval: grant, swap, then (branch) close_approval after expiry | main | 2 | 4 | 0.086610240 | 4 | 0.086610240 |
| SPL swap, exact approval: grant, swap, then (branch) close_approval after expiry | branch | 3 | 1 | 0.005442720 | 0 | 0.000000000 |
| Jupiter trade, standing grant | main | 1 | 3 | 0.081000480 | 3 | 0.081000480 |
| Jupiter trade, standing grant | branch | 1 | 0 | 0.000000000 | 0 | 0.000000000 |
| Jupiter trade, exact approval: grant, trade, then (branch) close_approval after expiry | main | 2 | 4 | 0.086610240 | 4 | 0.086610240 |
| Jupiter trade, exact approval: grant, trade, then (branch) close_approval after expiry | branch | 3 | 1 | 0.005442720 | 0 | 0.000000000 |
| Pump trade, standing grant | main | 1 | 4 | 0.090862800 | 4 | 0.090862800 |
| Pump trade, standing grant | branch | 1 | 0 | 0.000000000 | 0 | 0.000000000 |
| Pump trade, exact approval: grant, buy, then (branch) close_approval after expiry | main | 2 | 5 | 0.096472560 | 5 | 0.096472560 |
| Pump trade, exact approval: grant, buy, then (branch) close_approval after expiry | branch | 3 | 1 | 0.005442720 | 0 | 0.000000000 |
| deposit: owner moves cash into the governed wallet's custody | main | 1 | 1 | 0.036867120 | 1 | 0.036867120 |
| deposit: owner moves cash into the governed wallet's custody | branch | 1 | 0 | 0.000000000 | 0 | 0.000000000 |
| deposit: owner capital contribution into existing custody | main | 1 | 1 | 0.036867120 | 1 | 0.036867120 |
| deposit: owner capital contribution into existing custody | branch | 1 | 0 | 0.000000000 | 0 | 0.000000000 |
| deposit: tokens into wallet trading custody | main | 1 | 2 | 0.041064000 | 2 | 0.041064000 |
| deposit: tokens into wallet trading custody | branch | 1 | 0 | 0.000000000 | 0 | 0.000000000 |
| withdrawal: owner withdraws free cash from custody | main | 1 | 1 | 0.036867120 | 1 | 0.036867120 |
| withdrawal: owner withdraws free cash from custody | branch | 1 | 0 | 0.000000000 | 0 | 0.000000000 |
| withdrawal: tokens out of wallet trading custody | main | 1 | 2 | 0.041064000 | 2 | 0.041064000 |
| withdrawal: tokens out of wallet trading custody | branch | 1 | 0 | 0.000000000 | 0 | 0.000000000 |

## one-time setup (principals, books, projections, custody, facility, line, budget, mandates, permissions)

Long-lived records created once per principal, mint, custody, facility, line, budget and mandate; they are not per-flow costs. Main's figure includes five capital-allocation receipts.

main (own-money): `initialize synthetic SPL token and disposable freeze authority`; `register company continuing principal`; `initialize company canonical financial book`; `initialize company zero-frontier Pool projection`; `create company owner token account`; `register issuer continuing principal`; `initialize issuer canonical financial book`; `initialize issuer zero-frontier Pool projection`; `create issuer owner token account`; `register financier continuing principal`; `initialize financier canonical financial book`; `initialize financier zero-frontier Pool projection`; `create financier owner token account`; `register supplier continuing principal`; `initialize supplier canonical financial book`; `initialize supplier zero-frontier Pool projection`; `create supplier owner token account`; `register fee continuing principal`; `initialize fee canonical financial book`; `initialize fee zero-frontier Pool projection`; `create fee owner token account`; `register protocol continuing principal`; `initialize protocol canonical financial book`; `initialize protocol zero-frontier Pool projection`; `create protocol owner token account`; `register assignee continuing principal`; `initialize assignee canonical financial book`; `initialize assignee zero-frontier Pool projection`; `create assignee owner token account`; `register sponsor continuing principal`; `initialize sponsor canonical financial book`; `initialize sponsor zero-frontier Pool projection`; `create sponsor owner token account`; `fund only synthetic company owner cash`; `fund only synthetic issuer owner cash`; `fund only synthetic assignee owner cash`; `fund only synthetic fee owner cash`; `owner initializes company allowance`; `owner creates actor 1`; `owner creates actor 2`; `create protocol custody token account`; `owner allocates protocol capital without inventing token inflow`; `create fee custody token account`; `owner allocates fee capital without inventing token inflow`; `create issuer custody token account`; `prefund issuer token account before explicit allocation`; `owner allocates issuer capital without inventing token inflow`; `buyer's owner creates and binds the buyer's own governed wallet`; `buyer's own wallet gets its token account`; `buyer's owner opens the wallet's cash record with nothing in it`; `buyer's owner moves 150,000 of the buyer's own cash into the buyer's own governed wallet`; `supplier approves immutable aggregate fee agreement main`; `ACCEPTED: the buyer opens a facility in which it is both the financier and the credit issuer`; `buyer's owner and the credit issuer's owner approve a facility: the buyer is the financier, a second principal is the credit issuer`; `buyer's owner and the credit issuer's owner open the buyer's credit line on that facility`; `buyer's owner permits agent 1: this seller, this lender facility, per-order limit 100,000, for 7,200 seconds`
- SPL token account: rent 0.001461600 SOL, still holds 0.001461600 SOL (created by `initialize synthetic SPL token and disposable freeze authority`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `initialize synthetic SPL token and disposable freeze authority`)
- principal_registry.Principal: rent 0.002143680 SOL, still holds 0.002143680 SOL (created by `register company continuing principal`)
- claims_core.FinanceBook: rent 0.003027600 SOL, still holds 0.003027600 SOL (created by `initialize company canonical financial book`)
- pool_issuer.FinancialProjection: rent 0.003250320 SOL, still holds 0.003250320 SOL (created by `initialize company zero-frontier Pool projection`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create company owner token account`)
- principal_registry.Principal: rent 0.002143680 SOL, still holds 0.002143680 SOL (created by `register issuer continuing principal`)
- claims_core.FinanceBook: rent 0.003027600 SOL, still holds 0.003027600 SOL (created by `initialize issuer canonical financial book`)
- pool_issuer.FinancialProjection: rent 0.003250320 SOL, still holds 0.003250320 SOL (created by `initialize issuer zero-frontier Pool projection`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create issuer owner token account`)
- principal_registry.Principal: rent 0.002143680 SOL, still holds 0.002143680 SOL (created by `register financier continuing principal`)
- claims_core.FinanceBook: rent 0.003027600 SOL, still holds 0.003027600 SOL (created by `initialize financier canonical financial book`)
- pool_issuer.FinancialProjection: rent 0.003250320 SOL, still holds 0.003250320 SOL (created by `initialize financier zero-frontier Pool projection`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create financier owner token account`)
- principal_registry.Principal: rent 0.002143680 SOL, still holds 0.002143680 SOL (created by `register supplier continuing principal`)
- claims_core.FinanceBook: rent 0.003027600 SOL, still holds 0.003027600 SOL (created by `initialize supplier canonical financial book`)
- pool_issuer.FinancialProjection: rent 0.003250320 SOL, still holds 0.003250320 SOL (created by `initialize supplier zero-frontier Pool projection`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create supplier owner token account`)
- principal_registry.Principal: rent 0.002143680 SOL, still holds 0.002143680 SOL (created by `register fee continuing principal`)
- claims_core.FinanceBook: rent 0.003027600 SOL, still holds 0.003027600 SOL (created by `initialize fee canonical financial book`)
- pool_issuer.FinancialProjection: rent 0.003250320 SOL, still holds 0.003250320 SOL (created by `initialize fee zero-frontier Pool projection`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create fee owner token account`)
- principal_registry.Principal: rent 0.002143680 SOL, still holds 0.002143680 SOL (created by `register protocol continuing principal`)
- claims_core.FinanceBook: rent 0.003027600 SOL, still holds 0.003027600 SOL (created by `initialize protocol canonical financial book`)
- pool_issuer.FinancialProjection: rent 0.003250320 SOL, still holds 0.003250320 SOL (created by `initialize protocol zero-frontier Pool projection`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create protocol owner token account`)
- principal_registry.Principal: rent 0.002143680 SOL, still holds 0.002143680 SOL (created by `register assignee continuing principal`)
- claims_core.FinanceBook: rent 0.003027600 SOL, still holds 0.003027600 SOL (created by `initialize assignee canonical financial book`)
- pool_issuer.FinancialProjection: rent 0.003250320 SOL, still holds 0.003250320 SOL (created by `initialize assignee zero-frontier Pool projection`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create assignee owner token account`)
- principal_registry.Principal: rent 0.002143680 SOL, still holds 0.002143680 SOL (created by `register sponsor continuing principal`)
- claims_core.FinanceBook: rent 0.003027600 SOL, still holds 0.003027600 SOL (created by `initialize sponsor canonical financial book`)
- pool_issuer.FinancialProjection: rent 0.003250320 SOL, still holds 0.003250320 SOL (created by `initialize sponsor zero-frontier Pool projection`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create sponsor owner token account`)
- principal_registry.Budget: rent 0.001635600 SOL, still holds 0.001635600 SOL (created by `owner initializes company allowance`)
- principal_registry.Actor: rent 0.001809600 SOL, still holds 0.001809600 SOL (created by `owner creates actor 1`)
- principal_registry.Actor: rent 0.001809600 SOL, still holds 0.001809600 SOL (created by `owner creates actor 2`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create protocol custody token account`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `owner allocates protocol capital without inventing token inflow`)
- claims_core.FinancierCustody: rent 0.002644800 SOL, still holds 0.002644800 SOL (created by `owner allocates protocol capital without inventing token inflow`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create fee custody token account`)
- claims_core.FinancierCustody: rent 0.002644800 SOL, still holds 0.002644800 SOL (created by `owner allocates fee capital without inventing token inflow`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `owner allocates fee capital without inventing token inflow`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create issuer custody token account`)
- claims_core.FinancierCustody: rent 0.002644800 SOL, still holds 0.002644800 SOL (created by `owner allocates issuer capital without inventing token inflow`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `owner allocates issuer capital without inventing token inflow`)
- system account: rent 0.000890880 SOL, still holds 0.000890880 SOL (created by `buyer's owner creates and binds the buyer's own governed wallet`)
- principal_registry.FundingWalletBinding: rent 0.002136720 SOL, still holds 0.002136720 SOL (created by `buyer's owner creates and binds the buyer's own governed wallet`)
- swig account (152 bytes): rent 0.001948800 SOL, still holds 0.001948800 SOL (created by `buyer's owner creates and binds the buyer's own governed wallet`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `buyer's own wallet gets its token account`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `buyer's owner opens the wallet's cash record with nothing in it`)
- claims_core.FinancierCustody: rent 0.002644800 SOL, still holds 0.002644800 SOL (created by `buyer's owner opens the wallet's cash record with nothing in it`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `buyer's owner moves 150,000 of the buyer's own cash into the buyer's own governed wallet`)
- claims_core.SellerFeeAgreement: rent 0.002470800 SOL, still holds 0.002470800 SOL (created by `supplier approves immutable aggregate fee agreement main`)
- claims_core.FundingFacility: rent 0.003918480 SOL, still holds 0.003918480 SOL (created by `ACCEPTED: the buyer opens a facility in which it is both the financier and the credit issuer`)
- claims_core.FacilityAuthority: rent 0.001691280 SOL, still holds 0.001691280 SOL (created by `ACCEPTED: the buyer opens a facility in which it is both the financier and the credit issuer`)
- claims_core.FacilityAuthority: rent 0.001691280 SOL, still holds 0.001691280 SOL (created by `buyer's owner and the credit issuer's owner approve a facility: the buyer is the financier, a second principal is the credit issuer`)
- claims_core.FundingFacility: rent 0.003918480 SOL, still holds 0.003918480 SOL (created by `buyer's owner and the credit issuer's owner approve a facility: the buyer is the financier, a second principal is the credit issuer`)
- claims_core.CustomerCreditLine: rent 0.003521760 SOL, still holds 0.003521760 SOL (created by `buyer's owner and the credit issuer's owner open the buyer's credit line on that facility`)
- principal_registry.Mandate: rent 0.008338080 SOL, still holds 0.008338080 SOL (created by `buyer's owner permits agent 1: this seller, this lender facility, per-order limit 100,000, for 7,200 seconds`)

branch (own-money): `initialize synthetic SPL token and disposable freeze authority`; `register company continuing principal`; `initialize company canonical financial book`; `initialize company zero-frontier Pool projection`; `create company owner token account`; `register issuer continuing principal`; `initialize issuer canonical financial book`; `initialize issuer zero-frontier Pool projection`; `create issuer owner token account`; `register financier continuing principal`; `initialize financier canonical financial book`; `initialize financier zero-frontier Pool projection`; `create financier owner token account`; `register supplier continuing principal`; `initialize supplier canonical financial book`; `initialize supplier zero-frontier Pool projection`; `create supplier owner token account`; `register fee continuing principal`; `initialize fee canonical financial book`; `initialize fee zero-frontier Pool projection`; `create fee owner token account`; `register protocol continuing principal`; `initialize protocol canonical financial book`; `initialize protocol zero-frontier Pool projection`; `create protocol owner token account`; `register assignee continuing principal`; `initialize assignee canonical financial book`; `initialize assignee zero-frontier Pool projection`; `create assignee owner token account`; `register sponsor continuing principal`; `initialize sponsor canonical financial book`; `initialize sponsor zero-frontier Pool projection`; `create sponsor owner token account`; `fund only synthetic company owner cash`; `fund only synthetic issuer owner cash`; `fund only synthetic assignee owner cash`; `fund only synthetic fee owner cash`; `owner initializes company allowance`; `owner creates actor 1`; `owner creates actor 2`; `create protocol custody token account`; `owner allocates protocol capital without inventing token inflow`; `create fee custody token account`; `owner allocates fee capital without inventing token inflow`; `create issuer custody token account`; `prefund issuer token account before explicit allocation`; `owner allocates issuer capital without inventing token inflow`; `buyer's owner creates and binds the buyer's own governed wallet`; `buyer's own wallet gets its token account`; `buyer's owner opens the wallet's cash record with nothing in it`; `buyer's owner moves 150,000 of the buyer's own cash into the buyer's own governed wallet`; `supplier approves immutable aggregate fee agreement main`; `ACCEPTED: the buyer opens a facility in which it is both the financier and the credit issuer`; `buyer's owner and the credit issuer's owner approve a facility: the buyer is the financier, a second principal is the credit issuer`; `buyer's owner and the credit issuer's owner open the buyer's credit line on that facility`; `buyer's owner permits agent 1: this seller, this lender facility, per-order limit 100,000, for 7,200 seconds`
- SPL token account: rent 0.001461600 SOL, still holds 0.001461600 SOL (created by `initialize synthetic SPL token and disposable freeze authority`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `initialize synthetic SPL token and disposable freeze authority`)
- principal_registry.Principal: rent 0.002422080 SOL, still holds 0.002422080 SOL (created by `register company continuing principal`)
- claims_core.FinanceBook: rent 0.004196880 SOL, still holds 0.004196880 SOL (created by `initialize company canonical financial book`)
- pool_issuer.FinancialProjection: rent 0.003473040 SOL, still holds 0.003473040 SOL (created by `initialize company zero-frontier Pool projection`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create company owner token account`)
- principal_registry.Principal: rent 0.002422080 SOL, still holds 0.002422080 SOL (created by `register issuer continuing principal`)
- claims_core.FinanceBook: rent 0.004196880 SOL, still holds 0.004196880 SOL (created by `initialize issuer canonical financial book`)
- pool_issuer.FinancialProjection: rent 0.003473040 SOL, still holds 0.003473040 SOL (created by `initialize issuer zero-frontier Pool projection`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create issuer owner token account`)
- principal_registry.Principal: rent 0.002422080 SOL, still holds 0.002422080 SOL (created by `register financier continuing principal`)
- claims_core.FinanceBook: rent 0.004196880 SOL, still holds 0.004196880 SOL (created by `initialize financier canonical financial book`)
- pool_issuer.FinancialProjection: rent 0.003473040 SOL, still holds 0.003473040 SOL (created by `initialize financier zero-frontier Pool projection`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create financier owner token account`)
- principal_registry.Principal: rent 0.002422080 SOL, still holds 0.002422080 SOL (created by `register supplier continuing principal`)
- claims_core.FinanceBook: rent 0.004196880 SOL, still holds 0.004196880 SOL (created by `initialize supplier canonical financial book`)
- pool_issuer.FinancialProjection: rent 0.003473040 SOL, still holds 0.003473040 SOL (created by `initialize supplier zero-frontier Pool projection`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create supplier owner token account`)
- principal_registry.Principal: rent 0.002422080 SOL, still holds 0.002422080 SOL (created by `register fee continuing principal`)
- claims_core.FinanceBook: rent 0.004196880 SOL, still holds 0.004196880 SOL (created by `initialize fee canonical financial book`)
- pool_issuer.FinancialProjection: rent 0.003473040 SOL, still holds 0.003473040 SOL (created by `initialize fee zero-frontier Pool projection`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create fee owner token account`)
- principal_registry.Principal: rent 0.002422080 SOL, still holds 0.002422080 SOL (created by `register protocol continuing principal`)
- claims_core.FinanceBook: rent 0.004196880 SOL, still holds 0.004196880 SOL (created by `initialize protocol canonical financial book`)
- pool_issuer.FinancialProjection: rent 0.003473040 SOL, still holds 0.003473040 SOL (created by `initialize protocol zero-frontier Pool projection`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create protocol owner token account`)
- principal_registry.Principal: rent 0.002422080 SOL, still holds 0.002422080 SOL (created by `register assignee continuing principal`)
- claims_core.FinanceBook: rent 0.004196880 SOL, still holds 0.004196880 SOL (created by `initialize assignee canonical financial book`)
- pool_issuer.FinancialProjection: rent 0.003473040 SOL, still holds 0.003473040 SOL (created by `initialize assignee zero-frontier Pool projection`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create assignee owner token account`)
- principal_registry.Principal: rent 0.002422080 SOL, still holds 0.002422080 SOL (created by `register sponsor continuing principal`)
- claims_core.FinanceBook: rent 0.004196880 SOL, still holds 0.004196880 SOL (created by `initialize sponsor canonical financial book`)
- pool_issuer.FinancialProjection: rent 0.003473040 SOL, still holds 0.003473040 SOL (created by `initialize sponsor zero-frontier Pool projection`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create sponsor owner token account`)
- principal_registry.Budget: rent 0.001858320 SOL, still holds 0.001858320 SOL (created by `owner initializes company allowance`)
- principal_registry.Actor: rent 0.002032320 SOL, still holds 0.002032320 SOL (created by `owner creates actor 1`)
- principal_registry.Actor: rent 0.002032320 SOL, still holds 0.002032320 SOL (created by `owner creates actor 2`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create protocol custody token account`)
- claims_core.FinancierCustody: rent 0.004036800 SOL, still holds 0.004036800 SOL (created by `owner allocates protocol capital without inventing token inflow`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create fee custody token account`)
- claims_core.FinancierCustody: rent 0.004036800 SOL, still holds 0.004036800 SOL (created by `owner allocates fee capital without inventing token inflow`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `create issuer custody token account`)
- claims_core.FinancierCustody: rent 0.004036800 SOL, still holds 0.004036800 SOL (created by `owner allocates issuer capital without inventing token inflow`)
- system account: rent 0.000890880 SOL, still holds 0.000890880 SOL (created by `buyer's owner creates and binds the buyer's own governed wallet`)
- principal_registry.FundingWalletBinding: rent 0.002359440 SOL, still holds 0.002359440 SOL (created by `buyer's owner creates and binds the buyer's own governed wallet`)
- swig account (152 bytes): rent 0.001948800 SOL, still holds 0.001948800 SOL (created by `buyer's owner creates and binds the buyer's own governed wallet`)
- SPL token account: rent 0.002039280 SOL, still holds 0.002039280 SOL (created by `buyer's own wallet gets its token account`)
- claims_core.FinancierCustody: rent 0.004036800 SOL, still holds 0.004036800 SOL (created by `buyer's owner opens the wallet's cash record with nothing in it`)
- claims_core.SellerFeeAgreement: rent 0.002693520 SOL, still holds 0.002693520 SOL (created by `supplier approves immutable aggregate fee agreement main`)
- claims_core.FundingFacility: rent 0.004141200 SOL, still holds 0.004141200 SOL (created by `ACCEPTED: the buyer opens a facility in which it is both the financier and the credit issuer`)
- claims_core.FacilityAuthority: rent 0.001914000 SOL, still holds 0.001914000 SOL (created by `ACCEPTED: the buyer opens a facility in which it is both the financier and the credit issuer`)
- claims_core.FacilityAuthority: rent 0.001914000 SOL, still holds 0.001914000 SOL (created by `buyer's owner and the credit issuer's owner approve a facility: the buyer is the financier, a second principal is the credit issuer`)
- claims_core.FundingFacility: rent 0.004141200 SOL, still holds 0.004141200 SOL (created by `buyer's owner and the credit issuer's owner approve a facility: the buyer is the financier, a second principal is the credit issuer`)
- claims_core.CustomerCreditLine: rent 0.003744480 SOL, still holds 0.003744480 SOL (created by `buyer's owner and the credit issuer's owner open the buyer's credit line on that facility`)
- principal_registry.Mandate: rent 0.009813600 SOL, still holds 0.009813600 SOL (created by `buyer's owner permits agent 1: this seller, this lender facility, per-order limit 100,000, for 7,200 seconds`)

## order, commit to close: cash repayment settles it, closes appended to the settling repayment

Branch: the Tab, hold and run close in the settling repayment and each deposit returns to the payer recorded at creation; the repayment record is created and closed inside that same transaction, so it nets to zero and is not listed. Main keeps every record.

main (own-money): `ACCEPTED: the buyer's agent orders 100,000 from the seller, backed by the buyer's own wallet`; `ACCEPTED: the seller reports 100,000 of completed work`; `ACCEPTED: an outside key collects the 100,000 from the buyer's wallet to the seller`; `ACCEPTED: clearing step 1 of 2, the buyer's owner pays 100,000 from the buyer's own token account to the credit issuer`
- principal_registry.OperationReceipt: rent 0.002143680 SOL, still holds 0.002143680 SOL (created by `ACCEPTED: the buyer's agent orders 100,000 from the seller, backed by the buyer's own wallet`)
- claims_core.FundedClaim: rent 0.009688320 SOL, still holds 0.009688320 SOL (created by `ACCEPTED: the buyer's agent orders 100,000 from the seller, backed by the buyer's own wallet`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `ACCEPTED: the buyer's agent orders 100,000 from the seller, backed by the buyer's own wallet`)
- principal_registry.Reservation: rent 0.003807120 SOL, still holds 0.003807120 SOL (created by `ACCEPTED: the buyer's agent orders 100,000 from the seller, backed by the buyer's own wallet`)
- claims_core.FundedRun: rent 0.005964720 SOL, still holds 0.005964720 SOL (created by `ACCEPTED: the seller reports 100,000 of completed work`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `ACCEPTED: the seller reports 100,000 of completed work`)
- claims_core.FundedPayment: rent 0.002081040 SOL, still holds 0.002081040 SOL (created by `ACCEPTED: the seller reports 100,000 of completed work`)
- principal_registry.OperationReceipt: rent 0.002143680 SOL, still holds 0.002143680 SOL (created by `ACCEPTED: the seller reports 100,000 of completed work`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `ACCEPTED: an outside key collects the 100,000 from the buyer's wallet to the seller`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `ACCEPTED: clearing step 1 of 2, the buyer's owner pays 100,000 from the buyer's own token account to the credit issuer`)

branch (own-money): `ACCEPTED: the buyer's agent orders 100,000 from the seller, backed by the buyer's own wallet`; `ACCEPTED: the seller reports 100,000 of completed work`; `ACCEPTED: an outside key collects the 100,000 from the buyer's wallet to the seller`; `ACCEPTED: clearing step 1 of 2, the buyer's owner pays 100,000 from the buyer's own token account to the credit issuer`
- principal_registry.Reservation: rent 0.002136720 SOL, closed, 0 locked (created by `ACCEPTED: the buyer's agent orders 100,000 from the seller, backed by the buyer's own wallet`)
- claims_core.FundedClaim: rent 0.010349520 SOL, closed, 0 locked (created by `ACCEPTED: the buyer's agent orders 100,000 from the seller, backed by the buyer's own wallet`)
- claims_core.FundedRun: rent 0.004405680 SOL, closed, 0 locked (created by `ACCEPTED: the seller reports 100,000 of completed work`)

## order, commit to close: Swig-governed final payment, keeper sends the closes separately (Tab records; its exact repayment approvals are the next rows)

Branch: the keeper's separate close transaction returns the Tab, hold, run and both repayment records to their recorded payers. The exact repayment approvals that authorized the governed payments are separate accounts, measured in the next row.

main (financing-governed-repayment-run): `accept finite supplier commitment governed-company`; `supplier attests completed run governed-delivery`; `independent supplier collection of earned right`; `individual owner approval repays delivered principal through native Swig`; `lost response recovery reuses original economic payment`; `designated executor pays without another owner signature`; `historical standing result ignores minimum interval`; `second eligible payment consumes bounded standing allowance`; `remaining title can be assigned after governed payments`; `original payment survives creditor assignment owner rotation and revocation`; `replacement owner can approve the continuing wallet and current creditor`; `immutable receipt recovers after its permission expires`
- principal_registry.OperationReceipt: rent 0.002143680 SOL, still holds 0.002143680 SOL (created by `accept finite supplier commitment governed-company`)
- principal_registry.Reservation: rent 0.003807120 SOL, still holds 0.003807120 SOL (created by `accept finite supplier commitment governed-company`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `accept finite supplier commitment governed-company`)
- claims_core.FundedClaim: rent 0.009688320 SOL, still holds 0.009688320 SOL (created by `accept finite supplier commitment governed-company`)
- claims_core.FundedPayment: rent 0.002081040 SOL, still holds 0.002081040 SOL (created by `supplier attests completed run governed-delivery`)
- principal_registry.OperationReceipt: rent 0.002143680 SOL, still holds 0.002143680 SOL (created by `supplier attests completed run governed-delivery`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `supplier attests completed run governed-delivery`)
- claims_core.FundedRun: rent 0.005964720 SOL, still holds 0.005964720 SOL (created by `supplier attests completed run governed-delivery`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `independent supplier collection of earned right`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `individual owner approval repays delivered principal through native Swig`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `designated executor pays without another owner signature`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `second eligible payment consumes bounded standing allowance`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `remaining title can be assigned after governed payments`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `replacement owner can approve the continuing wallet and current creditor`)

branch (financing-governed-repayment-run): `accept finite supplier commitment governed-company`; `supplier attests completed run governed-delivery`; `independent supplier collection of earned right`; `individual owner approval repays delivered principal through native Swig`; `designated executor pays without another owner signature`; `second eligible payment consumes bounded standing allowance`; `remaining title can be assigned after governed payments`; `replacement owner can approve the continuing wallet and current creditor`; `release unused accepted work governed-unused-capacity`; `replacement owner's exact approval pays the last amount the run owes`; `keeper closes the governed-settled Tab in a separate transaction`
- claims_core.FundedClaim: rent 0.010349520 SOL, closed, 0 locked (created by `accept finite supplier commitment governed-company`)
- principal_registry.Reservation: rent 0.002136720 SOL, closed, 0 locked (created by `accept finite supplier commitment governed-company`)
- claims_core.FundedRun: rent 0.004405680 SOL, closed, 0 locked (created by `supplier attests completed run governed-delivery`)
- claims_core.RepaymentRecord: rent 0.003807120 SOL, closed, 0 locked (created by `individual owner approval repays delivered principal through native Swig`)
- claims_core.RepaymentRecord: rent 0.003807120 SOL, closed, 0 locked (created by `replacement owner can approve the continuing wallet and current creditor`)

## exact repayment approval: grant, then close_approval after expiry (the payment it authorizes is measured with its order)

Branch: double-charge.py grants approval a (its grant; 5,547,120 lamports paid by its grantor), uses it for one payment, and after expiry an outside key closes it : exactly 5,547,120 lamports go back to the grantor, the closer pays only the fee. Main's exact repayment permission is a RepaymentPermission account with no close. In financing_governed_repayment.py the branch's three exact repayment approvals stay open because that suite never runs close_approval.

main (financing-governed-repayment-run): `owner grants exact individual repayment permission individual-permission`
- principal_registry.RepaymentPermission: rent 0.005714160 SOL, still holds 0.005714160 SOL (created by `owner grants exact individual repayment permission individual-permission`)

branch (double-charge): `owner grants exact individual repayment permission approval-a`; `anyone closes used approval a after its expiry`
- principal_registry.ExactRepaymentApproval: rent 0.005547120 SOL, closed, 0 locked (created by `owner grants exact individual repayment permission approval-a`)

## evidence order, commit to close: no records, finalized at zero

main (evidence-billing): `accept bounded authenticated work no-records`; `independent caller establishes deterministic bill then releases residual no-records`
- principal_registry.Reservation: rent 0.003807120 SOL, still holds 0.003807120 SOL (created by `accept bounded authenticated work no-records`)
- claims_core.FundedClaim: rent 0.009688320 SOL, still holds 0.009688320 SOL (created by `accept bounded authenticated work no-records`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `accept bounded authenticated work no-records`)
- principal_registry.OperationReceipt: rent 0.002143680 SOL, still holds 0.002143680 SOL (created by `accept bounded authenticated work no-records`)
- claims_core.EvidenceBill: rent 0.009806640 SOL, still holds 0.009806640 SOL (created by `accept bounded authenticated work no-records`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `independent caller establishes deterministic bill then releases residual no-records`)
- claims_core.FundedPayment: rent 0.002081040 SOL, still holds 0.002081040 SOL (created by `independent caller establishes deterministic bill then releases residual no-records`)
- claims_core.FundedRun: rent 0.005964720 SOL, still holds 0.005964720 SOL (created by `independent caller establishes deterministic bill then releases residual no-records`)
- principal_registry.OperationReceipt: rent 0.002143680 SOL, still holds 0.002143680 SOL (created by `independent caller establishes deterministic bill then releases residual no-records`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `independent caller establishes deterministic bill then releases residual no-records`)

branch (evidence-billing): `accept bounded authenticated work no-records`; `independent caller establishes deterministic bill then releases residual no-records`; `Change 5: an outside key closes settled order no-records`
- claims_core.FundedClaim: rent 0.010349520 SOL, closed, 0 locked (created by `accept bounded authenticated work no-records`)
- claims_core.EvidenceBill: rent 0.009361200 SOL, closed, 0 locked (created by `accept bounded authenticated work no-records`)
- principal_registry.Reservation: rent 0.002136720 SOL, closed, 0 locked (created by `accept bounded authenticated work no-records`)

## wallet send, standing grant

main (wallet-spending-send): `bounded executor sends the first permitted amount`
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `bounded executor sends the first permitted amount`)
- claims_core.WalletSpendReceipt: rent 0.007266240 SOL, still holds 0.007266240 SOL (created by `bounded executor sends the first permitted amount`)

branch (wallet-spending-send): `bounded executor sends the first permitted amount`

## wallet send, exact approval: grant, send, then (branch) close_approval after expiry

Branch: wallet_double_charge.py rows 94, 95 and 100. The grant's payer (9LbZUY...) pays 5,442,720 lamports and gets exactly 5,442,720 back from close_approval after expiry; the closer pays only the fee. Main keeps the SpendingPermission and both receipts.

main (wallet-spending-send): `owner grants exact spending permission exact-send-approval`; `explicit owner approval transfers exactly the approved asset and amount`
- principal_registry.SpendingPermission: rent 0.005609760 SOL, still holds 0.005609760 SOL (created by `owner grants exact spending permission exact-send-approval`)
- claims_core.WalletSpendReceipt: rent 0.007266240 SOL, still holds 0.007266240 SOL (created by `explicit owner approval transfers exactly the approved asset and amount`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `explicit owner approval transfers exactly the approved asset and amount`)

branch (wallet-double-charge): `owner grants exact spending permission double-charge-close-used`; `close_approval: the used approval's trade lands`; `close_approval: the expired used approval closes`
- principal_registry.ExactSpendingApproval: rent 0.005442720 SOL, closed, 0 locked (created by `owner grants exact spending permission double-charge-close-used`)

## exact spending approval never used: grant, then close_approval after expiry

Branch only (main has no close): an approval that was never used also closes after expiry, rent back to its payer.

branch (wallet-double-charge): `owner grants exact spending permission double-charge-close-unused`; `close_approval: the expired unused approval closes`
- principal_registry.ExactSpendingApproval: rent 0.005442720 SOL, closed, 0 locked (created by `owner grants exact spending permission double-charge-close-unused`)

## SPL swap, standing grant

main (wallet-spending-swap): `bounded delegated swap commits the actual debit and output`
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `bounded delegated swap commits the actual debit and output`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `bounded delegated swap commits the actual debit and output`)
- claims_core.WalletSpendReceipt: rent 0.007266240 SOL, still holds 0.007266240 SOL (created by `bounded delegated swap commits the actual debit and output`)

branch (wallet-spending-swap): `bounded delegated swap commits the actual debit and output`

## SPL swap, exact approval: grant, swap, then (branch) close_approval after expiry

Branch: wallet_spending.py now closes its exact approvals after expiry; the ExactSpendingApproval's rent goes back to its recorded payer. Main keeps the SpendingPermission and the receipts.

main (wallet-spending-swap): `owner grants exact spending permission exact-swap-approval`; `owner-approved exact-input swap executes the actual source-built adapter`
- principal_registry.SpendingPermission: rent 0.005609760 SOL, still holds 0.005609760 SOL (created by `owner grants exact spending permission exact-swap-approval`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `owner-approved exact-input swap executes the actual source-built adapter`)
- claims_core.WalletSpendReceipt: rent 0.007266240 SOL, still holds 0.007266240 SOL (created by `owner-approved exact-input swap executes the actual source-built adapter`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `owner-approved exact-input swap executes the actual source-built adapter`)

branch (wallet-spending-swap): `owner grants exact spending permission exact-swap-approval`; `owner-approved exact-input swap executes the actual source-built adapter`; `close_approval: expired exact approval exact-swap-approval closes`
- principal_registry.ExactSpendingApproval: rent 0.005442720 SOL, closed, 0 locked (created by `owner grants exact spending permission exact-swap-approval`)

## Jupiter trade, standing grant

main (wallet-jupiter-public): `actual recorded public public-pump-direct route under governed wallet authority`
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `actual recorded public public-pump-direct route under governed wallet authority`)
- claims_core.WalletSpendReceipt: rent 0.007266240 SOL, still holds 0.007266240 SOL (created by `actual recorded public public-pump-direct route under governed wallet authority`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `actual recorded public public-pump-direct route under governed wallet authority`)

branch (wallet-jupiter-public): `actual recorded public public-pump-direct route under governed wallet authority`

## Jupiter trade, exact approval: grant, trade, then (branch) close_approval after expiry

Branch: wallet_jupiter.py now closes its exact approvals after expiry; the approval's rent goes back to its recorded payer.

main (wallet-jupiter-public): `owner grants exact spending permission public-route-0`; `actual recorded public public-legacy route under governed wallet authority`
- principal_registry.SpendingPermission: rent 0.005609760 SOL, still holds 0.005609760 SOL (created by `owner grants exact spending permission public-route-0`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `actual recorded public public-legacy route under governed wallet authority`)
- claims_core.WalletSpendReceipt: rent 0.007266240 SOL, still holds 0.007266240 SOL (created by `actual recorded public public-legacy route under governed wallet authority`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `actual recorded public public-legacy route under governed wallet authority`)

branch (wallet-jupiter-public): `owner grants exact spending permission public-route-0`; `actual recorded public public-legacy route under governed wallet authority`; `close_approval: expired exact approval public-route-0 closes`
- principal_registry.ExactSpendingApproval: rent 0.005442720 SOL, closed, 0 locked (created by `owner grants exact spending permission public-route-0`)

## Pump trade, standing grant

main (wallet-pump-curve): `bounded standing permission admits first launch buy`
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `bounded standing permission admits first launch buy`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `bounded standing permission admits first launch buy`)
- claims_core.WalletSpendReceipt: rent 0.007266240 SOL, still holds 0.007266240 SOL (created by `bounded standing permission admits first launch buy`)
- claims_core.WalletTradingDetails: rent 0.009862320 SOL, still holds 0.009862320 SOL (created by `bounded standing permission admits first launch buy`)

branch (wallet-pump-curve): `bounded standing permission admits first launch buy`

## Pump trade, exact approval: grant, buy, then (branch) close_approval after expiry

Branch: wallet_pump.py now closes its exact approvals after expiry; the approval's rent goes back to its recorded payer.

main (wallet-pump-curve): `owner grants exact spending permission pump-exact-buy`; `actual governed Pump launch buy unwraps only accounted WSOL`
- principal_registry.SpendingPermission: rent 0.005609760 SOL, still holds 0.005609760 SOL (created by `owner grants exact spending permission pump-exact-buy`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `actual governed Pump launch buy unwraps only accounted WSOL`)
- claims_core.WalletTradingDetails: rent 0.009862320 SOL, still holds 0.009862320 SOL (created by `actual governed Pump launch buy unwraps only accounted WSOL`)
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `actual governed Pump launch buy unwraps only accounted WSOL`)
- claims_core.WalletSpendReceipt: rent 0.007266240 SOL, still holds 0.007266240 SOL (created by `actual governed Pump launch buy unwraps only accounted WSOL`)

branch (wallet-pump-curve): `owner grants exact spending permission pump-exact-buy`; `actual governed Pump launch buy unwraps only accounted WSOL`; `close_approval: expired exact approval pump-exact-buy closes`
- principal_registry.ExactSpendingApproval: rent 0.005442720 SOL, closed, 0 locked (created by `owner grants exact spending permission pump-exact-buy`)

## deposit: owner moves cash into the governed wallet's custody

main (own-money): `buyer's owner moves 150,000 of the buyer's own cash into the buyer's own governed wallet`
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `buyer's owner moves 150,000 of the buyer's own cash into the buyer's own governed wallet`)

branch (own-money): `buyer's owner moves 150,000 of the buyer's own cash into the buyer's own governed wallet`

## deposit: owner capital contribution into existing custody

main (financing-commercial): `owner contribution records actual token inflow and allocated own capital`
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `owner contribution records actual token inflow and allocated own capital`)

branch (financing-commercial): `owner contribution records actual token inflow and allocated own capital`

## deposit: tokens into wallet trading custody

main (wallet-token-lifecycle): `deposit exact gross/net wallet tokens`
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `deposit exact gross/net wallet tokens`)
- claims_core.WalletTokenTransferReceipt: rent 0.004196880 SOL, still holds 0.004196880 SOL (created by `deposit exact gross/net wallet tokens`)

branch (wallet-token-lifecycle): `deposit exact gross/net wallet tokens`

## withdrawal: owner withdraws free cash from custody

main (own-money): `ACCEPTED: buyer's owner withdraws exactly the free 50,000 from the buyer's wallet`
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `ACCEPTED: buyer's owner withdraws exactly the free 50,000 from the buyer's wallet`)

branch (own-money): `ACCEPTED: buyer's owner withdraws exactly the free 50,000 from the buyer's wallet`

## withdrawal: tokens out of wallet trading custody

main (wallet-token-lifecycle): `withdraw exact gross/net wallet tokens`
- claims_core.FinanceReceipt: rent 0.036867120 SOL, still holds 0.036867120 SOL (created by `withdraw exact gross/net wallet tokens`)
- claims_core.WalletTokenTransferReceipt: rent 0.004196880 SOL, still holds 0.004196880 SOL (created by `withdraw exact gross/net wallet tokens`)

branch (wallet-token-lifecycle): `withdraw exact gross/net wallet tokens`

