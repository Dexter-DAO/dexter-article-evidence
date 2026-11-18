# A bill settles once both records are in, and a program can sign for delivery

A delivery-billed order settles as soon as the delivery report and the seller's claim are both in. On November 18, 2026, on compiled Account programs, the carrier signed for 900 units delivered on an order with a maximum of 2,000 and the seller claimed 1,400. With both records in and both windows still open, a key with no role in the order finalized the bill at 900 and released 1,100, and in the next transaction another such key collected 900 from the lender's wallet to the seller. With only the report or only the claim in, finalization was refused until both windows had closed, and the missing record then counted as zero.

An order can also name a program as its delivery signer. The order was accepted when the program had no upgrade authority and refused when the same program had one. Only that program's signature admitted the report: the carrier's key was refused on that order, and the program's signature was refused on orders that named a key or a Registry principal. An owner pinned the program's address as the delivery signer, and an order naming another address was refused.

| Run | Row | What happened | Outcome | Error |
|---|---:|---|---|---|
| Main | 59 | The carrier signs 900 units delivered on order 1 at 0 seconds | Accepted |  |
| Main | 60 | The seller signs a claim of 1,400 on order 1 at 0 seconds | Accepted |  |
| Main | 61 | A key with no role finalizes order 1 at 0 seconds, with both windows open: bill 900, 1,100 released | Accepted |  |
| Main | 62 | A key with no role collects order 1's 900 from the lender's wallet to the seller at 0 seconds | Paid |  |
| Main | 95 | Finalization of order 2 at 0 seconds: the carrier's report is in, the seller has not claimed | Refused | `BillingFinality` 6066, ClaimsCore |
| Main | 97 | Finalization of order 3 at 0 seconds: the seller's claim is in, the carrier has not signed | Refused | `BillingFinality` 6066, ClaimsCore |
| Main | 66 | Program order 1 names the rule program that has an upgrade authority | Refused | `BillingSourceProgram` 6078, ClaimsCore |
| Main | 75 | Program order 1, maximum 2,000, names the rule program that has no upgrade authority | Accepted |  |
| Main | 78 | The carrier signs the report on program order 1 with its own key | Refused | `BillingSignature` 6063, ClaimsCore |
| Main | 84 | The rule program signs 900 units delivered on program order 1 | Accepted |  |
| Main | 89 | A key with no role collects program order 1's 900 from the lender's wallet to the seller | Paid |  |
| Main | 91 | Agent 2's program order names an address other than the one its owner pinned | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |

The programs ran on a local Solana virtual machine with a synthetic token. The record has two runs with 172 signed transactions, and all 633 recorded assertions passed. After each of the 23 refusals in the tables below, every account the transaction could write, other than the fee payer, held the same bytes as before. [case-index.json](case-index.json) names every transaction of the two runs, and the two traces hold each one's signed bytes, program log and account state.

## When a bill can be finalized

An evidence order's bill is the smallest of three numbers: the order's maximum, the seller's signed claim and what the delivery signer's signed report supports. Anyone can finalize it, and finalization makes the bill payable.

- **Both records in: at once.** `finalize_evidence_bill` runs as soon as the delivery report and the seller's claim are both admitted, or after both admission windows close.
- **Each record is admitted once.** An exact resubmission changes nothing, and a different one is refused with `BillingReplay`, before and after settlement. The amount is therefore the same whenever the bill is finalized.
- **A missing record still waits for the windows.** With only the report or only the claim in, finalization is refused with `BillingFinality` until the last window closes. After that, a missing record counts as zero.
- **Interest starts at finalization.** Finalization starts the buyer's debt and its interest, so a bill finalized early accrues interest from that earlier moment, up to the accepted `accrual_cutoff` and within the run's share of `maximum_interest`.

```text
bill = min(maximum, seller claim, supported)
finalized when the report and the claim are both in, or after both windows close
```

## How an order names a program

An order names its delivery signer in its billing policy, `policy.source`. In the [named-signer record](../named-signer/README.md) that is a key, and a recoverable order names a Registry principal. An order can also name a program.

- **The order names the program's address.** ClaimsCore's `accept_program_sourced_financed_tab` takes the accounts and arguments of `accept_evidence_financed_tab`, the rule program and its ProgramData, and an optional list of terms. ClaimsCore derives the address `["delivery-source", policy.source_identity]` from the rule program shown and requires it to equal `policy.source`.
- **The program has no upgrade authority.** The rule program must be executable, owned by the upgradeable loader and point to the ProgramData shown, and that ProgramData (loader tag 3; a buffer is refused) must hold no upgrade authority. Otherwise the order fails with `BillingSourceProgram`.
- **Only the program's signature admits its report.** `submit_program_bill_evidence` requires the bill's delivery-source address to sign, which only the rule program can do, by a call from inside the program. ClaimsCore derives the address again from the rule program shown. The report digest, the deadline and the replay rule are those of `submit_bill_evidence`.
- **The paths do not cross.** A key's Ed25519 signature on a program-sourced order is refused with `BillingSignature`, and the principal report instruction finds no authority binding for it. The program path on an order that names a key or a Registry principal is refused with `BillingSignature`. The plain evidence instruction refuses a delivery signer that is not an Ed25519 key with `BillingPolicy`.
- **An owner can pin the program.** The Registry checks a shown list as on the other pinned evidence instructions, so an owner pins `delivery_signer` to the rule program's address, and an order naming another address is refused with `TermsNotPermitted`.

```text
delivery source = find_program_address(["delivery-source", policy.source_identity], rule program)
ProgramData     = [3,0,0,0] || slot (8 bytes) || upgrade authority tag (1 byte, 0 = none) || ...
```

| Value used in the runs | |
|---|---|
| Rule program, no upgrade authority | `uY46ibBrA5pEsx4z8iiQ6BpNeBuMR6weo4u2ubREB9q` |
| The same program with upgrade authority `Gymcte79ALGtFrWybRxqb7M7uRjnbjgaHjGgQf1v4qB8` | `8ESb6S2iPPgUes97tCQJPn7PTNbkWtfQLGqKgFXyvwrr` |
| `source_identity` | `98ef73239ea5dd0aff93d084bb91cd3b67116779e6a9d63af34d71342d079fdc` |
| Delivery-source address of the rule program | `9j3Dn1ix4qEc4g4f1rDRWNgfm8c4DgmVxHQNNorR4hxN` |

The rule program in this record is the smallest program that can sign for an order: it signs any report its caller passes. Its 23 lines are in [source/](source/foundation/test-programs/delivery-rule/src/lib.rs). It was loaded twice from the same binary, once with no upgrade authority and once with one, and the trace asserts the loader records of each copy.

## The scenario

The cast of the [pinned-price record's](../pinned-price/README.md#who-holds-which-key) main run: one buyer, two agents, one seller, the carrier, one credit issuer and one lender, with three more keys that submit the signed records, finalize and collect. Evidence orders have a maximum of 2,000 at a fixed price of 1 per unit. Work must finish within 20 seconds of acceptance, the delivery signer may report until 40 seconds and the seller may claim until 45. Wherever both are present, the delivery signer signs 900 units and the seller claims 1,400, so a finalized bill is 900 and 1,100 is released. Amounts are base units of the synthetic token.

| Order | Delivery signer | Records in before finalization | Finalized at | Bill |
|---|---|---|---:|---:|
| 1 | The carrier's key | Report and claim, at 0 seconds | 0 seconds | 900, paid at 0 seconds |
| 2 | The carrier's key | Report only | 46 seconds | 0 |
| 3 | The carrier's key | Claim only | 46 seconds | 0 |
| 4 | The carrier's key | Report at 0 seconds, claim at 45 seconds | 46 seconds | 900, paid |
| Program order 1 | The rule program | Report and claim, at 0 seconds | 0 seconds | 900, paid at 0 seconds |
| Agent 2's program order | The rule program, pinned by the owner | | | Accepted |
| Recoverable order 1 | The source's Registry principal | Report and claim, at 0 seconds | 0 seconds | 900 |

In the tables below, "Row" is the zero-based position in a trace's `transactions` array and equals `index` in the case index for that run's `suite`. "Error" is the name and number in the program log, followed by the program that raised it. An order refusal raised in the Registry reaches it through ClaimsCore, so the Registry fails first and ClaimsCore fails with the same code.

## Both records in: the bill settles at once

Trace: [early-settlement-trace.json.xz](early-settlement-trace.json.xz), suite `early-settlement`.

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 55 | Evidence order 1, maximum 2,000, naming the carrier | Accepted |  |
| 56 | Evidence order 2, maximum 2,000, naming the carrier | Accepted |  |
| 57 | Evidence order 3, maximum 2,000, naming the carrier | Accepted |  |
| 58 | Evidence order 4, maximum 2,000, naming the carrier | Accepted |  |
| 59 | The carrier signs 900 units delivered on order 1 at 0 seconds | Accepted |  |
| 60 | The seller signs a claim of 1,400 on order 1 at 0 seconds | Accepted |  |
| 61 | A key with no role finalizes order 1 at 0 seconds, with both windows open: bill 900, 1,100 released | Accepted |  |
| 62 | A key with no role collects order 1's 900 from the lender's wallet to the seller at 0 seconds | Paid |  |
| 63 | Order 1 is finalized again | No effect |  |
| 64 | The carrier's exact report on order 1 is sent again after settlement | No effect |  |
| 65 | The carrier signs 1,000 units on settled order 1, which already holds its report of 900 | Refused | `BillingReplay` 6064, ClaimsCore |

For order 1 the trace asserts that the bill was finalized at the clock of row 61, before the delivery signer's window closed, and that the order's record then showed 900 owed by the buyer and 1,100 released. In row 62 the seller received exactly 900 and the lender's wallet paid exactly 900. The trace asserts that rows 63 and 64 change nothing.

## One record missing: no early finalization

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 94 | The carrier signs 900 units delivered on order 2 | Accepted |  |
| 95 | Finalization of order 2 at 0 seconds: the carrier's report is in, the seller has not claimed | Refused | `BillingFinality` 6066, ClaimsCore |
| 96 | The seller signs a claim of 1,400 on order 3 | Accepted |  |
| 97 | Finalization of order 3 at 0 seconds: the seller's claim is in, the carrier has not signed | Refused | `BillingFinality` 6066, ClaimsCore |
| 100 | Finalization of order 2 at 45 seconds, the last second of the seller's claim window | Refused | `BillingFinality` 6066, ClaimsCore |

## After both windows close

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 98 | The carrier signs 900 units delivered on order 4 | Accepted |  |
| 99 | The seller signs a claim of 1,400 on order 4 at 45 seconds | Accepted |  |
| 101 | A key with no role finalizes order 2 at 46 seconds; the carrier signed 900 and the seller never claimed: bill 0, all 2,000 released | Accepted |  |
| 102 | A key with no role finalizes order 3 at 46 seconds; the seller claimed 1,400 and the carrier never signed: bill 0, all 2,000 released | Accepted |  |
| 103 | A key with no role finalizes order 4 at 46 seconds; the carrier signed 900 and the seller claimed 1,400: bill 900, 1,100 released | Accepted |  |
| 104 | A key with no role collects order 4's 900 from the lender's wallet to the seller | Paid |  |

Orders 1 and 4 have the same records and the same bill. Order 1 was finalized at 0 seconds and order 4 at 46.

## A program as delivery signer: the order

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 66 | Program order 1 names the delivery-source address of the rule program that has an upgrade authority, and shows that program | Refused | `BillingSourceProgram` 6078, ClaimsCore |
| 67 | Program order 1 names the upgradeable program's address but shows the rule program with no upgrade authority | Refused | `BillingSourceProgram` 6078, ClaimsCore |
| 68 | Program order 1 names the rule program's address for a source identity other than the policy's | Refused | `BillingSourceProgram` 6078, ClaimsCore |
| 69 | Program order 1 names the carrier's key as delivery signer through the program instruction | Refused | `BillingSourceProgram` 6078, ClaimsCore |
| 70 | Program order 1 shows the seller's wallet as the rule program, naming that wallet's delivery-source address | Refused | `BillingSourceProgram` 6078, ClaimsCore |
| 71 | Program order 1 shows the rule program with the upgradeable program's ProgramData | Refused | `BillingSourceProgram` 6078, ClaimsCore |
| 72 | A loader buffer account is created and initialized | Accepted |  |
| 73 | Program order 1 shows the rule program with the loader buffer in place of its ProgramData | Refused | `BillingSourceProgram` 6078, ClaimsCore |
| 74 | Program order 1 naming the rule program's address, sent through the plain `accept_evidence_financed_tab` | Refused | `BillingPolicy` 6060, ClaimsCore |
| 75 | Program order 1, maximum 2,000, names the delivery-source address of the rule program, which has no upgrade authority | Accepted |  |
| 76 | Accepted program order 1 is sent again | No effect |  |

Rows 66 to 71, 73 and 74 are program order 1 with one thing changed, and row 75 is program order 1 unchanged. After each refusal the trace asserts that the budget hold, the lender's promise, the facility, the credit line and the lender's wallet are unchanged, and that no order record, Registry reservation, Registry operation record, receipt or bill exists at the order's addresses. For accepted program order 1 it asserts that the bill names `9j3Dn1ix4qEc4g4f1rDRWNgfm8c4DgmVxHQNNorR4hxN` as the delivery signer and that the rule program does not sign the order.

## A program as delivery signer: the report

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 77 | Evidence order 5, maximum 2,000, naming the carrier | Accepted |  |
| 78 | The carrier signs the report of 900 on program order 1 with its Ed25519 key | Refused | `BillingSignature` 6063, ClaimsCore |
| 79 | A principal's owner submits the report of 900 on program order 1 through `submit_principal_bill_evidence` | Refused | `AccountNotInitialized` 3012, ClaimsCore, on the account `binding` |
| 80 | The report of 900 on program order 1 is sent to ClaimsCore directly, without the rule program's address signing | Refused | `AccountNotSigner` 3010, ClaimsCore, on the account `source` |
| 81 | The upgradeable copy of the rule program signs the report of 900 on program order 1 at its own delivery-source address | Refused | `BillingSignature` 6063, ClaimsCore |
| 82 | The rule program signs, but the instruction shows the upgradeable program as the rule program | Refused | `BillingSignature` 6063, ClaimsCore |
| 83 | The rule program signs a report of 900 on order 5, which names the carrier | Refused | `BillingSignature` 6063, ClaimsCore |
| 84 | The rule program signs 900 units delivered on program order 1 | Accepted |  |
| 85 | The rule program sends its exact report on program order 1 again | No effect |  |
| 86 | The rule program signs a second report of 1,000 on program order 1 | Refused | `BillingReplay` 6064, ClaimsCore |
| 87 | The seller signs a claim of 1,400 on program order 1 | Accepted |  |
| 88 | A key with no role finalizes program order 1 at 0 seconds: the rule program signed 900, the seller claimed 1,400; bill 900 | Accepted |  |
| 89 | A key with no role collects program order 1's 900 from the lender's wallet to the seller | Paid |  |

In row 84 the only transaction signer is the key that submits signed records, and the report digest the bill keeps is the one an Ed25519 signer would sign. In row 89 the seller received exactly 900 and the lender's wallet paid exactly 900.

## The owner pins the program's address

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 90 | The buyer's owner creates agent 2's permission pinned to `delivery_signer` = the rule program's address | Accepted |  |
| 91 | Agent 2's program order names the rule program's address for another source identity | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 92 | Agent 2's program order naming the pinned address shows no list of terms | Refused | `TermsRequired` 6051, PrincipalRegistry |
| 93 | Agent 2's program order, maximum 2,000, names the pinned address and shows its list | Accepted |  |

## A recoverable order

Trace: [early-settlement-recoverable-trace.json.xz](early-settlement-recoverable-trace.json.xz), suite `early-settlement-recoverable`. The delivery signer is the source's Registry principal.

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 61 | Recoverable order 1, maximum 2,000, naming the source principal as delivery signer | Accepted |  |
| 62 | The rule program signs the report on recoverable order 1, whose delivery signer is the source principal | Refused | `BillingSignature` 6063, ClaimsCore |
| 63 | The source principal's owner signs 900 units delivered on recoverable order 1 | Accepted |  |
| 64 | Finalization of recoverable order 1 at 0 seconds: the source's report is in, the seller has not claimed | Refused | `BillingFinality` 6066, ClaimsCore |
| 65 | The seller principal's owner signs a claim of 1,400 on recoverable order 1 | Accepted |  |
| 66 | A key with no role finalizes recoverable order 1 at 0 seconds: the source signed 900, the seller claimed 1,400; bill 900 | Accepted |  |

## Earlier records at their own commits

The [named-signer record](../named-signer/README.md), at commit `e55c0ad`, refused finalization of order 1 at 0 seconds (its row 79) and at 45 seconds (its row 82) while the named signer's report and the seller's claim were both in. At this record's commit those two cases point at order 2 of the named-signer test, which has the seller's claim and no report, and they are refused at the same moments with the same error, `BillingFinality`. Two cases of the Account repository's evidence-billing test changed the same way.

The Account repository's record compares the traces of the 28 test modules recorded before this change, 48 traces with 4,418 transactions, between main at `6b3009f` and this commit, transaction by transaction: name, signed bytes, accounts, instructions, error, program logs with compute units and moved source lines set aside, observed account state, assertions and receipts. These four cases are the only differences. Six of the 28 modules stop with the same error on both builds, and their partial traces are part of the comparison.

## Who holds which key

| Role in the main run | Key |
|---|---|
| Buyer's owner | `3BuW9SR5tG6VFK4MmkQQ3Ak8ny1K1Vv5Uz7is8Aa5pwG` |
| Agent 1 | `Cdkrk8tujFY6mTyGwFgKpnbiGc1hqtXCog1qvUdKAe6D` |
| Agent 2 | `5WcE8o73vmsSZXeeWTLm3ty3fAJKCnBWRF6VuKUme5nu` |
| Seller | `4MfyR4G3NWfVRDWo6iNAHDBZqWMgwZX6FNtMqEW3a9JT` |
| Carrier, the Ed25519 delivery signer | `GFKfRLPKHYRyARLSTc4p94vukKPMFAkSrQT5j55WYoy2` |
| Lender's owner | `6TcyBfPdBt1kjsvDZLzmBFnuMaLWiTaAt4RjUr9VA5YD` |
| Credit issuer's owner | `FMUEmtxhU46GzhKF4FW9MLJdQWiLgjiXP9TYRWSrqTpV` |
| Fee payer of setup and order transactions | `AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9` |
| Key that submits signed records | `92CudvFbL7Tyw2RkWC7NU1ehdNBSjy8FEG1TsJaN8wgz` |
| Key that finalizes | `4ap9kAPdHiV82d8D73mrtEgwGxBbUxJgyFr1fRGLhide` |
| Key that collects | `6jQ7iw7szEgN6hp49SAAMagYSYdDAZTQRDF3b8vTLNqM` |

The trace asserts that these eleven keys are all different.

| Role in the recoverable run | Key |
|---|---|
| Buyer's owner | `3BuW9SR5tG6VFK4MmkQQ3Ak8ny1K1Vv5Uz7is8Aa5pwG` |
| Agent | `Cdkrk8tujFY6mTyGwFgKpnbiGc1hqtXCog1qvUdKAe6D` |
| Seller's owner | `4MfyR4G3NWfVRDWo6iNAHDBZqWMgwZX6FNtMqEW3a9JT` |
| Source principal's owner | `7nLFtY5yiR4CZnkB6bYMth3bbAns9DnFhtgjHGqcCiRn` |

## The rule in the source

The three program files that differ from source already published are here as they stand at the commit the binaries were built from, under [source/](source), with the rule program's source. The source calls the delivery signer the `source` and the seller the `supplier`.

Finalization opens when both records are in:

```rust
// programs/claims-core/src/evidence_billing.rs
365:     // Both records are immutable once admitted, so the amount is already final.
366:     require!(a.bill.policy.finalization_ready(now)
367:         || (a.bill.source_report.is_some() && a.bill.seller_claim.is_some()), CoreError::BillingFinality);
```

Each record is admitted once, and an admitted record is never replaced:

```rust
// programs/claims-core/src/evidence_billing.rs
296:     if let Some(original) = &bill.source_report {
297:         require!(original == &report && bill.source_digest == digest, CoreError::BillingReplay);
298:         return Ok(()); // Never replace the original chain admission time.
334:     if let Some(original) = &a.bill.seller_claim {
335:         require!(original == &seller_claim && a.bill.seller_digest == digest, CoreError::BillingReplay);
336:         return Ok(());
```

A program-sourced order names the rule program's address, and the rule program has no upgrade authority:

```rust
// programs/claims-core/src/evidence_billing.rs
160: fn check_rule_program(policy: &BillingPolicy, program: &AccountInfo, program_data: &AccountInfo) -> Result<()> {
161:     let (source, _) = Pubkey::find_program_address(&[DELIVERY_SOURCE_SEED, &policy.source_identity], program.key);
162:     require_keys_eq!(source, policy.source, CoreError::BillingSourceProgram);
163:     let loader = anchor_lang::solana_program::bpf_loader_upgradeable::ID;
164:     require!(program.executable && *program.owner == loader && *program_data.owner == loader,
165:         CoreError::BillingSourceProgram);
166:     let (p, pd) = (program.try_borrow_data()?, program_data.try_borrow_data()?);
167:     // Program: Program { programdata_address }. ProgramData: ProgramData { slot, upgrade_authority: None }.
168:     require!(p.len() == 36 && p[..4] == 2u32.to_le_bytes() && p[4..36] == program_data.key.to_bytes()
169:         && pd.len() >= 45 && pd[..4] == 3u32.to_le_bytes() && pd[12] == 0, CoreError::BillingSourceProgram);
```

Only that program's signature admits its report:

```rust
// programs/claims-core/src/evidence_billing.rs
319: pub fn submit_program_evidence(ctx: Context<SubmitProgramBillRecord>, report: SourceReport) -> Result<()> {
321:     admit_report(&mut a.bill, report, &|bill, _| {
322:         let (source, _) = Pubkey::find_program_address(
323:             &[DELIVERY_SOURCE_SEED, &bill.policy.source_identity], a.rule_program.key);
324:         require!(source == bill.policy.source && a.source.key() == source, CoreError::BillingSignature);
```

The plain evidence instruction accepts only a key as the delivery signer:

```rust
// programs/claims-core/src/evidence_billing.rs
223:             require!((program_source || policy.source.is_on_curve()) && a.funding.supplier.key().is_on_curve(),
224:                 CoreError::BillingPolicy);
```

[lib.rs](source/foundation/onchain/programs/claims-core/src/lib.rs) adds the two instructions at lines 136 and 142 and the error `BillingSourceProgram` at line 1062, and [recoverable_billing.rs](source/foundation/onchain/programs/claims-core/src/recoverable_billing.rs) passes the new argument at line 101. The bill amount is unchanged from the [named-signer record](../named-signer/README.md#the-rule-in-the-source).

## Where it ran

The compiled programs ran in LiteSVM from solders 0.29.0 under Python 3.12.3, with signature verification on and native v1 transactions. Each run has one synthetic SPL token and every key is a disposable test key. The main run moves the test clock to 45 and 46 seconds after acceptance for orders 2, 3 and 4. The runs started on November 18, 2026 at 6:21 PM US Eastern. They sent no transaction to a public network.

This record ran commit `1333fb6fd861890d8b107ad0249a4766004d1394` of the Dexter Account repository, which reached its main branch in merge `5df2ff1`. The record itself is commit `0257908`. Its base, main at `6b3009f`, has the same program source as the [pinned-price record](../pinned-price/README.md#where-it-ran), and the same checkout first rebuilt that base and reproduced its three binary hashes. Three of the 69 build inputs differ from published files, and they are the three program files under [source/](source). Three more are identical to files under [../pinned-price/source/](../pinned-price/source/NOTICE.md), one to a file under [../pinned-delivery/source/](../pinned-delivery/source/NOTICE.md) and the other 62 to files under [../source/](../source/README.md). PrincipalRegistry's and PoolIssuer's binaries are the same as in the pinned-price record.

| Program | Address | SHA-256 of the binary |
|---|---|---|
| ClaimsCore | `727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz` | `a96472c1e26f35fcabe9a5af9122c2d807c1048ba8a7a511dc0b2ea5473b69f2` |
| PrincipalRegistry | `9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G` | `f6f4052ceb824561f98718176bb4d32d7851a603a949b750aecf2b9465587659` |
| PoolIssuer | `3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm` | `6804757b94ce2da8854388a1f748b614296207682db27cde85958f868b726e65` |
| Swig | `swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB` | `bb744d9811c392e3f343952d1a99bef9c97aa1936248a23b9d7efc1adb8939c1` |
| Rule program | `uY46ibBrA5pEsx4z8iiQ6BpNeBuMR6weo4u2ubREB9q` and `8ESb6S2iPPgUes97tCQJPn7PTNbkWtfQLGqKgFXyvwrr` | `4f16350c3aa170832750179d5fd304d2d3cd60e42c5ba02a21e79943c56142ec` |

The rule program was built from the three files under [source/foundation/test-programs/delivery-rule/](source/foundation/test-programs/delivery-rule/Cargo.toml) with:

```sh
./dev exec cargo-build-sbf --no-rustup-override --skip-tools-install --tools-version v1.57 \
  --manifest-path foundation/test-programs/delivery-rule/Cargo.toml --arch v3 \
  --sbf-out-dir .local/anchor/fixtures -- --locked
```

Each trace records the SHA-256 of all 69 source inputs and of the three Account binaries. The receipt records the rule program's binary hash and the SHA-256 of its three source files. The source lines in the case index (`error_source`) are lines of the files under [source/](source), except `principal-registry/src/lib.rs`, which is in [../pinned-price/source/](../pinned-price/source/NOTICE.md).

| Run | Transactions | Refusals | Assertions | SHA-256 of the JSON inside the trace |
|---|---:|---:|---:|---|
| Main | 105 | 21 | 517 | `4abdd7209b4e0e1a2d34288c5670dc57f20961dfa117c088b72ebd7b3cf175cd` |
| Recoverable | 67 | 2 | 116 | `42d579bf941fdcbfd4a26b22b9c7f5b4629025fc958935053ea96d8e32e11cab` |

The largest signed transactions in the two traces are 3,221 bytes, main rows 91 and 93.

## Files

| File | Contents |
|---|---|
| [early-settlement-trace.json.xz](early-settlement-trace.json.xz) | The main run's 105 transactions: signed bytes, result, program log, and the observed accounts before and after. About 20 MB uncompressed. |
| [early-settlement-recoverable-trace.json.xz](early-settlement-recoverable-trace.json.xz) | The recoverable run, in the same form. |
| [case-index.json](case-index.json) | One entry per transaction of the two runs: run, name, outcome, error name and number, the program that raised it and the source line its log names. |
| [early-settlement-receipt.json](early-settlement-receipt.json) | Counts and trace hashes for each run, the role keys, the binary hashes and the rule program's addresses, binary hash and source hashes. |
| [early-settlement.log](early-settlement.log) | The three lines the runs printed. |
| [early_settlement.py](early_settlement.py) | The test module. It builds on the pinned-delivery record's `pinned_delivery.py`, which at this commit differs from the [published copy](../pinned-delivery/pinned_delivery.py) in one line, the source line it expects for a refused report signature. It also uses `pinned_terms.py`, the setup of the commit record's [commit_legs.py](../commit_legs.py), which is byte-identical at this commit, and the Account repository's `evidence_billing.py` and `recovery.py` fixtures. |
| [source/](source) | The three program source files, the rule program's three files and their [notice](source/NOTICE.md). |
| [provenance.json](provenance.json) | The source commits and the source and published hash of each file. |
| [SHA256SUMS](SHA256SUMS) | Hashes of the fourteen files above. |

The traces, case index, receipt, test module and six source files are byte-identical to their source commits: commit `0257908f32ec5dfb693b7ede8645a15f44a3892e` for the record and the test module, and `1333fb6` for the source. In the log, the absolute workspace path is replaced with `<fixture-workspace>` in two places. The source files keep their existing copyright, and this release adds no license grant for them. Each trace lists in `fixture_inputs` the hash of every test file the run used.

## Check a row yourself

Download a trace, then print any row's name, result, state comparison and program log. This reads row 66 of the main run, the order that names the rule program with an upgrade authority:

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
" early-settlement-trace.json.xz 66
```

```text
refused: program order 1 names the delivery-source address of the rule program with an upgrade authority, and shows that program
expected: reject | error: {'InstructionError': [0, {'Custom': 6078}]}
state unchanged: True
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz invoke [1]
Program log: Instruction: AcceptProgramSourcedFinancedTab
Program log: AnchorError thrown in programs/claims-core/src/evidence_billing.rs:168. Error Code: BillingSourceProgram. Error Number: 6078. Error Message: The delivery signer is not the delivery-source address of a rule program with no upgrade authority.
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz failed: custom program error: 0x17be
```

The lines left out here are system program calls and compute-unit counts. Line 168 is the upgrade-authority check quoted above. With row 61 in place of 66, the same command prints the finalization of order 1 at 0 seconds, `FinalizeEvidenceBill`, with its calls to PrincipalRegistry and PoolIssuer. `transaction_base64` in each row is the signed transaction, and `observed_before` and `observed_after` hold each observed account's owner, lamports and data.

To check the files against their hashes, and a decompressed trace against `trace_sha256` in the receipt:

```sh
sha256sum -c SHA256SUMS
xz -dc early-settlement-trace.json.xz | sha256sum
```

The second command prints `4abdd7209b4e0e1a2d34288c5670dc57f20961dfa117c088b72ebd7b3cf175cd`.

The [named-signer record](../named-signer/README.md) covers orders that name a key as the delivery signer, and the [pinned-delivery record](../pinned-delivery/README.md) an owner pinning that signer. This record adds early settlement and a program as the signer. The [claim guide](../../../articles/the-account-inversion/README.md) maps the article to its records.
