<p align="center">
  <img src="https://raw.githubusercontent.com/Dexter-DAO/dexter-x402-sdk/main/assets/dexter-wordmark.svg" alt="Dexter" width="360">
</p>

<h1 align="center">Dexter Article Evidence</h1>

<p align="center">
  <strong>The records, program source and checks behind Dexter's accounts.</strong>
</p>

<p align="center">
  <a href="#what-the-records-show"><img src="https://img.shields.io/badge/access-public-0f766e" alt="Access: public"></a>
  <a href="#evidence-library"><img src="https://img.shields.io/badge/source-pinned-334155" alt="Source: pinned"></a>
  <a href="#check-them-yourself"><img src="https://img.shields.io/badge/checks-offline-7c3aed" alt="Checks: offline"></a>
</p>

<p align="center">
  <a href="#what-the-records-show">Records</a> &middot;
  <a href="#articles">Articles</a> &middot;
  <a href="#evidence-library">Evidence library</a> &middot;
  <a href="#check-them-yourself">Get the evidence</a> &middot;
  <a href="https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/articles/index.json">Machine-readable catalog</a>
</p>

---

Dexter Labs Corporation builds accounts that separate permission from possession. An agent holds a key inside limits the owner approved, and the money stays in the wallet. Three checks meet at the program in one transaction: the owner's permission, the money behind the order, and the seller and terms. Earned payment collects with no new signature from the buyer, the agent, the lender or Dexter. Obligations stay with the account when an agent or operator changes. The earlier Account records, from November 1 and 5, call the seller the supplier and the lender the financier.

The [company reference](https://dexter.cash/llms.txt) explains the problem and the design. This repository holds the proof.

## What the records show

| Result | Where | Read it |
|---|---|---|
| One transaction checks the owner's permission, the money behind the order, and the seller and the exact terms, and it refuses the order when any one is missing. The commit accepted an order with a maximum of 100,000 units and all three present, and it refused 23 attempts that each lacked one. A key with no role in the order then collected the seller's earned 60,000, and eight altered versions of that collection were refused. The run used the compiled Account programs and a synthetic token. | Local&nbsp;SVM<br>Nov&nbsp;18,&nbsp;2026 | [Guide and tables](releases/2026-11-18/README.md), [case index](releases/2026-11-18/case-index.json), [trace](releases/2026-11-18/commit-legs-trace.json.xz) |
| One reserve backs many orders. Two agents drew on one budget of 150,000 and two sellers were backed from one lender wallet holding 150,000. The second order was refused when it would overdraw the budget or the wallet, and accepted at the amount that fit. Each seller was then paid by an outside key from the shared wallet while the other's share stayed untouched, and the lender could withdraw unused backing only after the seller released it. The run used the compiled Account programs and a synthetic token. | Local&nbsp;SVM<br>Nov&nbsp;18,&nbsp;2026 | [Guide and tables](releases/2026-11-18/shared-reserve/README.md), [case index](releases/2026-11-18/shared-reserve/case-index.json), [budget trace](releases/2026-11-18/shared-reserve/shared-reserve-budget-trace.json.xz), [wallet trace](releases/2026-11-18/shared-reserve/shared-reserve-wallet-trace.json.xz) |
| An order can name who signs for delivery. The seller was paid 900 when the named signer signed for 900 against a claim of 1,400, and nothing when the named signer never signed. The seller's own report of delivery was refused on that order, and so was a report signed by any key the order did not name. One order named the buyer as the signer. The run used the compiled Account programs and a synthetic token. | Local&nbsp;SVM<br>Nov&nbsp;18,&nbsp;2026 | [Guide and tables](releases/2026-11-18/named-signer/README.md), [case index](releases/2026-11-18/named-signer/case-index.json), [trace](releases/2026-11-18/named-signer/named-signer-trace.json.xz), [source](releases/2026-11-18/named-signer/README.md#the-rule-in-the-source) |
| Dexter signed a payout to the wrong recipient with its own payment key. The program refused it, and the approved store collected the same claim. The case also records refusals of a copycat charge, an amount above the remaining limit and a charge after expiry. Dexter operated the demonstration stores. | Solana&nbsp;mainnet<br>Oct&nbsp;30,&nbsp;2026 | [Case record](https://dexter.cash/mandate?case=b0ee0521-898e-828b-9c14-265bfb59216c), [transactions](excerpts/records.md#the-oct-30-mainnet-record), [signed record](evidence/historical-evidence/record.json), [current verifier](releases/2026-11-18/verifier/README.md) |
| An owner's dollars moved into a yield-bearing token and back, twice, without leaving the owner's wallet. Each trade carried the owner's passkey signature and the Vault program checked the balances in the same transaction. The program refuses a purchase that would leave less stablecoin than the wallet owes. | Solana&nbsp;mainnet<br>July&nbsp;18,&nbsp;2026 | [Guide and transactions](releases/2026-11-17/README.md), [on-chain readback](releases/2026-11-17/mainnet-transactions.json) |
| A separate signer collected the supplier's earned payment after the owner revoked the agent and the financier cooperatively rotated its owner key. The payable fell from 100,000 token atoms to zero. Unused backing stayed at 200,000 and buyer debt stayed at 100,000. The run used the compiled Account programs and synthetic assets. | Local&nbsp;SVM<br>Nov&nbsp;1,&nbsp;2026 | [Decoded sequence](excerpts/records.md#the-decoded-account-sequence), [15 recorded scenarios](excerpts/records.md#fifteen-recorded-scenarios), [case index](evidence/account-evidence/account/docs/evidence/wallet-spending-20261101/case-index.json) |
| After the initiating actor's revocation, a supplier claimed 1,400 units. The work record supported 900. A separate signer collected 900 for the supplier, and the buyer still owed the financier 900. The SDK/RPC run used synthetic work records. | Local&nbsp;validator<br>Nov&nbsp;5,&nbsp;2026 | [Billing record](excerpts/records.md#billing-after-revocation) |
| The supplier recovered its account between two collections. The first 400 units and the remaining 500 reached the same custody. Two of three enrolled guardians authorized the new key on compiled Account programs. | Local&nbsp;SVM<br>Nov&nbsp;5,&nbsp;2026 | [Recovery record](excerpts/records.md#guardian-recovery-between-collections), [guardian policy and original trace](releases/2026-11-06/recovery/README.md) |
| The API, PostgreSQL, SDK and native programs carried a purchase from admission through claim, finalization and supplier payment. Twenty-six fresh processes recovered the recorded outcomes without a new payment or funding authorization. This run used Agave and controlled provider identities. | Local&nbsp;validator<br>Nov&nbsp;5,&nbsp;2026 | [Purchase record](excerpts/records.md#one-purchase-from-admission-to-collection) |

## What the programs refused

The [November 18 commit record](releases/2026-11-18/README.md) sends one order to the commit, ClaimsCore `accept_financed_tab`, with one fact changed in each case. Its [case index](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/releases/2026-11-18/case-index.json) names all 117 transactions by index. These rows are refusals of the commit:

- commit-legs 67, "Q1 refused: order of 100,001 against a per-order limit of 100,000", PrincipalRegistry `PerOperationLimit`, error 6011.
- commit-legs 70, "Q1 refused: order 1 (100,000) against a remaining budget of 99,999", PrincipalRegistry `BudgetExhausted`, error 6012.
- commit-legs 110, "Q1 refused: the revoked agent signs order 4 (100,000)", PrincipalRegistry `CredentialInactive`, error 6004.
- commit-legs 73, "Q2 refused: order 1 (100,000) against a lender facility limit of 99,999", ClaimsCore `FundingFacility`, error 6025.
- commit-legs 81, "Q2 refused: order 1 (100,000) against lender free cash of 99,999", ClaimsCore `FundingCashShortfall`, error 6033.
- commit-legs 57, "Q3 refused: a key that is not the seller signs order 1 in the seller's place", ClaimsCore `FundingOwner`, error 6023.
- commit-legs 60, "Q3 refused: order 1 says 100,000 but the seller-side terms say 99,999", ClaimsCore `FundingTerms`, error 6031.
- commit-legs 65, "Q1+Q3 refused: a different registered seller signs order 1; the owner's permission names the first seller", PrincipalRegistry `IntentMismatch`, error 6010.

Row 85 is order 1 with nothing changed, and the commit accepts it. The release lists all 23 refusals in three tables: permission (Q1), money (Q2) and the deal (Q3).

Across the 15 Account scenarios recorded on November 1, the checks covered 1,406 signed packets, 272 financial receipts, 115 refusals with rollback and 5,113 recorded-state assertions. Twelve of those refusals are [listed by row](excerpts/records.md#twelve-refused-wallet-operations): eight sends that PrincipalRegistry refused while authorizing the wallet, and a withdrawal, two sends and a swap that ClaimsCore refused because each would leave the wallet below its committed backing.

The rules behind the November 1 refusals are in the [program source](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/evidence/account-evidence/execution/foundation/onchain/programs), at the execution pin of those records. The [excerpts page](excerpts/account-programs.md) quotes the lines for order acceptance, the network fee and corrections. Then read [the reserve](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/execution/foundation/onchain/programs/claims-core/src/funding_state.rs) and [collection](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/execution/foundation/onchain/programs/claims-core/src/funding_collection.rs). The November 18 records ran a later commit of the same programs, and their guides name that commit and the hash of each binary.

## Articles

| Article | Evidence | Package |
|---|---|---|
| **[Turns Out the Impasse Was a Tech Problem](articles/the-account-inversion/)**<br>Agentic Commerce Is at an Impasse Because Account Holders Are Not Account Owners. That Problem Is Now Solved. | [Claims, records and checks](articles/the-account-inversion/README.md) | [November 18 ZIP](https://github.com/Dexter-DAO/dexter-article-evidence/releases/download/2026-11-18/dexter-article-evidence-2026-11-18.zip)<br>All records except the named-signer record |

## Evidence library

| Collection | Contents |
|---|---|
| [November 18 commit record](releases/2026-11-18/README.md) | One order sent to the three-way commit with one fact changed in each case: the acceptance and 23 refusals, then collection and the lender's withdrawal, with the trace, the case index and the test module. |
| [November 18 shared-reserve record](releases/2026-11-18/shared-reserve/README.md) | Two agents and two sellers on one budget and one lender wallet, in two runs: the accepted and refused orders, each seller's collection and the lender's withdrawals, with both traces, the case index and the test module. |
| [November 18 named-signer record](releases/2026-11-18/named-signer/README.md) | Six orders that each name who signs for delivery: the bills, the collection and ten refusals, with the trace, the case index, the test module and the two program source files that implement the rule. |
| [Current verifier for the Oct 30 record](releases/2026-11-18/verifier/README.md) | The verifier that rereads the Oct 30 transactions from a Solana mainnet RPC and checks the archived store file, with the output of its November 18 run: 54 PASS lines and no FAIL line. |
| [July 18 earning run](releases/2026-11-17/README.md) | Nine mainnet transactions that kept an owner's balance in a yield-bearing token inside the owner's wallet, the program rule that protects what the wallet owes, and the test logs. |
| [November 6 release](releases/2026-11-06/README.md) | Billing through collection after revocation, guardian recovery and one purchase through the API, each with its records and standalone checks. |
| [Oct 30 mainnet record](excerpts/records.md#the-oct-30-mainnet-record) | Passkey approval, signed seller and purchase records, the refused diversion, collection transactions, disclosure examples and a [standalone verifier](releases/2026-11-18/verifier/README.md). |
| [Account commitments and collection](excerpts/records.md#the-decoded-account-sequence) | Pinned program source, retained execution records, shared-backing and authority-change scenarios, financial receipts and offline checks. |
| [Account program excerpts](excerpts/account-programs.md) | The exact source lines and recorded rows for order acceptance, the network fee and corrections. |
| [Record excerpts](excerpts/records.md) | The quoted lines of the Oct 30 record, the Nov 1 sequence and refused wallet rows, the Nov 4 checks and the Nov 6 release. |

## Check them yourself

Article guides link each claim to specific records and source. Each collection gives its verification commands, dependency versions and file hashes. A clone holds everything on this page:

```sh
git clone https://github.com/Dexter-DAO/dexter-article-evidence.git
```

The [November 18 ZIP](https://github.com/Dexter-DAO/dexter-article-evidence/releases/download/2026-11-18/dexter-article-evidence-2026-11-18.zip) and its [SHA-256 checksum](https://github.com/Dexter-DAO/dexter-article-evidence/releases/download/2026-11-18/SHA256SUMS) hold the November 4 collection, the November 6 records, the July 18 earning run, the November 18 commit and shared-reserve records and the [current verifier](releases/2026-11-18/verifier/README.md) for the Oct 30 record. The [named-signer record](releases/2026-11-18/named-signer/README.md) was added after the ZIP was cut and is in the repository only. Each release folder has its own `SHA256SUMS`.

The [article catalog](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/articles/index.json) is a machine-readable starting point. Evidence manifests and provenance files identify exact source revisions.

---

<p align="center">
  <a href="https://dexter.cash">Dexter</a> &middot;
  <a href="https://docs.dexter.cash">Documentation</a> &middot;
  <a href="https://github.com/Dexter-DAO">Dexter-DAO</a>
</p>
