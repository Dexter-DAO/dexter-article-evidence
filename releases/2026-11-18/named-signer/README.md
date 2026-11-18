# Payment follows the signer the order names

An order can name who signs for delivery, and payment follows that signature. On November 18, 2026, on the compiled Account programs, the seller was paid 900 when the named signer signed for 900 against the seller's claim of 1,400. When the named signer never signed, nothing was payable and the order's full 2,000 was released. The seller's own report of delivery was refused on such an order, and so was a report signed by any key the order did not name. One order named the buyer as the signer, and the buyer's signature set its bill.

The program checks the report's signature against the key the order names. Whose key that is, a carrier's, the buyer's or an inspector's, is what the two sides agreed when the order was accepted.

The programs ran on a local Solana virtual machine with a synthetic token. The run has 92 signed transactions, and all 577 recorded assertions passed. After each of the 10 refusals, every account the transaction could write, other than the fee payer, held the same bytes as before. [case-index.json](case-index.json) names every transaction, and [named-signer-trace.json.xz](named-signer-trace.json.xz) holds each one's signed bytes, program log and account state.

## The scenario

Six orders for the same goods, each with a maximum of 2,000 at a fixed price of 1 per unit, accepted with ClaimsCore `accept_evidence_financed_tab`. Amounts are base units of the synthetic token. Each order's terms set four times, counted from acceptance: the work must finish within 20 seconds, the named signer may report until 40 seconds, the seller may claim until 45 seconds, and the bill can be finalized from 46 seconds.

The bill is the smallest of three numbers: the order's maximum, the seller's signed claim, and what the named signer's signed report supports. A missing report or a missing claim counts as zero. What the bill leaves unused is released.

In the tables below, "Row" is the zero-based position in the trace's `transactions` array and equals `index` in the case index. "Error" is the name and number in the program log, followed by the program that raised it.

## Six orders

| Order | Signer the order names | Signer signed for | Seller claimed | Bill | Released | Rows: accepted, report, claim, finalized |
|---:|---|---:|---:|---:|---:|---|
| 1 | Delivery signer | 900 | 1,400 | 900 | 1,100 | 59, 71, 73, 83 |
| 2 | Delivery signer | Never signed | 1,400 | 0 | 2,000 | 60, none, 74, 84 |
| 3 | Delivery signer | 900 | 500 | 500 | 1,500 | 61, 66, 75, 85 |
| 4 | Delivery signer | 900, one second late, refused | 1,400 | 0 | 2,000 | 62, 81, 76, 86 |
| 5 | The buyer's owner | 900 | 1,400 | 900 | 1,100 | 63, 72, 77, 87 |
| 6 | Delivery signer | 900, in the last second | 1,400 | 900 | 1,100 | 64, 80, 78, 88 |

In row 91 a key with no role in the order collected order 1's bill. The seller received exactly 900 and the lender's wallet paid exactly 900. Order 2's record shows nothing paid, nothing payable, no debt and 2,000 released. The buyer's budget shows 3,200 consumed, the sum of the four bills, and nothing held.

Before finalization the signed records alone created no debt and nothing payable on any order. One key with no role in the orders submitted the signed records, a second finalized the bills and a third collected, and each of those transactions carries only that key's signature. The named signer and the seller sign their records away from the chain, and each signature travels as an Ed25519 instruction ahead of the ClaimsCore instruction.

## How the order names the signer

For each order at acceptance (rows 59 to 64) the trace records where the signer's key sits:

- The order's bill account holds the whole delivery policy, with the signer's key in `policy.source` and the seller as the only claimant.
- The order record holds the hash of that policy. A policy naming a different signer has a different hash.
- The PrincipalRegistry reservation commits to the same hash.

On orders 1, 2, 3, 4 and 6 the named signer does not sign the order and is not one of its accounts. Its key is in the terms that the agent, the seller and the credit issuer's owner signed.

## What was refused

| Row | Attempt | Outcome | Error |
|---:|---|---|---|
| 65 | The seller reports 900 delivered on order 1 through the seller-only path, `earn_financed_run` | Refused | `BillingProfile` 6059, ClaimsCore |
| 67 | A key no order names signs a report of 900 delivered on order 1 | Refused | `BillingSignature` 6063, ClaimsCore |
| 68 | The seller signs a report of 900 delivered on order 1 | Refused | `BillingSignature` 6063, ClaimsCore |
| 69 | The buyer's owner, whom order 1 does not name, signs a report of 900 delivered on order 1 | Refused | `BillingSignature` 6063, ClaimsCore |
| 70 | The named signer's signed report for order 3 is presented on order 1 | Refused | `BillingSignature` 6063, ClaimsCore |
| 79 | Finalization of order 1 at 0 seconds, while the signer's and the seller's windows are open | Refused | `BillingFinality` 6066, ClaimsCore |
| 81 | The named signer signs 900 on order 4 at 41 seconds, one second after its window closed | Refused | `BillingDeadline` 6065, ClaimsCore |
| 82 | Finalization of order 1 at 45 seconds, the last second of the seller's claim window | Refused | `BillingFinality` 6066, ClaimsCore |
| 89 | Collection of the 1,400 the seller claimed on order 2, which the named signer never signed for | Refused | `FundingRun` 6028, ClaimsCore |
| 90 | Collection of 901 on order 1, one unit above the bill of 900 | Refused | `FundingTerms` 6031, ClaimsCore |

Row 69 and row 72 are the same key. The buyer's owner is refused on order 1, which does not name it, and accepted on order 5, which does. A zero bill creates no work record that can be paid, which is why row 89 fails with `FundingRun`.

## Who holds which key

| Role | Key |
|---|---|
| Buyer's owner | `3BuW9SR5tG6VFK4MmkQQ3Ak8ny1K1Vv5Uz7is8Aa5pwG` |
| Buyer's agent | `Cdkrk8tujFY6mTyGwFgKpnbiGc1hqtXCog1qvUdKAe6D` |
| Seller | `4MfyR4G3NWfVRDWo6iNAHDBZqWMgwZX6FNtMqEW3a9JT` |
| Lender's owner | `6TcyBfPdBt1kjsvDZLzmBFnuMaLWiTaAt4RjUr9VA5YD` |
| Credit issuer's owner | `FMUEmtxhU46GzhKF4FW9MLJdQWiLgjiXP9TYRWSrqTpV` |
| Fee payer of setup and order transactions | `AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9` |
| Named delivery signer of orders 1, 2, 3, 4 and 6 | `GFKfRLPKHYRyARLSTc4p94vukKPMFAkSrQT5j55WYoy2` |
| Key that submits signed records | `92CudvFbL7Tyw2RkWC7NU1ehdNBSjy8FEG1TsJaN8wgz` |
| Key that finalizes bills | `4ap9kAPdHiV82d8D73mrtEgwGxBbUxJgyFr1fRGLhide` |
| Key that collects | `6jQ7iw7szEgN6hp49SAAMagYSYdDAZTQRDF3b8vTLNqM` |
| Key no order names, used in row 67 | `FLCbA5rPHLmgEeQSg1CiewVizRGw6Gy5MXc1m17HyxUG` |

The trace asserts that these eleven keys are all different and that the named delivery signer is none of the other ten. Order 5 names the buyer's owner as its signer.

## The rule in the source

The two files that implement the rule are published here as they stand at the commit the binaries were built from: [evidence_billing.rs](source/foundation/onchain/programs/claims-core/src/evidence_billing.rs) and [billing.rs](source/foundation/onchain/crates/contracts/src/billing.rs). The source calls the named signer the `source` and the seller the `supplier`.

The signer's key is a field of the order's accepted policy.

```rust
// crates/contracts/src/billing.rs
37: pub struct BillingPolicy {
39:     pub source: Pubkey,
```

A delivery report is admitted only inside the signer's window and only with a signature by that key.

```rust
// programs/claims-core/src/evidence_billing.rs
243:     require!(now <= a.bill.policy.evidence_deadline, CoreError::BillingDeadline);
247:     verify_signature(&a.instructions, signature_ix_index, &a.bill.policy.source, &digest)?;
```

`verify_signature` reads the Ed25519 instruction and requires its public key and message to equal the named key and the report's digest.

```rust
// programs/claims-core/src/evidence_billing.rs
226:     require!(d[0] == 1 && d[1] == 0 && word(2) == 16 && word(4) == u16::MAX
227:         && word(6) == 80 && word(8) == u16::MAX && word(10) == 112
228:         && word(12) == 32 && word(14) == u16::MAX
229:         && &d[80..112] == key.as_ref() && &d[112..144] == digest,
230:         CoreError::BillingSignature);
```

Finalization waits for both windows to close, counts a missing report or claim as zero, and takes the smallest of the three numbers.

```rust
// programs/claims-core/src/evidence_billing.rs
289:     require!(a.bill.policy.finalization_ready(now), CoreError::BillingFinality);
292:     let supported = match &a.bill.source_report {
298:         None => 0,
300:     let claimed = match &a.bill.seller_claim {
306:         None => 0,
308:     let amount = deterministic_bill_amount(a.claim.terms.cap, claimed, supported);
// crates/contracts/src/billing.rs
224: pub fn deterministic_bill_amount(cap: u64, seller_claim: u64, supported: u128) -> u64 {
226:     supported.min(cap as u128).min(seller_claim as u128) as u64
```

## Where it ran

The compiled programs ran in LiteSVM from solders 0.29.0 under Python 3.12.3, with signature verification on and native v1 transactions. The token is one synthetic six-decimal SPL token and every key is a disposable test key. The run started on November 18, 2026 at 10:18 AM US Eastern, and the test set the clock to 40, 41, 45 and 46 seconds after the start. It sent no transaction to a public network.

The binaries are the ones the [commit record](../README.md) ran, built from commit `e55c0ad3a224a1c4e72de4c283161345d438caaf` of the Dexter Account repository. The trace records the SHA-256 of all 69 source inputs and of each binary. The two source files above are among those inputs, and their hashes match.

| Program | Address | SHA-256 of the binary |
|---|---|---|
| ClaimsCore | `727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz` | `3ae2dc3c913c3ec696144c8e06104616f82a4d3305ff33e123c418eb446e0bb5` |
| PrincipalRegistry | `9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G` | `92a21620018609eb98f504f0bbfe8370eda6d10081ef1cc1d5c540903bf35214` |
| PoolIssuer | `3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm` | `6804757b94ce2da8854388a1f748b614296207682db27cde85958f868b726e65` |
| Swig | `swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB` | `bb744d9811c392e3f343952d1a99bef9c97aa1936248a23b9d7efc1adb8939c1` |

The other source lines in the case index (`error_source`) refer to the same commit. Its complete source, all 69 inputs, is in [../source/](../source/README.md). The [program source under `evidence/`](../../../evidence/account-evidence/execution/foundation/onchain/programs) is the earlier pin of the November 1 records, `fb2a3a4`, which has no named delivery signer.

## Files

| File | Contents |
|---|---|
| [named-signer-trace.json.xz](named-signer-trace.json.xz) | All 92 transactions: signed bytes, result, program log, and the observed accounts before and after. About 15 MB uncompressed. |
| [case-index.json](case-index.json) | One entry per transaction: name, outcome, error name and number, the program that raised it and the source line its log names. |
| [named-signer-receipt.json](named-signer-receipt.json) | Counts, the trace hashes, the eleven role keys and the binary hashes. |
| [named-signer.log](named-signer.log) | The two lines the run printed. |
| [named_signer.py](named_signer.py) | The test module. It builds on the Account repository's `evidence_billing.py` test and reuses the refusal checks in the commit record's [commit_legs.py](../commit_legs.py). |
| [source/](source) | The two program source files and their [notice](source/NOTICE.md). |
| [provenance.json](provenance.json) | The source commits and the source and published hash of each file. |
| [SHA256SUMS](SHA256SUMS) | Hashes of the nine files above. |

The trace, case index, receipt, test module and both source files are byte-identical to their source commits. In the log, one absolute workspace path is replaced with `<fixture-workspace>`. The source files keep their existing copyright, and this release adds no license grant for them. The trace's `fixture_inputs` lists the hash of every test file the run used.

This folder was added after the [November 18 ZIP](https://github.com/Dexter-DAO/dexter-article-evidence/releases/tag/2026-11-18) was cut. It is in the repository and not in that ZIP.

## Check a row yourself

Download the trace, then print any row's name, result, state comparison and program log. This reads row 68, the seller signing the delivery report:

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
" named-signer-trace.json.xz 68
```

```text
refused: the seller signs the report of 900 units delivered on order 1
expected: reject | error: {'InstructionError': [1, {'Custom': 6063}]}
state unchanged: True
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz invoke [1]
Program log: Instruction: SubmitBillEvidence
Program log: AnchorError thrown in programs/claims-core/src/evidence_billing.rs:226. Error Code: BillingSignature. Error Number: 6063. Error Message: Evidence signature does not use the exact accepted key, message and native framing.
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz consumed 12732 of 1400000 compute units
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz failed: custom program error: 0x17af
```

Line 226 is the check quoted above. `transaction_base64` in the same row is the signed transaction, and `observed_before` and `observed_after` hold each observed account's owner, lamports and data.

To check the files against their hashes, and the decompressed trace against `trace_sha256` in the receipt:

```sh
sha256sum -c SHA256SUMS
xz -dc named-signer-trace.json.xz | sha256sum
```

The second command prints `96888ddd2825b957c09cd6295eb665e39baf492c7253fb4c5948982f5548ba64`.

The [commit record](../README.md) covers the three checks on a single order, and the [shared-reserve record](../shared-reserve/README.md) covers orders that share a budget and a wallet. The [claim guide](../../../articles/the-account-inversion/README.md) maps the article to its records.
