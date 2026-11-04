# Dexter Article Evidence

Records, source and runnable checks behind **The Account Inversion**.

[Download the complete evidence package](https://github.com/Dexter-DAO/dexter-article-evidence/releases/download/2026-11-04/dexter-article-evidence-2026-11-04.zip) · [Read the plain-text guide](evidence/README.txt) · [Inspect the file manifest](evidence/manifest.json)

## Start with a claim

| Article claim | Inspect it |
|---|---|
| A payment diverted to the wrong recipient is refused; the rightful recipient can subsequently collect. | [Historical mainnet case and transaction links](evidence/historical-evidence/findings.txt), [exact published record](evidence/historical-evidence/record.json), [public demonstration](https://dexter.cash/mandate?case=b0ee0521-898e-828b-9c14-265bfb59216c) |
| Protected backing survives attempted spending, revocation of the initiating agent, and cooperative rotation of owner authority. | [Decoded Account sequence](evidence/account-evidence/checks/article-claim-check-20261104.json), [source and explanation](evidence/account-evidence/README.md) |
| A separate signer can collect an earned supplier payment while unused backing and financing debt remain intact. | [The same collection sequence](evidence/account-evidence/checks/article-claim-check-20261104.json), [script that checks it](evidence/account-evidence/inspect_article_claim.py) |
| Passkey approval binds the wallet grant; claim discharge is tied to the actual payment. | [Pinned WebAuthn verifier](evidence/account-evidence/vault/programs/dexter-vault/src/verify/webauthn.rs), [grant installation](evidence/account-evidence/vault/programs/dexter-vault/src/instructions/grant_agent_session.rs), [settlement check](evidence/account-evidence/vault/programs/dexter-vault/src/instructions/settle_locked_voucher.rs) |

## Check the records

The historical case is a retained Solana mainnet demonstration. The newer Account scenarios were executed with compiled programs in a local SVM. Each has its own source revisions and verification instructions.

| Fresh checks completed November 4, 2026 | Result |
|---|---|
| Exact historical record: hashes, seller evidence, agent vouchers and receipt signatures | [21 checks passed](evidence/historical-evidence/results/record-offline-checks.json) |
| Mutations of the historical record | [4 tests passed](evidence/historical-evidence/results/offline-record-mutations.tap) |
| Pinned historical verifier fixture and mutation suite | [82 tests passed](evidence/historical-evidence/results/verifier-tests.tap) |
| Retained Account execution records | [15 scenarios passed](evidence/account-evidence/checks/offline-check-20261104.json): 1,406 signed packets, 272 financial receipts, 115 refusals with rollback and 5,113 recorded-state assertions |

These checks authenticate and reconcile saved records. The [historical verifier](evidence/historical-evidence/README.txt) can also read the chain through an archival RPC. The [Account replay guide](evidence/account-evidence/REPLAY.md) specifies the compiled artifacts and runtime needed to execute those programs again.

## If a page or network request fails

Download the package and open the raw records locally. Install the pinned dependencies once, then run the offline checks using the [historical instructions](evidence/historical-evidence/README.txt) or [Account instructions](evidence/account-evidence/README.md). The historical package also lists exact transaction identifiers for inspection through another archival RPC.

The [two disclosure examples](evidence/historical-evidence/disclosure-links.json) show the same historical case with the shared name concealed and with a key that permits the reader to reveal it. The key is present only in the explicit shared-name link.

## Provenance and rights

The evidence files are preserved byte-for-byte from the approved package. Their manifests retain the preparation-time status and exact source pins. [APPROVED-PACKAGE.json](APPROVED-PACKAGE.json) records the downloadable archive's hash and size.

Historical Vault source retains its MIT license. Account source retains its existing copyright; this publication adds no open-source license grant. See the [source notices](evidence/account-evidence/NOTICE.md).
