# The records behind Dexter's references

This page quotes the records that [dexter.cash/llms.txt](https://dexter.cash/llms.txt) and the [full reference](https://dexter.cash/llms-full.txt) cite. Line numbers are from the file each section links.

## The Oct 30 mainnet record

On Solana mainnet on Oct 30, 2026, Dexter's Vault program, against demonstration stores Dexter operated, refused four attempts, and the store collected the same claim afterward. [findings.txt](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/historical-evidence/findings.txt) states the record:

```text
// evidence/historical-evidence/findings.txt
11: The payout-redirect attempt and the second successful purchase name the same
12: claim: 49Bg9FJx2VC73udyvN6EFXhwDxbV5yVsKJCWHrCrU3fz
13: The record retains DebtTransferMismatch (6070) at slot 463186079 for the
14: attempted diversion. Its later settlement reference pays the approved seller.
16: Refused diversion:
17: https://explorer.solana.com/tx/4MMw5EVLvGK6yQBrGUWRJxNH4LudxFkZRcKdkayUUxxpjzLMPHnE8Z9QjBD6U8niYuNn5yxjR99Q9XHPn5DLFxXg
18: Subsequent collection of that same claim:
19: https://explorer.solana.com/tx/QNb41FGAxugzUWaWiKjj8vsouA2sLngHVftaXuycR4JmBxMg5CetYmUGVwmymuo7hCF3P3h8DkSmKXJkg96eN8h
21: This is the concrete payment-binding example: an accepted claim cannot be
22: settled by substituting a different recipient. The pinned verifier checks the
23: signed Swig payout instructions, token balance changes, claim closure, exact
24: refusal log, and transaction ordering. Its same-claim check is here:
25: https://github.com/Dexter-DAO/dexter-x402-sdk/blob/43980c8daaa5e3067681b9e3738b7bbb968ea200/examples/verify-case-three/verify.mjs#L362-L391
```

Rows in [evidence-index.json](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/historical-evidence/evidence-index.json), which carries each explorer link:

| Role | Recorded error | Slot |
|---|---|---:|
| "no-permission" | 3012, AccountNotInitialized | 463185024 |
| "payout-redirect" | 6070, DebtTransferMismatch | 463186079 |
| "permission refusal" | 6013, RevolvingCapacityExceeded | 463186436 |
| "permission refusal" | 6009, SessionExpiryInPast | 463226899 |

The Nov 4, 2026 checks, from lines 29 to 40 of findings.txt:

- ["21 of 21 checks passed against the exact published record, with network access disabled."](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/historical-evidence/results/record-offline-checks.json)
- ["4 of 4 additional tests passed against copies of that record."](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/historical-evidence/results/offline-record-mutations.tap)
- ["82 of 82 tests passed in the unchanged public verifier suite."](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/historical-evidence/results/verifier-tests.tap)

The [case page](https://dexter.cash/mandate?case=b0ee0521-898e-828b-9c14-265bfb59216c) shows the same record, and [record.json](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/historical-evidence/record.json) is "the complete 49,826-byte public response."

## The decoded Account sequence

Compiled Account programs on a local SVM, with synthetic assets, recorded this order on Nov 1, 2026. [article-claim-check-20261104.json](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/checks/article-claim-check-20261104.json) decodes it from the [retained shared trace](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/account/docs/evidence/wallet-spending-20261101/wallet-spending-shared-trace.json.xz). Indexes are zero-based.

| Index | Expected | Name | Recorded result | Signatures | Signers |
|---:|---|---|---|---:|---|
| 69 | reject | "fresh authorization still refuses spending another facility's backing" | Error 6033 | 1 | AKnL4... |
| 72 | reject | "a separate exact owner approval cannot spend one committed token" | Error 6033 | 1 | AKnL4... |
| 74 | success | "owner revokes the initiating agent after work was earned" | null | 2 | AKnL4..., 3BuW9... |
| 79 | success | "both owners authorize financier control rotation" | null | 3 | AKnL4..., 6JhaG..., 6TcyB... |
| 81 | success | "independent collector services accepted rights without initiating agent or financier backend" | null | 1 | AKnL4... |

Signers are shortened here. The full addresses are in the file quoted below.

```text
// evidence/account-evidence/checks/article-claim-check-20261104.json
75:   "shared_custody": "EMPX3MrhzgnWTi9jjwfCQ5b2waoHomSVULtwRiTpCvym",
76:   "facilities_sharing_custody": [
77:     "8TgjkCEtZMkkPFUrKSXN4Qq2h6261Bmj7yjD3hszpW5K",
78:     "CUGbyGhek7TRMoLAVgA4FCRkqLZnwqznhXe7angDGDXj"
79:   ],
82:     "signer": "AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9",
83:     "signer_is_initiating_agent_or_old_or_new_financier_owner": false,
84:     "actual_token_cash_before": 300000,
85:     "actual_token_cash_after": 200000,
86:     "unused_backing_before": 200000,
87:     "unused_backing_after": 200000,
88:     "supplier_payable_before": 100000,
89:     "supplier_payable_after": 0,
90:     "company_financing_debt_before": 100000,
91:     "company_financing_debt_after": 100000,
```

The backing sits in a financier's wallet. Rows 51 to 53 of the same trace are "create and bind exact governed financier wallet", "create financier custody token account" and "prefund financier token account before explicit allocation".

From line 5 of the [Account evidence guide](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/README.md):

> Two facilities share backing. Attempts to spend committed funds fail. The owner revokes the initiating agent and cooperatively rotates the financier's owner authority. A separate collector then pays the supplier with one transaction signature. The remaining backing stays protected and the company's financing debt stays outstanding.

## Fifteen recorded scenarios

[offline-check-20261104.json](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/checks/offline-check-20261104.json) holds the Nov 4, 2026 check of all 15 retained scenarios. Each matched its retained verifier result.

| Scenario | Signed packets | Financial receipts | Refusals with rollback | Recorded-state assertions |
|---|---:|---:|---:|---:|
| financing-commercial | 114 | 25 | 18 | 337 |
| financing-corrections-interest-alias | 68 | 11 | 1 | 120 |
| financing-corrections-lifecycle | 100 | 20 | 8 | 510 |
| financing-corrections-own-capital | 69 | 13 | 0 | 222 |
| financing-corrections-ownership-escrow | 92 | 22 | 6 | 636 |
| financing-corrections-ownership | 93 | 22 | 6 | 634 |
| financing-corrections-repaid-uncollected | 69 | 16 | 0 | 404 |
| financing-governed-repayment-facility | 84 | 17 | 2 | 280 |
| financing-governed-repayment-refunds | 90 | 16 | 3 | 277 |
| financing-governed-repayment-roles | 79 | 17 | 0 | 207 |
| financing-governed-repayment-run | 102 | 15 | 15 | 282 |
| financing-wallet | 146 | 30 | 22 | 437 |
| wallet-spending-send | 108 | 14 | 21 | 268 |
| wallet-spending-shared | 90 | 15 | 4 | 230 |
| wallet-spending-swap | 102 | 19 | 9 | 269 |
| Total | 1,406 | 272 | 115 | 5,113 |

```text
// evidence/account-evidence/checks/offline-check-20261104.json
3:   "status": "passed",
4:   "checked_at_utc": "2026-11-04T06:13:36.324950+00:00",
253:     "verified_spending_operations": 12,
254:     "verified_actual_adapter_swaps": 4,
```

## Billing after revocation

The Nov 6 release's billing run is a local SDK/RPC run on a local validator, with synthetic source facts and separate signers, in fixture units. [joined-rpc-receipt.json](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/releases/2026-11-06/billing/joined-rpc-receipt.json), line 14: "Admission, revocation, detached record submission, finalization and collection below use real SDK native-v1 packets and finalized RPC."

| Lot | Cap | Supplier claim | Source facts support | Bill established | Released | Buyer debt |
|---|---:|---:|---:|---:|---:|---:|
| earned | 2,000 | 1,400 | 900 | 900 | 1,100 | 900 |
| missing-claim | 2,000 | None | 900 | 0 | 2,000 | 0 |
| missing-both | 2,000 | None | 0 | 0 | 2,000 | 0 |

```text
// releases/2026-11-06/billing/joined-rpc-receipt.json
155:       "paid": "900"
204:   "balancesBefore": {
208:     "financier": "1000000",
209:     "supplier": "0"
211:   "balancesAfter": {
215:     "financier": "999100",
216:     "supplier": "900"
247:       "active": false
249:     "beforeAnyEvidenceOrClaim": true
264:       "Program log: AnchorError thrown in programs/principal-registry/src/lib.rs:213. Error Code: CredentialInactive. Error Number: 6004. Error Message: Credential is inactive."
268:     "protectedAccountsUnchanged": true
551:   "finalizedTransactions": 11,
552:   "distinctRecordedOperations": 10,
553:   "freshProcessRecoveries": 20,
554:   "recoveryRebroadcasts": 0,
```

The initiating actor's key, line 243, is Cdkrk8tujFY6mTyGwFgKpnbiGc1hqtXCog1qvUdKAe6D. The three acceptances carry it among four signers. These transactions carry one other signer each:

| Transaction | Slot | Signer |
|---|---:|---|
| revoke-initiating-actor | 172 | 3BuW9SR5tG6VFK4MmkQQ3Ak8ny1K1Vv5Uz7is8Aa5pwG |
| earned-evidence | 205 | 92CudvFbL7Tyw2RkWC7NU1ehdNBSjy8FEG1TsJaN8wgz |
| earned-claim | 243 | ArgGAMvAQEEaM1TiehNy83bbL5SACZLvh6BZDHTUvhuQ |
| earned-finalization | 626 | 4ap9kAPdHiV82d8D73mrtEgwGxBbUxJgyFr1fRGLhide |
| earned-collection | 743 | 9XJfNJ5Hej7WDSDs26QqxXC2DST38YgNMwi3NB8NvWJn |

## Passkey authority alongside recovery

A local run with software-generated P-256 keys recorded same-Principal adoption, company authority and a financed admission with two P-256 approvals. Rows from [passkey-rpc-receipt.json](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/releases/2026-11-06/recovery/passkey-rpc-receipt.json), all finalized:

| Case | Wire bytes |
|---|---:|
| "adopt-company-alias" | 1,027 |
| "adopt-issuer-alias" | 1,027 |
| "adopt-supplier-alias" | 1,027 |
| "company-passkey-creates-current-actor" | 905 |
| "supplier-passkey-opens-receiving-custody" | 1,062 |
| "company-passkey-approves-exact-custody-mandate" | 1,003 |
| "two-p256-financed-admission" | 3,726 |

```text
// releases/2026-11-06/recovery/passkey-rpc-receipt.json
168:     "casesPassed": 13,
169:     "assertionsPassed": 223,
170:     "finalizedTransactions": 11,
180:       "P256Proofs": 2,
197:       "budgetReservedAfter": "2200",
199:       "custodyUnusedAfter": "2000",
200:       "existingTokenAccountsUnchanged": true,
201:       "existingFinancialStateUnchangedAcrossAdoption": true,
202:       "principalAndSeedOwnerPreserved": true
```

## One purchase from admission to collection

On local Agave, in fixture units, with controlled provider and application identities and software passkeys, the API, PostgreSQL, SDK and native programs ran one purchase. [SELECTED-RECEIPT.json](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/releases/2026-11-06/continuing-api/SELECTED-RECEIPT.json) summarizes it, and [SELECTED-RECORDS.json](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/releases/2026-11-06/continuing-api/SELECTED-RECORDS.json) holds the original values. The summary lists 16 transactions, each with a null error. The purchase's steps:

| Label | Slot |
|---|---:|
| "native-api-admission" | 282 |
| "two-passkey-api-admission" | 566 |
| "independent-native-actor-revocation" | 745 |
| "retained-provider-native-source-record" | 826 |
| "original-supplier-passkey-claim" | 937 |
| "native-earned-independent-finalization" | 1,683 |
| "passkey-zero-independent-finalization" | 1,782 |
| "independent-api-governed-collection" | 1,872 |

```text
// releases/2026-11-06/continuing-api/SELECTED-RECEIPT.json
167:     "before_cash": {
168:       "financier": "1000000",
169:       "supplier": "0"
171:     "after_cash": {
172:       "financier": "999100",
173:       "supplier": "900"
175:     "earned": {
178:       "debt": "900",
179:       "paid": "900",
181:       "released": "1100",
185:     "unused_bill": {
191:       "released": "2000",
196:   "recovery": {
197:     "distinct_processes": 26,
200:     "all_passed": true,
201:     "send_requests": 0,
202:     "funding_requests": 0,
203:     "admission_authorizations": 0
```

[verify-records.mjs](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/releases/2026-11-06/continuing-api/verify-records.mjs) checks the 16 retained transaction wires, their Ed25519 signatures and these amounts offline.

## Check the Oct 30 record

From `evidence/historical-evidence/` in the [dated ZIP](https://github.com/Dexter-DAO/dexter-article-evidence/releases/tag/2026-11-06b), with Node 24 or newer:

```sh
shasum -a 256 -c SHA256SUMS
cd verifier
npm ci --ignore-scripts --no-audit --no-fund
npm test
cd ..
node checks/check-record-offline.mjs
node --test checks/offline-record.test.mjs
```

After the one install, both test suites and the record checker run locally. To read the chain through any archival Solana RPC:

```sh
cd verifier
node verify.mjs ../record.json --rpc https://YOUR-ARCHIVAL-SOLANA-RPC
```

That command is read-only. The commands come from [README.txt](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/historical-evidence/README.txt).

## Source notices and approval

Files under `vault/` retain the original MIT license, per line 3 of [NOTICE.md](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/NOTICE.md). Account source retains its existing copyright, per line 33 of the [package README](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/README.txt). [APPROVED-PACKAGE.json](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/APPROVED-PACKAGE.json) records the approved archive:

```text
// APPROVED-PACKAGE.json
2:   "archive": "dexter-article-evidence-2026-11-04.zip",
4:   "sha256": "e789a59a553c23488007befa3678032373ab1e68bf41462a92a5da10eadd33f2",
7:   "approved_at": "2026-11-04T07:56:30.603699+00:00",
8:   "preservation": "Evidence directory matches the exact package approved for publication."
```
