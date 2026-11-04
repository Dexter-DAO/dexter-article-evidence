DEXTER: HISTORICAL MAINNET CASE AND REPRODUCIBLE CHECKS

This package preserves the October 30, 2026 Solana mainnet demonstration: a
passkey-authorized agent made two Tab purchases; an attempted payout to a
different recipient was refused; the approved seller then collected the same
claim. It contains the published record, the pinned public verifier, and fresh
offline checks of the retained evidence.

START HERE

1. Read findings.txt for the claim, transaction references, and check results.
2. Open record.json for the complete published case, without a website login.
3. Open results/record-offline-checks.json for 21 fresh checks of that record.
4. Open results/verifier-tests.tap for the public verifier's 82 passing tests.
5. Open disclosure-links.json for the paired concealed-name and shared-name
   demonstrations. The shared-name link carries an intentional disclosure key.

The mainnet evidence belongs to the historical Vault implementation. Current
Account implementation evidence is a separate package. Each result below names
the artifact it checks.

WHAT WORKS WITHOUT A NETWORK

Every record, source file, result, and explanation can be read locally. The
record's raw bytes have SHA256:
9c2052a9e07706d1596d819521615c187b76fc215087cc37cd13b1c17cbd152a

Verify the package's file hashes from this directory:

  shasum -a 256 -c SHA256SUMS

The offline checker validates the saved seller file and quote, the binding's
commitment to the recorded ceremony nonce, the agent's voucher signatures,
seller receipt signatures, and the saved copycat decisions. Its network access
is disabled. Full blockchain verification reads approval, lock, reservation,
settlement, and refusal transactions from an archival Solana RPC.

REPRODUCE THE CHECKS

Use Node 24 or newer. Install the pinned dependencies once, while online:

  cd verifier
  npm ci --ignore-scripts --no-audit --no-fund
  npm test
  cd ..
  node checks/check-record-offline.mjs
  node --test checks/offline-record.test.mjs

After installation, both test suites and the record checker run locally.
The ZIP includes the dependency lockfile; installed node_modules are excluded.

To reread the historical chain evidence:

  cd verifier
  node verify.mjs ../record.json --rpc https://YOUR-ARCHIVAL-SOLANA-RPC

The command is read-only and submits no transaction. The full verifier also
checks the retained Wayback capture and first-contact screenshot against their
recorded hashes. Its README explains each check and exit status.

SOURCE PINS

Repository: https://github.com/Dexter-DAO/dexter-x402-sdk
Verifier commit: 43980c8daaa5e3067681b9e3738b7bbb968ea200
Case-page commit: 2bfd942a588f44e72755f15d22ad8f334e0dad64
Both commits have Git tree: 2f50c2064370bb7f18d79a6aa6e31ef70a962d7a

All nine files copied from the verifier directory match their original Git
blob hashes. source-provenance.json records each file and its direct source
URL. checks/ contains the additional offline inspection code for this package.

IF A WEBSITE OR PROVIDER DOES NOT OPEN

Use this package's record.json and saved results immediately. Direct links in
evidence-index.json let you inspect the individual chain transactions, pinned
source, or original record independently. The retained results name what ran;
a network failure does not turn the record or its source into missing evidence.
