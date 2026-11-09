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
| Dexter signed a payout to the wrong recipient with its own payment key. Vault refused it, and the approved store collected the same claim. The case also records refusals of a copycat charge, an amount above the remaining limit and a charge after expiry. Dexter operated the demonstration stores on Solana mainnet on October 30, 2026. | [Case record](https://dexter.cash/mandate?case=b0ee0521-898e-828b-9c14-265bfb59216c), [transactions](excerpts/records.md#the-oct-30-mainnet-record), [signed record](evidence/historical-evidence/record.json) |
| A separate signer collected the supplier's earned payment after the owner revoked the agent and the financier cooperatively rotated its owner key. The payable fell from 100,000 token atoms to zero. Unused backing stayed at 200,000 and buyer debt stayed at 100,000. The compiled Account programs ran on a local SVM with synthetic assets. | [Decoded sequence](excerpts/records.md#the-decoded-account-sequence), [15 recorded scenarios](excerpts/records.md#fifteen-recorded-scenarios), [case index](evidence/account-evidence/account/docs/evidence/wallet-spending-20261101/case-index.json) |
| After the initiating actor's revocation, a supplier claimed 1,400 units. The work record supported 900. A separate signer collected 900 for the supplier, and the buyer still owed the financier 900. The SDK/RPC run used synthetic work records on a local validator. | [Billing record](excerpts/records.md#billing-after-revocation) |
| The supplier recovered its account between two collections. The first 400 units and the remaining 500 reached the same custody. Two of three enrolled guardians authorized the new key on compiled Account programs in a local runtime. | [Recovery record](excerpts/records.md#guardian-recovery-between-collections), [guardian policy and original trace](releases/2026-11-06/recovery/README.md) |
| The API, PostgreSQL, SDK and native programs carried a purchase from admission through claim, finalization and supplier payment. Twenty-six fresh processes recovered the recorded outcomes without a new payment or funding authorization. This run used local Agave and controlled provider identities. | [Purchase record](excerpts/records.md#one-purchase-from-admission-to-collection) |

## What the programs refused

Across 15 Account scenarios, the recorded checks covered 1,406 signed packets, 272 financial receipts, 115 refusals with rollback and 5,113 recorded-state assertions. The [case index](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/account/docs/evidence/wallet-spending-20261101/case-index.json) of those Account programs names all 300 transactions in the send, shared and swap suites by suite and index. These rows are refusals:

- wallet-spending-send 77, "per-operation ceiling refuses one excess token", error 6033.
- wallet-spending-send 81, "cumulative authority cannot be refilled by a new request id", error 6033.
- wallet-spending-send 86, "standing permission cannot start before its not-before bound", error 6033.
- wallet-spending-send 88, "standing permission expires at its exclusive time boundary", error 6033.
- wallet-spending-send 91, "revocation prevents a previously unused delegated send", error 6033.
- wallet-spending-send 98, "spending approval cannot use another principal's capital", error 6030.
- wallet-spending-send 102, "owner rotation invalidates an old-generation unused permission", error 6020.
- wallet-spending-send 62, "changed signed bundle hash cannot authorize an ordinary send", error 6025.
- wallet-spending-shared 73, "ordinary withdrawal preserves the same canonical reserve floor", error 6033.
- wallet-spending-shared 72, "a separate exact owner approval cannot spend one committed token", error 6033.
- wallet-spending-shared 69, "fresh authorization still refuses spending another facility's backing", error 6033.
- wallet-spending-swap 97, "owner-approved swap cannot convert accepted supplier backing into another asset", error 6033.

The rules are in the [program source](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/evidence/account-evidence/execution/foundation/onchain/programs), at the execution pin of those records. The [excerpts page](excerpts/account-programs.md) quotes the lines for order acceptance, the network fee and corrections. Then read [the reserve](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/execution/foundation/onchain/programs/claims-core/src/funding_state.rs) and [collection](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/execution/foundation/onchain/programs/claims-core/src/funding_collection.rs).

## Articles

| Article | Evidence | Package |
|---|---|---|
| **[The Account Inversion](articles/the-account-inversion/)**<br>From Account Holder to Account Owner | [Claims, records and checks](articles/the-account-inversion/README.md) | [November 6b ZIP](https://github.com/Dexter-DAO/dexter-article-evidence/releases/download/2026-11-06b/dexter-article-evidence-2026-11-06b.zip) |

## Evidence library

| Collection | Contents |
|---|---|
| [November 6 release](releases/2026-11-06/README.md) | Billing through collection after revocation, guardian recovery and one purchase through the API, each with its records and standalone checks. |
| [Oct 30 mainnet record](excerpts/records.md#the-oct-30-mainnet-record) | Passkey approval, signed seller and purchase records, the refused diversion, collection transactions, disclosure examples and a standalone verifier. |
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
