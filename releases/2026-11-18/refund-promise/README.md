# A seller holds its own money for a refund and a named decider releases it

A seller can hold its own money for a refund and let a named decider release it. On November 18, 2026, on the compiled Account programs, a seller put 200,000 of its own cash in its own wallet and, alongside each of two purchases of 100,000, promised up to 100,000 back to the buyer, naming a decider that is none of the parties. While the promises were open the seller could not withdraw 1 unit. A refund signed by the seller, or by the buyer, in the decider's place was refused. The decider signed for 30,000 on the first promise: 30,000 went from the seller's wallet to the buyer and 70,000 was released. The decider never signed on the second: nothing was paid and 100,000 was released.

The promise is a second order in the opposite direction that the two sides accept alongside the purchase. The program does not tie the two together: the only link is a hash in the promise's terms, which the program does not read. A credit issuer takes part in the promise, and a paid refund leaves the seller owing that issuer 30,000 and the issuer owing the seller, as the lender of its own promise, the same 30,000.

The programs ran on a local Solana virtual machine with a synthetic token. The run has 86 signed transactions, and all 325 recorded assertions passed. After each of the 6 refusals, every account the transaction could write, other than the fee payer, held the same bytes as before. [case-index.json](case-index.json) names every transaction, and [refund-promise-trace.json.xz](refund-promise-trace.json.xz) holds each one's signed bytes, program log and account state.

## How it is composed

| | Purchase | Refund promise |
|---|---|---|
| Buyer of record | The buyer | The seller |
| Seller of record, who is paid | The seller | The original buyer |
| Maximum | 100,000 | 100,000 |
| Backed by | A lender's wallet | The seller's own cash in the seller's own wallet |
| Accepted with | `accept_financed_tab` | `accept_evidence_financed_tab`, naming the decider |
| What sets the amount paid | The seller's own report | The decider's signed report, capped by the original buyer's signed claim |
| Signed by | The buyer's agent, the credit issuer's owner and the seller | The seller's agent, the credit issuer's owner and the original buyer |

The seller's principal is the lender of the facility that backs its promises, and a second principal is that facility's credit issuer (rows 62 and 63). Each promise's terms let the decider sign until 40 seconds after acceptance and the original buyer claim until 45 seconds. Amounts are base units of the synthetic token.

In the tables below, "Row" is the zero-based position in the trace's `transactions` array and equals `index` in the case index. Rows 0 to 60 and row 64 are setup. "Error" is the name and number in the program log, followed by the program that raised it.

## Two pairs

| Pair | Decider signed for | Original buyer claimed | Refund | Released to the seller | Rows: purchase, promise, decider's report, buyer's claim, finalized, refund collected |
|---:|---:|---:|---:|---:|---|
| 1 | 30,000 | 100,000 | 30,000 | 70,000 | 65, 66, 77, 78, 80, 84 |
| 2 | Never signed | 100,000 | 0 | 100,000 | 67, 68, none, 79, 81, none |

The other accepted steps:

| Row | What happened | Outcome |
|---:|---|---|
| 61 | The seller's owner moves 200,000 of the seller's own cash into the seller's own wallet | Accepted |
| 62 | The seller's owner and the credit issuer's owner approve a facility in which the seller is the lender | Accepted |
| 63 | The seller's owner and the credit issuer's owner open the seller's credit line on that facility | Accepted |
| 70, 72 | The seller reports 100,000 delivered on purchase 1 and on purchase 2 | Accepted |
| 71, 73 | A key with no role collects each purchase's 100,000 from the lender's wallet to the seller | Paid |
| 85 | The seller's owner withdraws the 170,000 left in the seller's wallet after both promises closed | Accepted |

What the trace records around these rows:

- With both promises accepted, the seller's wallet holds 200,000 of the seller's own cash, all of it promised and none free.
- The seller is paid 200,000 from the lender's wallet for the two purchases, and its own wallet still holds the promised 200,000.
- The refund in row 84 is exactly one token transfer: 30,000 from the seller's wallet token account to the original buyer's payout token account. The lender's wallet and the seller's own token account did not change.
- After the refund the buyer's debt on purchase 1 is still 100,000. The refund is 30,000 of tokens paid to the buyer.
- The seller ends with 370,000: its own 200,000, plus 200,000 for the two purchases, less the 30,000 refund.

## What was refused

| Row | Attempt | Outcome | Error |
|---:|---|---|---|
| 69 | The seller's owner withdraws 1 unit from the seller's wallet while both promises are open | Refused | `FundingCashShortfall` 6033, ClaimsCore |
| 74 | The seller signs a refund of 30,000 on promise 1 in the decider's place | Refused | `BillingSignature` 6063, ClaimsCore |
| 75 | The original buyer signs a refund of 30,000 on promise 1 in the decider's place | Refused | `BillingSignature` 6063, ClaimsCore |
| 76 | Finalization of promise 1 at 0 seconds, before the decider's window has closed | Refused | `BillingFinality` 6066, ClaimsCore |
| 82 | Collection to the original buyer of the 100,000 it claimed on promise 2, which the decider never signed for | Refused | `FundingRun` 6028, ClaimsCore |
| 83 | Collection to the original buyer of 30,001 on promise 1, one unit above the refund the decider signed | Refused | `FundingTerms` 6031, ClaimsCore |

## What links a promise to its purchase

Each promise's bill account stores a `request_hash` in its policy. In this run it is the SHA-256 of the text `refund promise for purchase record ` followed by the address of the purchase's order record. The program requires the hash to be non-zero and requires the decider's report to carry the same hash. It does not check that the hash refers to a purchase. The purchase's record is not an account of the promise's acceptance, and the purchase's record carries no reference to the promise. Purchase and promise are accepted in separate transactions.

## Who holds which key

| Role | Key |
|---|---|
| Original buyer's owner | `3BuW9SR5tG6VFK4MmkQQ3Ak8ny1K1Vv5Uz7is8Aa5pwG` |
| Original buyer's agent | `Cdkrk8tujFY6mTyGwFgKpnbiGc1hqtXCog1qvUdKAe6D` |
| Seller's owner | `4MfyR4G3NWfVRDWo6iNAHDBZqWMgwZX6FNtMqEW3a9JT` |
| Seller's agent | `3MZskhKUdNRkeMQ6zyNVSJcCx38o79ohwmSgZ2d5a4cu` |
| Lender's owner | `6TcyBfPdBt1kjsvDZLzmBFnuMaLWiTaAt4RjUr9VA5YD` |
| Credit issuer's owner | `FMUEmtxhU46GzhKF4FW9MLJdQWiLgjiXP9TYRWSrqTpV` |
| Decider | `GFKfRLPKHYRyARLSTc4p94vukKPMFAkSrQT5j55WYoy2` |
| Fee payer of setup and order transactions | `AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9` |
| Key that submits signed records | `92CudvFbL7Tyw2RkWC7NU1ehdNBSjy8FEG1TsJaN8wgz` |
| Key that finalizes | `4ap9kAPdHiV82d8D73mrtEgwGxBbUxJgyFr1fRGLhide` |
| Key that collects | `6jQ7iw7szEgN6hp49SAAMagYSYdDAZTQRDF3b8vTLNqM` |

The trace asserts that these eleven keys are all different. The decider is none of the parties.

## The rule in the source

The decider is the signer the promise names, so the rule is the one in the [named-signer record](../named-signer/README.md): `submit_evidence`, line 234, `verify_signature`, line 216, and `finalize`, line 276, of [evidence_billing.rs](../source/foundation/onchain/programs/claims-core/src/evidence_billing.rs), and `deterministic_bill_amount`, line 224 of [billing.rs](../source/foundation/onchain/crates/contracts/src/billing.rs). The `request_hash` is line 44 of `billing.rs`, and the check that a report carries it is at line 171. The withdrawal in row 69 is refused in `debit_uncommitted`, line 93 of [funding_state.rs](../source/foundation/onchain/programs/claims-core/src/funding_state.rs).

## Where it ran

The compiled programs ran in LiteSVM from solders 0.29.0 under Python 3.12.3, with signature verification on and native v1 transactions. The token is one synthetic six-decimal SPL token and every key is a disposable test key. The run started on November 18, 2026 at 10:50 AM US Eastern, and the test set the clock to 30 and 46 seconds after the start. It sent no transaction to a public network.

The binaries are the ones the [commit record](../README.md) ran, built from commit `e55c0ad3a224a1c4e72de4c283161345d438caaf` of the Dexter Account repository. The trace records the SHA-256 of all 69 source inputs and of each binary, and the [complete source](../source/README.md) at that commit is published beside this folder.

| Program | Address | SHA-256 of the binary |
|---|---|---|
| ClaimsCore | `727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz` | `3ae2dc3c913c3ec696144c8e06104616f82a4d3305ff33e123c418eb446e0bb5` |
| PrincipalRegistry | `9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G` | `92a21620018609eb98f504f0bbfe8370eda6d10081ef1cc1d5c540903bf35214` |
| PoolIssuer | `3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm` | `6804757b94ce2da8854388a1f748b614296207682db27cde85958f868b726e65` |
| Swig | `swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB` | `bb744d9811c392e3f343952d1a99bef9c97aa1936248a23b9d7efc1adb8939c1` |

The source lines in the case index (`error_source`) are lines of those source files.

## Files

| File | Contents |
|---|---|
| [refund-promise-trace.json.xz](refund-promise-trace.json.xz) | All 86 transactions: signed bytes, result, program log, and the observed accounts before and after. About 11 MB uncompressed. |
| [case-index.json](case-index.json) | One entry per transaction: name, outcome, error name and number, the program that raised it and the source line its log names. |
| [refund-promise-receipt.json](refund-promise-receipt.json) | Counts, the trace hashes, the eleven role keys, the seller's wallet addresses and the binary hashes. |
| [refund-promise.log](refund-promise.log) | The 2 lines the run printed. |
| [refund_promise.py](refund_promise.py) | The test module. It builds on the Account repository's `evidence_billing.py` test and reuses the refusal checks in the commit record's [commit_legs.py](../commit_legs.py). |
| [provenance.json](provenance.json) | The source commit and the source and published hash of each file. |
| [SHA256SUMS](SHA256SUMS) | Hashes of the six files above. |

The trace, case index, receipt and test module are byte-identical to the source commit. In the log, one absolute workspace path is replaced with `<fixture-workspace>`. The trace's `fixture_inputs` lists the hash of every test file the run used.

## Check a row yourself

Download the trace, then print any row's name, result, state comparison and program log. This reads row 74, the seller signing a refund in the decider's place:

```sh
python3 -c "
import json, lzma, sys
t = json.loads(lzma.open(sys.argv[1]).read())
x = t['transactions'][int(sys.argv[2])]
r = x['result']
print(x['name'])
print('expected:', x['expected'], '| error:', r.get('err'))
print('state unchanged:', x['observed_before'] == x['observed_after'])
print('\n'.join((r.get('meta') or r).get('logs') or []))
" refund-promise-trace.json.xz 74
```

```text
refused: the seller signs a refund of 30,000 on promise 1 in the decider's place
expected: reject | error: {'InstructionError': [1, {'Custom': 6063}]}
state unchanged: True
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz invoke [1]
Program log: Instruction: SubmitBillEvidence
Program log: AnchorError thrown in programs/claims-core/src/evidence_billing.rs:226. Error Code: BillingSignature. Error Number: 6063. Error Message: Evidence signature does not use the exact accepted key, message and native framing.
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz failed: custom program error: 0x17af
```

The lines left out here are system program calls and compute-unit counts. `transaction_base64` in the same row is the signed transaction, and `observed_before` and `observed_after` hold each observed account's owner, lamports and data.

To check the files against their hashes, and the decompressed trace against `trace_sha256` in the receipt:

```sh
sha256sum -c SHA256SUMS
xz -dc refund-promise-trace.json.xz | sha256sum
```

The second command prints `0ae4dfabfab16fc885b5df03f9469609aed24801ea4fe30a93bfd5070344157a`.

The [named-signer record](../named-signer/README.md) covers the same signature rule on a delivery. The [claim guide](../../../articles/the-account-inversion/README.md) maps the article to its records.
