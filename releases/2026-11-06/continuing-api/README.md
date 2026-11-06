# One purchase from admission to collection, across API, database, SDK and programs

This collection contains records from the November 5 continuing-purchase test, published in the November 6 evidence release. The run connected Account programs, SDK, API and PostgreSQL through admission, provider evidence, a supplier claim, finalization and collection, on local Agave, with software-generated passkey approvals and controlled provider and application identities.

The recorded collection paid the original supplier custody 900 atomic fixture-token units. The buyer retained 900 units of debt. An unused second bill released its 2,000-unit cap without debt.

Run the checker with Node.js 22 or later:

```sh
node verify-records.mjs
```

The checker runs offline on Node's built-in libraries. It checks the selected file hashes, 16 retained transaction wires, Ed25519 signatures, captured transaction identifiers and account keys, recorded payment arithmetic and recovery observations.

Recovery observations reference operation keys and revisions present in the retained database history. Their hashes use ordinary JSON serialization, and the database digests use canonical JSON with sorted keys.

Twenty-six recovery processes reported successful reads without sending transactions or requesting new funding or admission authorization.

## Files and provenance

- `SELECTED-RECORDS.json` retains selected original report and lifecycle values.
- `SELECTED-RECEIPT.json` is a readable derived summary, [quoted here](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/excerpts/records.md#one-purchase-from-admission-to-collection). The selected original records supply the inputs for the checker.
- `EXPORT-MANIFEST.json` records file sizes and hashes.
- `verify-records.mjs` performs the offline checks. Its v1 envelope interpretation follows the locally inspected public `@solana/transactions` 8.6.0 decoder, distributed under MIT by Anza. It uses Node's Ed25519 implementation and has no package dependencies.

The retained source report hash is `028599afe1fdacf3710c91db0e05adfecb473dc5f5b0d82443a2aa7ce031ea87`. API source is `34a665c9ca3870e5cde4c88549e7f02ae72d2da4`, and SDK source is `5863c531bb14a2b05af575933102381ff5f47c74`.
