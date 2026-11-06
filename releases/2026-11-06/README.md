# An Account that keeps its commitments

Dexter separates the continuing economic account from the agents and services operating it. An owner can replace an operator or recover control without erasing work already accepted, the supplier's payment rights or the buyer's debt. That combination matters for agent commerce: customers retain control over future work while counterparties can rely on existing commitments.

This release publishes completed implementation and recorded execution for three parts of that system: operator replacement and continuing obligations, credential recovery, and standing commerce. The records include local compiled-program execution, SDK/RPC journeys and a combined API/database purchase. The [November 4 archive](https://github.com/Dexter-DAO/dexter-article-evidence/releases/tag/2026-11-04) preserves the separate historical mainnet demonstration and earlier evidence.

## Completed results

| Capability | What the recorded execution demonstrates | Inspect |
|---|---|---|
| Operator replacement and continuing obligations | Revoking the initiating actor blocks new authority while accepted work can still be reported, billed and independently collected. The supplier receives 900 fixture units from financier custody; the buyer's 900-unit debt remains. Separate signers and fresh recovery processes finish retained operations. | [Billing and operator continuation](billing/README.md) |
| Credential recovery | An enrolled guardian quorum restores control without the lost or hostile owner's signature. Buyer, financier, supplier and source owners recover before the first report or claim. A supplier recovers again between collections; both payments reach the same continuing custody, and the new owner can withdraw. Old authority is refused. | [Recovery journey, records and trust assumptions](recovery/README.md) |
| Standing commerce | The API, PostgreSQL, SDK and native programs carry a purchase through admission, retained provider evidence, claim, deadline finalization and collection. Twenty-six recovery processes read retained outcomes without sending another transaction or requesting new funding or admission authorization. | [Purchase records and standalone checker](continuing-api/README.md) |
| Productive backing | Coming next: investment ownership and income, valuation, exclusive backing, liquidity, loss allocation and repayment. This release makes no completed productive-backing claim or delivery-date promise. | Roadmap |

These results establish concrete authority, accounting and continuation behavior. They do not measure customer adoption or commercial returns. The operator result covers independent servicing of accepted work after revocation; it does not claim an arbitrary replacement provider has taken over every business operation. The guardian mechanism and local execution settings are documented beside their records.

## Download and check

[Download the dated ZIP](https://github.com/Dexter-DAO/dexter-article-evidence/releases/download/2026-11-06/dexter-article-evidence-2026-11-06.zip) and [SHA-256 checksum](https://github.com/Dexter-DAO/dexter-article-evidence/releases/download/2026-11-06/SHA256SUMS). After extracting the ZIP, run from its top-level directory:

```sh
python3 releases/2026-11-06/verify-release.py
node releases/2026-11-06/continuing-api/verify-records.mjs
```

The commands use Python 3.9 or later and Node.js 22 or later, with standard libraries only. They inspect the downloaded files without a private repository, credentials, signing keys, network access or transaction submission. The first checks the release manifest and retained recovery observations; the second verifies the selected API transaction signatures and record consistency. A fresh execution of the entire system requires additional build and runtime inputs and is not advertised by these commands.

All new financial amounts are disposable fixture atomic units. They are not dollar revenue, customer balances or evidence of independent merchant adoption. The Account runs use local validators or LiteSVM; software-generated passkey proofs are identified explicitly. Physical-device and full customer rollout are separate from the completed implementation and execution published here. No new engineering run was performed to make this release.

## Sources and archive

[Claims and record links](claims.json) provide a machine-readable map. [Provenance](provenance.json) records exact source commits, original/export hashes, selected files and workspace-path redactions. The recovery/passkey stack was reviewed at `1489470609756238a6728d85ab182741d600d64a` and merged at `133c2286e3c1b52aea952a1a4ddc79acdb9f59ae` with the same tree. Each execution retains its own source and binary identity; separate runs are not presented as a single journey.

The [earlier claim guide](../../articles/the-account-inversion/README.md) and its original mainnet case remain available. This evidence release is independent of the author's unfinished article. Existing source licenses and notices remain in force; publishing records does not grant a new license to private source repositories.
