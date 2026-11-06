# Inspect the retained local purchase records

This collection contains records from the November 5 continuing-purchase test, published in the November 6 evidence release. The run connected Account programs, SDK, API and PostgreSQL through admission, provider evidence, a supplier claim, finalization and collection.

The recorded collection paid the original supplier custody 900 atomic fixture-token units. The fixture token has six decimals; this is 0.0009 fixture tokens, with no claim of dollar revenue. The buyer retained 900 units of debt and had repaid zero. An unused second bill released its 2,000-unit cap without debt. The third admission remained signed but unsent, pending reconciliation.

Run the checker with Node.js 22 or later:

```sh
node verify-records.mjs
```

The checker uses Node's built-in libraries. It makes no network requests, executes no transactions and requires no private checkout or keys. It checks the selected file hashes, 16 retained transaction wires, Ed25519 signatures, captured transaction identifiers and account keys, recorded payment arithmetic and recovery observations. A successful result means those selected records passed the stated checks.

The RPC responses and database observations were captured during a Dexter-controlled local run. Valid signatures establish who signed the transaction bytes. They do not authenticate the recorded validator responses or establish that the programs enforce every intended rule. The decoded debt fields remain captured observations; this checker does not decode the omitted raw claim receipts. Fresh execution would require the separate program, source, fixture and toolchain inputs.

Recovery observations reference operation keys and revisions present in the retained database history. Their hashes use ordinary JSON serialization; the database digests use canonical JSON with sorted keys. The selected records omit the underlying objects, so the checker verifies those references but does not compare the different hashes or establish recovered-content equality.

The run used software-generated passkey approvals and controlled provider/application identities. Twenty-six recovery processes reported successful reads without sending transactions or requesting new funding or admission authorization. Physical-device approval, an independent provider, arbitrary host-loss recovery, Pool projection and the released customer experience need separate evidence. API exposure still retained the original allocations totaling 6,600, so this run does not establish reservation recycling.

## Files and provenance

- `SELECTED-RECORDS.json` retains selected original report and lifecycle values. Its selection description lists omissions. The signed-but-unsent wire, raw receipt blobs, database dump and large prelaunch seal are excluded.
- `SELECTED-RECEIPT.json` is a readable derived summary. The selected original records supply the inputs for the checker.
- `EXPORT-MANIFEST.json` records file sizes and hashes. It provides consistency checks within this collection; it does not independently certify the publisher or the original run.
- `verify-records.mjs` performs the bounded offline checks. Its v1 envelope interpretation follows the locally inspected public `@solana/transactions` 8.6.0 decoder, distributed under MIT by Anza. It uses Node's Ed25519 implementation and has no package dependencies.

The retained source report hash is `028599afe1fdacf3710c91db0e05adfecb473dc5f5b0d82443a2aa7ce031ea87`. API source is `34a665c9ca3870e5cde4c88549e7f02ae72d2da4`; SDK source is `5863c531bb14a2b05af575933102381ff5f47c74`. The lifecycle records the actual local binary boundary, including the difference from Pro's binaries. The original run's failed predecessors remain preserved privately and are not represented as successful runs here.

Existing Vault-state migration is not required for the new Account experience. That scope decision does not remove the new system's obligations, recovery or complete customer-path requirements.
