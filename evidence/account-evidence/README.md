# Dexter Account: inspect the article's evidence

This package contains the signed local execution records, program source and checks behind the Account paragraph in *The Account Inversion*. It also includes the historical Vault code behind the passkey, delegated grant and exact-payment explanation.

Start with the [decoded article sequence](checks/article-claim-check-20261104.json). Two facilities share backing. Attempts to spend committed funds fail. The owner revokes the initiating agent and cooperatively rotates the financier's owner authority. A separate collector then pays the supplier with one transaction signature. The remaining backing stays protected and the company's financing debt stays outstanding.

| Question | Evidence to inspect |
|---|---|
| Can another valid authorization spend committed backing? | Shared trace transactions 69 and 72 return error 6033; their protected account state rolls back. |
| Does collection still work after revocation and rotation? | Shared trace transactions 74, 79 and 81 show that sequence. Transaction 81 has one signature; its signer is neither the initiating agent nor the old or new financier owner. |
| What changes when the supplier collects? | Transaction 81 reduces actual cash and payable by 100,000 token atoms, leaves 200,000 atoms of unused backing, and leaves 100,000 atoms of company financing debt outstanding. |
| What checks the payment destination and amount? | [Account collection source](execution/foundation/onchain/programs/claims-core/src/funding_collection.rs), [wallet authority checks](execution/foundation/onchain/programs/principal-registry/src/wallet.rs), signed packets, account snapshots and immutable receipts. |
| What binds passkey approval to the grant? | Historical [WebAuthn verification](vault/programs/dexter-vault/src/verify/webauthn.rs) and [grant installation](vault/programs/dexter-vault/src/instructions/grant_agent_session.rs). |
| What is the historical payment “weld”? | [Claim settlement](vault/programs/dexter-vault/src/instructions/settle_locked_voucher.rs) checks the [encoded wallet transfers](vault/programs/dexter-vault/src/verify/swig_transfer.rs) before discharging the claim. |

The [retained shared trace](account/docs/evidence/wallet-spending-20261101/wallet-spending-shared-trace.json.xz) contains all 90 signed transactions and their observed account states, including setup. Transaction numbers above are zero-based indices in that file. The [case index](account/docs/evidence/wallet-spending-20261101/case-index.json) names the transactions across the complete retained suite.

## Run the checks

Use Python 3.10 or newer in a fresh environment:

```sh
python3 -m venv .venv
.venv/bin/python -m pip install -r requirements.txt
.venv/bin/python verify_files.py
.venv/bin/python verify_recorded_evidence.py
.venv/bin/python inspect_article_claim.py
```

The checks read local files. They do not contact a blockchain or submit transactions. The first command checks the package hashes. The second authenticates the signed packets and checks recorded account continuity, receipt hashes, financial effects, token movements, reserves and rollback. The third decodes the particular sequence used in the article.

The [November 4 check](checks/offline-check-20261104.json) passed for all 15 retained scenarios: 1,406 signed packets, 272 financial receipts, 115 refusals with rollback and 5,113 recorded assertions. Each fresh result exactly matched the corresponding retained verifier result. These are fresh checks of existing records; the original execution used compiled programs in a local SVM.

## Source versions

The records were collected from `dexter-account` at `433cf9c487637b00057ab5f6a0963f1a5dc7a411`. Their execution manifest pins program/build source to `fb2a3a4d2c2abec37468bd592ae72a75626d7b60` and fixture/verifier source to `5b205c6489efeb8d999d559434e251dfcaf92e2d`. The package preserves those execution pins. All 42 build inputs and all seven named verification inputs match the original manifest.

The historical Vault files come from `7dbd239ec8166d658cc6a51e9a555ddc546a88bb`. They explain the earlier approval and transfer construction. The newer Account's local evidence is a separate record. The [historical mainnet case](https://dexter.cash/mandate?case=b0ee0521-898e-828b-9c14-265bfb59216c) supplies the observed refused diversion and later collection discussed elsewhere in the article.

The [manifest](manifest.json) gives every included file's hash and source revision. The [account architecture excerpt](spec/account-and-interfaces.md) and [Tab funding excerpt](spec/tab-and-backing.md) reproduce selected specification lines exactly. The full original specification and unrelated source files were not included.

## Scope of this record

The Account record establishes the exercised local program paths with synthetic assets and disposable fixture signers. The standalone verifier can check signed records and their consistency; re-executing the programs requires the exact compiled artifacts or a verified rebuild. [Replay requirements](REPLAY.md) identify those artifacts and the original toolchain.

The article sequence uses cooperative owner rotation. A deployed operator outage, lost-owner recovery, governance changes and the dynamic shoe-purchase policy are separate claims. This package does not substitute this record for their evidence. The historical Vault source selection supports inspection of the cited mechanisms; it is not a complete Vault build workspace.

Licensing is recorded in [NOTICE.md](NOTICE.md).
