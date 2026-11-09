<p align="center">
  <img src="https://raw.githubusercontent.com/Dexter-DAO/dexter-x402-sdk/main/assets/dexter-wordmark.svg" alt="Dexter" width="360">
</p>

<h1 align="center">The Account Inversion</h1>

<p align="center"><strong>From Account Holder to Account Owner</strong></p>

<p align="center">
  <a href="../../README.md#articles">All articles</a> &middot;
  <a href="#start-with-a-claim">Evidence by claim</a> &middot;
  <a href="https://github.com/Dexter-DAO/dexter-article-evidence/releases/download/2026-11-06b/dexter-article-evidence-2026-11-06b.zip">Download November 6b ZIP</a> &middot;
  <a href="https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/articles/the-account-inversion/evidence.json">Machine-readable index</a>
</p>

---

A supplier collected payment for accepted work after the initiating agent lost its authority. In a separate run, a supplier recovered its account between payments and received the balance in the same custody. The [November 6 guide](../../releases/2026-11-06/README.md) links both results and the purchase that ran from API admission to collection.

## Start with a claim

| Article claim | Inspect it |
|---|---|
| Dexter signed a diversion with its own payment key. Vault refused it, and the approved store collected the same claim. The transaction ran on Solana mainnet on October 30, 2026, against Dexter-operated demonstration stores. | [Case page](https://dexter.cash/mandate?case=b0ee0521-898e-828b-9c14-265bfb59216c), [transaction links](../../excerpts/records.md#the-oct-30-mainnet-record), [signed record](../../evidence/historical-evidence/record.json) |
| On compiled Account programs on a local SVM, with synthetic assets, protected backing survives attempted spending, revocation of the initiating agent and cooperative rotation of owner authority. A separate signer then collects an earned supplier payment, and unused backing and financing debt stay intact. | [Decoded Account sequence](../../excerpts/records.md#the-decoded-account-sequence), [source and explanation](../../excerpts/account-programs.md), [script that checks it](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/inspect_article_claim.py), [collection source](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/execution/foundation/onchain/programs/claims-core/src/funding_collection.rs) |
| In Dexter's Vault program source, pinned at 7dbd239, passkey approval binds the wallet grant, and claim discharge is tied to the actual payment. | [WebAuthn verifier](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/vault/programs/dexter-vault/src/verify/webauthn.rs), [grant installation](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/vault/programs/dexter-vault/src/instructions/grant_agent_session.rs), [settlement check](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/vault/programs/dexter-vault/src/instructions/settle_locked_voucher.rs) |
| After revocation, an independent signer collected the 900 units the supplier had earned, and the buyer's debt to the financier remained 900. The local SDK/RPC run used synthetic work records. | [Billing results](../../excerpts/records.md#billing-after-revocation), [execution guide](../../releases/2026-11-06/billing/README.md) |
| Two of three enrolled guardians restored control without the old key. A supplier recovered between collections of 400 and 500 units. Both paid the same custody on compiled Account programs in a local runtime. | [Recovery results](../../excerpts/records.md#guardian-recovery-between-collections), [policy and trace](../../releases/2026-11-06/recovery/README.md) |
| The API, database, SDK and native programs finalized an earned bill and paid the supplier. Twenty-six fresh processes recovered recorded outcomes with zero new sends. The run used controlled provider identities on local Agave. | [Transaction sequence](../../excerpts/records.md#one-purchase-from-admission-to-collection), [purchase guide](../../releases/2026-11-06/continuing-api/README.md) |

For the account and its funding arrangements, read the [Account specification excerpts and reserve source](../../excerpts/account-programs.md#the-account). For individual scenarios, the [trace index and wallet verifier](../../excerpts/account-programs.md#read-a-row-yourself) connect the program rules to recorded transactions.

## Check the records

Each record has its own source revisions and verification instructions.

| Checks completed November 4, 2026 | Result |
|---|---|
| Exact Oct 30 record: hashes, seller evidence, agent vouchers and receipt signatures | [21 checks passed](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/historical-evidence/results/record-offline-checks.json) |
| Mutations of the Oct 30 record | [4 tests passed](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/historical-evidence/results/offline-record-mutations.tap) |
| Pinned verifier fixture and mutation suite | [82 tests passed](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/historical-evidence/results/verifier-tests.tap) |
| Account execution records | [15 scenarios passed](../../excerpts/records.md#fifteen-recorded-scenarios): 1,406 signed packets, 272 financial receipts, 115 refusals with rollback and 5,113 recorded-state assertions |

The [verifier instructions](../../excerpts/records.md#check-the-oct-30-record) read the chain through an archival RPC.

## Download and check offline

Download the [November 6b ZIP](https://github.com/Dexter-DAO/dexter-article-evidence/releases/download/2026-11-06b/dexter-article-evidence-2026-11-06b.zip) and [checksum](https://github.com/Dexter-DAO/dexter-article-evidence/releases/download/2026-11-06b/SHA256SUMS). The [November 6 instructions](../../releases/2026-11-06/README.md#download-and-check) verify the recovery and purchase records with Python and Node. For the earlier records, follow the [Oct 30 verifier instructions](../../excerpts/records.md#check-the-oct-30-record) or [Account instructions](../../excerpts/account-programs.md#check-the-account-records).

The package's two disclosure examples show the same case, on the [case page](https://dexter.cash/mandate?case=b0ee0521-898e-828b-9c14-265bfb59216c), with the shared name concealed and with a key that lets the reader reveal it. The key is present only in the explicit shared-name link.

## Provenance and rights

The [November 6b release](https://github.com/Dexter-DAO/dexter-article-evidence/releases/tag/2026-11-06b) publishes the checksum for the current ZIP. [APPROVED-PACKAGE.json](../../APPROVED-PACKAGE.json) identifies the original November 4 archive. Each record retains its source revision. Licensing is in the [source notices](../../excerpts/records.md#source-notices-and-approval).
