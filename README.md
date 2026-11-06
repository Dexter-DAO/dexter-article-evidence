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
  <a href="https://github.com/Dexter-DAO/dexter-article-evidence/releases">Downloads</a> &middot;
  <a href="https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/articles/index.json">Machine-readable catalog</a>
</p>

---

Dexter Labs Corporation builds accounts that separate permission from possession. An agent holds a key inside limits the owner approved, and the money stays in the wallet. Three checks meet at the program in one transaction: the owner's permission, the money behind the order, and the seller and terms. Earned payment collects with no new signature from the buyer, the agent, the lender or Dexter. Obligations stay with the account when an agent or operator changes.

The [company reference](https://dexter.cash/llms.txt) explains the problem and the design. This repository holds the proof.

## What the records show

| Result | Read it |
|---|---|
| On Solana mainnet on Oct 30, 2026, Dexter's Vault program, against demonstration stores Dexter operated, refused four attempts with Dexter's own payment key signing: a charge to a copycat's address, a reserved payout rerouted to the copycat, 1.50 USDC with 1.00 left, and a charge after expiry. The refused payout and the store's later payment name the same claim. | [Case record](https://dexter.cash/mandate?case=b0ee0521-898e-828b-9c14-265bfb59216c), [findings](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/excerpts/records.md#the-oct-30-mainnet-record), [exact record](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/historical-evidence/record.json) |
| Compiled Account programs on a local SVM, with synthetic assets, recorded this order: work earned, owner revokes the initiating agent, the financier's owner authority rotates cooperatively, one separate signer collects. Payable goes from 100,000 token atoms to 0, unused backing stays 200,000, company debt stays 100,000. The 15 scenarios from those programs hold 1,406 signed packets, 272 financial receipts, 115 refusals with rollback and 5,113 recorded-state assertions. | [Decoded sequence](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/excerpts/records.md#the-decoded-account-sequence), [offline results](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/excerpts/records.md#fifteen-recorded-scenarios), [case index](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/account/docs/evidence/wallet-spending-20261101/case-index.json) |
| In a local SDK/RPC run with separate signers, accepted work is billed and collected after the initiating actor's revocation, and a supplier claim of 1,400 fixture units is billed at the 900 its synthetic source facts support. | [Billing](releases/2026-11-06/billing/README.md) |
| On compiled programs in a local runtime, a two-of-three guardian quorum restores control without the old owner's signature. A supplier recovers between collections, and both reach the same custody. | [Recovery](releases/2026-11-06/recovery/README.md) |
| On local Agave, with controlled provider identities, the API, PostgreSQL, SDK and native programs run one purchase from admission through provider evidence, claim, deadline finalization and collection. | [Standing commerce](releases/2026-11-06/continuing-api/README.md) |

## What the programs refused

The [case index](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/account/docs/evidence/wallet-spending-20261101/case-index.json) of those Account programs names all 300 transactions in the send, shared and swap suites by suite and index. These rows are refusals:

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

The rules are in the [program source](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/evidence/account-evidence/execution/foundation/onchain/programs), at the execution pin of those records. The [excerpts page](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/excerpts/account-programs.md) quotes the lines for order acceptance, the network fee and corrections. Then read [the reserve](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/execution/foundation/onchain/programs/claims-core/src/funding_state.rs) and [collection](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/execution/foundation/onchain/programs/claims-core/src/funding_collection.rs).

## Articles

| Article | Evidence | Package |
|---|---|---|
| **[The Account Inversion](articles/the-account-inversion/)**<br>From Account Holder to Account Owner | [Claims, records and checks](articles/the-account-inversion/README.md) | [November 4, 2026](https://github.com/Dexter-DAO/dexter-article-evidence/releases/tag/2026-11-04) |

## Evidence library

| Collection | Contents |
|---|---|
| [November 6 release](releases/2026-11-06/README.md) | Billing through collection after revocation, guardian recovery and one purchase through the API, each with its records and standalone checks. |
| [Oct 30 mainnet record](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/excerpts/records.md#the-oct-30-mainnet-record) | Passkey approval, signed seller and purchase records, the refused diversion, collection transactions, disclosure examples and a standalone verifier. |
| [Account commitments and collection](excerpts/records.md#the-decoded-account-sequence) | Pinned program source, retained execution records, shared-backing and authority-change scenarios, financial receipts and offline checks. |
| [Account program excerpts](excerpts/account-programs.md) | The exact source lines and recorded rows for order acceptance, the network fee and corrections. |
| [Record excerpts](excerpts/records.md) | The quoted lines of the Oct 30 record, the Nov 1 sequence, the Nov 4 checks and the Nov 6 release. |

## Check them yourself

Article guides link each claim to specific records and source. Each collection gives its verification commands, dependency versions and file hashes. The [dated release](https://github.com/Dexter-DAO/dexter-article-evidence/releases/tag/2026-11-06b) and [November 4 release](https://github.com/Dexter-DAO/dexter-article-evidence/releases/tag/2026-11-04) download as ZIP files, so a reader can run the checks offline.

The [article catalog](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/articles/index.json) is a machine-readable starting point. Evidence manifests identify exact source revisions, and release archives preserve the files used for each publication.

---

<p align="center">
  <a href="https://dexter.cash">Dexter</a> &middot;
  <a href="https://docs.dexter.cash">Documentation</a> &middot;
  <a href="https://github.com/Dexter-DAO">Dexter-DAO</a>
</p>
