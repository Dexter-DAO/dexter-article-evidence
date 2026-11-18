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
  <a href="https://github.com/Dexter-DAO/dexter-article-evidence/releases/tag/2026-11-06b">Download the evidence</a> &middot;
  <a href="https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/articles/index.json">Machine-readable catalog</a>
</p>

---

Dexter Labs Corporation builds accounts that separate permission from possession. An agent holds a key inside limits the owner approved, and the money stays in the wallet. Three checks meet at the program in one transaction: the owner's permission, the money behind the order, and the seller and terms. Earned payment collects with no new signature from the buyer, the agent, the lender or Dexter. Obligations stay with the account when an agent or operator changes.

The [company reference](https://dexter.cash/llms.txt) explains the problem and the design. This repository holds the proof.

## What the records show

| Result | Read it |
|---|---|
| One transaction checks the owner's permission, the money behind the order, and the seller and the exact terms, and it refuses the order when any one is missing. The commit accepted an order with a maximum of 100,000 units and all three present, and it refused 23 attempts that each lacked one. A key with no role in the order then collected the seller's earned 60,000, and eight altered versions of that collection were refused. The compiled Account programs ran on a local SVM with a synthetic token on November 18, 2026. | [Guide and tables](releases/2026-11-18/README.md), [case index](releases/2026-11-18/case-index.json), [trace](releases/2026-11-18/commit-legs-trace.json.xz) |
| Dexter signed a payout to the wrong recipient with its own payment key. The program refused it, and the approved store collected the same claim. The case also records refusals of a copycat charge, an amount above the remaining limit and a charge after expiry. Dexter operated the demonstration stores on Solana mainnet on October 30, 2026. | [Case record](https://dexter.cash/mandate?case=b0ee0521-898e-828b-9c14-265bfb59216c), [transactions](excerpts/records.md#the-oct-30-mainnet-record), [signed record](evidence/historical-evidence/record.json) |
| An owner's dollars moved into a yield-bearing token and back, twice, without leaving the owner's wallet. Each trade carried the owner's passkey signature and the Vault program checked the balances in the same transaction. The program refuses a purchase that would leave less stablecoin than the wallet owes. Solana mainnet, July 18, 2026. | [Guide and transactions](releases/2026-11-17/README.md), [on-chain readback](releases/2026-11-17/mainnet-transactions.json) |
| A separate signer collected the supplier's earned payment after the owner revoked the agent and the financier cooperatively rotated its owner key. The payable fell from 100,000 token atoms to zero. Unused backing stayed at 200,000 and buyer debt stayed at 100,000. The compiled Account programs ran on a local SVM with synthetic assets. | [Decoded sequence](excerpts/records.md#the-decoded-account-sequence), [15 recorded scenarios](excerpts/records.md#fifteen-recorded-scenarios), [case index](evidence/account-evidence/account/docs/evidence/wallet-spending-20261101/case-index.json) |
| After the initiating actor's revocation, a supplier claimed 1,400 units. The work record supported 900. A separate signer collected 900 for the supplier, and the buyer still owed the financier 900. The SDK/RPC run used synthetic work records on a local validator. | [Billing record](excerpts/records.md#billing-after-revocation) |
| The supplier recovered its account between two collections. The first 400 units and the remaining 500 reached the same custody. Two of three enrolled guardians authorized the new key on compiled Account programs in a local runtime. | [Recovery record](excerpts/records.md#guardian-recovery-between-collections), [guardian policy and original trace](releases/2026-11-06/recovery/README.md) |
| The API, PostgreSQL, SDK and native programs carried a purchase from admission through claim, finalization and supplier payment. Twenty-six fresh processes recovered the recorded outcomes without a new payment or funding authorization. This run used local Agave and controlled provider identities. | [Purchase record](excerpts/records.md#one-purchase-from-admission-to-collection) |

## What the programs refused

The [November 18 record](releases/2026-11-18/README.md) sends one order to the commit, ClaimsCore `accept_financed_tab`, with one fact changed in each case. Its [case index](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/releases/2026-11-18/case-index.json) names all 117 transactions by index. These rows are refusals of the commit:

- commit-legs 67, "Q1 refused: order of 100,001 against a per-order limit of 100,000", PrincipalRegistry `PerOperationLimit`, error 6011.
- commit-legs 70, "Q1 refused: order 1 (100,000) against a remaining budget of 99,999", PrincipalRegistry `BudgetExhausted`, error 6012.
- commit-legs 110, "Q1 refused: the revoked agent signs order 4 (100,000)", PrincipalRegistry `CredentialInactive`, error 6004.
- commit-legs 73, "Q2 refused: order 1 (100,000) against a lender facility limit of 99,999", ClaimsCore `FundingFacility`, error 6025.
- commit-legs 81, "Q2 refused: order 1 (100,000) against lender free cash of 99,999", ClaimsCore `FundingCashShortfall`, error 6033.
- commit-legs 57, "Q3 refused: a key that is not the seller signs order 1 in the seller's place", ClaimsCore `FundingOwner`, error 6023.
- commit-legs 60, "Q3 refused: order 1 says 100,000 but the seller-side terms say 99,999", ClaimsCore `FundingTerms`, error 6031.
- commit-legs 65, "Q1+Q3 refused: a different registered seller signs order 1; the owner's permission names the first seller", PrincipalRegistry `IntentMismatch`, error 6010.

Row 85 is order 1 with nothing changed, and the commit accepts it. The release lists all 23 refusals in three tables: permission (Q1), money (Q2) and the deal (Q3).

Across 15 Account scenarios recorded on November 1, the checks covered 1,406 signed packets, 272 financial receipts, 115 refusals with rollback and 5,113 recorded-state assertions. The [case index](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/account/docs/evidence/wallet-spending-20261101/case-index.json) of those Account programs names all 300 transactions in the send, shared and swap suites by suite and index. These rows are refusals of wallet sends, a withdrawal and a swap. The first eight are sends that PrincipalRegistry refused while authorizing the wallet. The last four are a withdrawal, two sends and a swap that ClaimsCore refused because each would leave the wallet below its committed backing. Error 6033 is `SpendingPermission` in PrincipalRegistry and `FundingCashShortfall` in ClaimsCore, so each row names its program and error:

- wallet-spending-send 77, "per-operation ceiling refuses one excess token", PrincipalRegistry `SpendingPermission`, error 6033.
- wallet-spending-send 81, "cumulative authority cannot be refilled by a new request id", PrincipalRegistry `SpendingPermission`, error 6033.
- wallet-spending-send 86, "standing permission cannot start before its not-before bound", PrincipalRegistry `SpendingPermission`, error 6033.
- wallet-spending-send 88, "standing permission expires at its exclusive time boundary", PrincipalRegistry `SpendingPermission`, error 6033.
- wallet-spending-send 91, "revocation prevents a previously unused delegated send", PrincipalRegistry `SpendingPermission`, error 6033.
- wallet-spending-send 98, "spending approval cannot use another principal's capital", PrincipalRegistry `FundingAction`, error 6030.
- wallet-spending-send 102, "owner rotation invalidates an old-generation unused permission", PrincipalRegistry `WalletGeneration`, error 6020.
- wallet-spending-send 62, "changed signed bundle hash cannot authorize an ordinary send", PrincipalRegistry `WalletBundleHash`, error 6025.
- wallet-spending-shared 73, "ordinary withdrawal preserves the same canonical reserve floor", ClaimsCore `FundingCashShortfall`, error 6033.
- wallet-spending-shared 72, "a separate exact owner approval cannot spend one committed token", ClaimsCore `FundingCashShortfall`, error 6033.
- wallet-spending-shared 69, "fresh authorization still refuses spending another facility's backing", ClaimsCore `FundingCashShortfall`, error 6033.
- wallet-spending-swap 97, "owner-approved swap cannot convert accepted supplier backing into another asset", ClaimsCore `FundingCashShortfall`, error 6033.

The rules behind the November 1 rows are in the [program source](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/evidence/account-evidence/execution/foundation/onchain/programs), at the execution pin of those records. The [excerpts page](excerpts/account-programs.md) quotes the lines for order acceptance, the network fee and corrections. Then read [the reserve](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/execution/foundation/onchain/programs/claims-core/src/funding_state.rs) and [collection](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/execution/foundation/onchain/programs/claims-core/src/funding_collection.rs). The November 18 record ran a later commit of the same programs, and its release names that commit and the hash of each binary.

## Articles

| Article | Evidence | Package |
|---|---|---|
| **[Turns Out the Impasse Was a Tech Problem](articles/the-account-inversion/)**<br>Agentic Commerce Is at an Impasse Because Account Holders Are Not Account Owners. That Problem Is Now Solved. | [Claims, records and checks](articles/the-account-inversion/README.md) | [November 6b ZIP](https://github.com/Dexter-DAO/dexter-article-evidence/releases/download/2026-11-06b/dexter-article-evidence-2026-11-06b.zip) |

## Evidence library

| Collection | Contents |
|---|---|
| [November 6 release](releases/2026-11-06/README.md) | Billing through collection after revocation, guardian recovery and one purchase through the API, each with its records and standalone checks. |
| [Oct 30 mainnet record](excerpts/records.md#the-oct-30-mainnet-record) | Passkey approval, signed seller and purchase records, the refused diversion, collection transactions, disclosure examples and a standalone verifier. |
| [November 18 commit record](releases/2026-11-18/README.md) | One order sent to the three-way commit with one fact changed in each case: the acceptance and 23 refusals, then collection and the lender's withdrawal, with the trace, the case index and the test module. |
| [July 18 earning run](releases/2026-11-17/README.md) | Nine mainnet transactions that kept an owner's balance in a yield-bearing token inside the owner's wallet, the program rule that protects what the wallet owes, and the test logs. |
| [Account commitments and collection](excerpts/records.md#the-decoded-account-sequence) | Pinned program source, retained execution records, shared-backing and authority-change scenarios, financial receipts and offline checks. |
| [Account program excerpts](excerpts/account-programs.md) | The exact source lines and recorded rows for order acceptance, the network fee and corrections. |
| [Record excerpts](excerpts/records.md) | The quoted lines of the Oct 30 record, the Nov 1 sequence, the Nov 4 checks and the Nov 6 release. |

## Check them yourself

Article guides link each claim to specific records and source. Each collection gives its verification commands, dependency versions and file hashes. Download the [November 6b ZIP](https://github.com/Dexter-DAO/dexter-article-evidence/releases/download/2026-11-06b/dexter-article-evidence-2026-11-06b.zip) and its [SHA-256 checksum](https://github.com/Dexter-DAO/dexter-article-evidence/releases/download/2026-11-06b/SHA256SUMS). It includes the November 4 collection as well as the November 6 records.

The [article catalog](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/articles/index.json) is a machine-readable starting point. Evidence manifests identify exact source revisions, and release archives preserve the files used for each publication.

---

<p align="center">
  <a href="https://dexter.cash">Dexter</a> &middot;
  <a href="https://docs.dexter.cash">Documentation</a> &middot;
  <a href="https://github.com/Dexter-DAO">Dexter-DAO</a>
</p>
