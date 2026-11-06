# An Account that keeps its commitments

Dexter separates the continuing economic account from the agents and services operating it. An owner can revoke an operator or recover control, and the work already accepted, the supplier's payment rights and the buyer's debt stay in place. Customers keep control over future work, and counterparties can rely on existing commitments.

This release publishes the recorded execution of three parts of that system on local validators and LiteSVM, with the exact source commit behind each run: operator revocation and continuing obligations, credential recovery, and standing commerce. The records hold compiled-program execution, SDK/RPC journeys and a combined API and database purchase.

## Results

| Capability | What the recorded execution shows | Inspect |
|---|---|---|
| Operator revocation and continuing obligations | Revoking the initiating actor blocks new authority, and accepted work can still be reported, billed and independently collected. The supplier receives 900 fixture units from financier custody, and the buyer's debt of 900 fixture units remains. Separate signers and fresh recovery processes finish retained operations. | [Billing and operator continuation](billing/README.md) |
| Credential recovery | An enrolled guardian quorum restores control without the lost or hostile owner's signature. Buyer, financier, supplier and assignee owners recover after one bill's report and claim are filed and before the main bill's. The earlier claim still pays its original destination. A supplier recovers again between collections, both payments reach the same continuing custody, and the new owner can withdraw. Old authority is refused. | [Recovery journey, records and guardian powers](recovery/README.md) |
| Standing commerce | The API, PostgreSQL, SDK and native programs carry a purchase through admission, retained provider evidence, claim, deadline finalization and collection. Twenty-six recovery processes read retained outcomes without sending another transaction or requesting new funding or admission authorization. | [Purchase records and standalone checker](continuing-api/README.md) |

## Other records in this repository

- The [decoded Account sequence](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/excerpts/records.md#the-decoded-account-sequence), on a local SVM with synthetic assets, has this order: work earned, owner revokes the initiating agent, the financier's owner authority rotates cooperatively, one separate signer collects.
- The [Oct 30 case](https://dexter.cash/mandate?case=b0ee0521-898e-828b-9c14-265bfb59216c), on Solana mainnet on Dexter's Vault program against demonstration stores Dexter operated, records four attempts the program refused with Dexter's own payment key signing, and the store collecting the same claim afterward. The [findings](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/excerpts/records.md#the-oct-30-mainnet-record) give its transaction links.

## Download and check

[Download the dated ZIP and SHA-256 checksum](https://github.com/Dexter-DAO/dexter-article-evidence/releases/tag/2026-11-06b) from the release page. After extracting the ZIP, run from its top-level directory:

```sh
python3 releases/2026-11-06/verify-release.py
node releases/2026-11-06/continuing-api/verify-records.mjs
```

The commands use Python 3.9 or later and Node.js 22 or later, with standard libraries only. They read the downloaded files and run offline. The first checks the release manifest and retained recovery observations. The second verifies the selected API transaction signatures and record consistency.

## Sources

[Claims and record links](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/releases/2026-11-06/claims.json) provide a machine-readable map. [Provenance](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/releases/2026-11-06/provenance.json) records exact source commits, original and export hashes and selected files. Each execution retains its own source and binary identity.

The [claim guide](../../articles/the-account-inversion/README.md) maps The Account Inversion to its records.
