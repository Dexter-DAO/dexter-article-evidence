DEXTER ARTICLE EVIDENCE

This package connects the article's technical claims to the records, source and checks used to support them.

1. HISTORICAL MAINNET DEMONSTRATION
   Start: historical-evidence/findings.txt
   Record: historical-evidence/record.json
   Commands and fallback: historical-evidence/README.txt
   Public case: https://dexter.cash/mandate?case=b0ee0521-898e-828b-9c14-265bfb59216c

   The retained case connects passkey approval, seller evidence, signed purchase vouchers, refused payment diversion and subsequent collection. The two links in historical-evidence/disclosure-links.json demonstrate the same record with its shared name concealed and with permission to reveal it.

   Fresh checks on November 4: 21 checks of the exact published record and four mutation tests passed. The pinned public verifier's 82 fixture and mutation tests also passed. The full chain verifier and transaction identifiers are included for an archival-RPC check.

2. ACCOUNT COMMITMENTS AND COLLECTION
   Start: account-evidence/README.md
   Decoded sequence: account-evidence/checks/article-claim-check-20261104.json
   Fresh results: account-evidence/checks/offline-check-20261104.json

   The shared-backing scenario records rejected attempts to spend committed funds, agent revocation, cooperative owner-authority rotation and collection by a separate signer. Collection pays the supplier while preserving unused backing and the company's financing debt. The exact source and signed records are included.

   Fresh offline verification passed for 15 retained scenarios: 1,406 signed packets, 272 financial receipts, 115 refusals with rollback and 5,113 recorded-state assertions. These checks authenticate and reconcile the retained local execution records. They do not constitute a fresh execution of the compiled programs. account-evidence/REPLAY.md identifies the original binary hashes and runtime requirements for that step.

3. PASSKEY, GRANT AND PAYMENT BINDING
   The Account README points directly to the historical WebAuthn, grant and transfer-check source. The historical case and newer Account records are separately identified, with their exact source revisions.

IF A NETWORK REQUEST FAILS

Read the local findings and raw JSON first. The package includes the records used by the offline checks. Install the pinned Python or Node dependencies once, then run the relevant checks without a live RPC. The full historical chain check can use a different archival RPC endpoint. The source and transaction identifiers remain available for further inspection.

FILE INTEGRITY AND RIGHTS

manifest.json records the size and SHA-256 of every included file. The Account and historical subpackages also retain their own provenance and hash manifests. Historical Vault source retains its MIT license. Account source retains its existing copyright; this candidate adds no open-source license grant. Dependency packages and production credentials are not included.
