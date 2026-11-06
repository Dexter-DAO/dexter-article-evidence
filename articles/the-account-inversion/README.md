<p align="center">
  <img src="https://raw.githubusercontent.com/Dexter-DAO/dexter-x402-sdk/main/assets/dexter-wordmark.svg" alt="Dexter" width="360">
</p>

<h1 align="center">The Account Inversion</h1>

<p align="center"><strong>From Account Holder to Account Owner</strong></p>

<p align="center">
  <a href="../../README.md#articles">All articles</a> &middot;
  <a href="#start-with-a-claim">Evidence by claim</a> &middot;
  <a href="https://github.com/Dexter-DAO/dexter-article-evidence/releases/tag/2026-11-04">Download package</a> &middot;
  <a href="https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/articles/the-account-inversion/evidence.json">Machine-readable index</a>
</p>

---

These records and checks support the article's account-ownership, authority, commitment and settlement explanations. The [November 6 capability guide](../../releases/2026-11-06/README.md) adds continuing obligations, credential recovery and standing commerce.

## Start with a claim

| Article claim | Inspect it |
|---|---|
| On Solana mainnet on Oct 30, 2026, Dexter's Vault program, against demonstration stores Dexter operated, refused a payment diverted to the wrong recipient, and the rightful recipient then collected the same claim. | [Case record](https://dexter.cash/mandate?case=b0ee0521-898e-828b-9c14-265bfb59216c), [findings and transaction links](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/excerpts/records.md#the-oct-30-mainnet-record), [exact published record](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/historical-evidence/record.json) |
| On compiled Account programs on a local SVM, with synthetic assets, protected backing survives attempted spending, revocation of the initiating agent and cooperative rotation of owner authority. A separate signer then collects an earned supplier payment, and unused backing and financing debt stay intact. | [Decoded Account sequence](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/excerpts/records.md#the-decoded-account-sequence), [source and explanation](../../excerpts/account-programs.md), [script that checks it](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/inspect_article_claim.py), [collection source](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/execution/foundation/onchain/programs/claims-core/src/funding_collection.rs) |
| In Dexter's Vault program source, pinned at 7dbd239, passkey approval binds the wallet grant, and claim discharge is tied to the actual payment. | [WebAuthn verifier](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/vault/programs/dexter-vault/src/verify/webauthn.rs), [grant installation](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/vault/programs/dexter-vault/src/instructions/grant_agent_session.rs), [settlement check](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/vault/programs/dexter-vault/src/instructions/settle_locked_voucher.rs) |
| In a local SDK/RPC run with synthetic source facts, accepted work is billed and collected after the initiating actor's revocation, and the buyer's debt remains. | [Billing guide](../../releases/2026-11-06/billing/README.md) |
| On compiled programs in a local runtime, an enrolled guardian quorum restores control without the old owner's signature, and accepted obligations continue. | [Recovery guide](../../releases/2026-11-06/recovery/README.md) |

## Check the records

Each record has its own source revisions and verification instructions.

| Checks completed November 4, 2026 | Result |
|---|---|
| Exact Oct 30 record: hashes, seller evidence, agent vouchers and receipt signatures | [21 checks passed](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/historical-evidence/results/record-offline-checks.json) |
| Mutations of the Oct 30 record | [4 tests passed](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/historical-evidence/results/offline-record-mutations.tap) |
| Pinned verifier fixture and mutation suite | [82 tests passed](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/historical-evidence/results/verifier-tests.tap) |
| Account execution records | [15 scenarios passed](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/excerpts/records.md#fifteen-recorded-scenarios): 1,406 signed packets, 272 financial receipts, 115 refusals with rollback and 5,113 recorded-state assertions |

The [verifier instructions](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/excerpts/records.md#check-the-oct-30-record) read the chain through an archival RPC.

## Download and check offline

Download the package and open the raw records locally. Install the pinned dependencies once, then run the offline checks using the [Oct 30 record instructions](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/excerpts/records.md#check-the-oct-30-record) or [Account instructions](../../excerpts/account-programs.md#check-the-account-records). The Oct 30 package also lists exact transaction identifiers for any archival RPC.

The package's two disclosure examples show the same case, on the [case page](https://dexter.cash/mandate?case=b0ee0521-898e-828b-9c14-265bfb59216c), with the shared name concealed and with a key that lets the reader reveal it. The key is present only in the explicit shared-name link.

## Provenance and rights

The evidence files are preserved byte-for-byte from the approved package. Their manifests retain the exact source pins. [APPROVED-PACKAGE.json](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/APPROVED-PACKAGE.json) records the downloadable archive's hash and size. Licensing is in the [source notices](../../excerpts/records.md#source-notices-and-approval).
