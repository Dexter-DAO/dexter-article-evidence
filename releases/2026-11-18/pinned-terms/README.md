# An owner pins terms of the deal, and an order that breaks them never commits

An owner can pin terms of the deal, and an order that breaks them never commits. On November 18, 2026, on compiled Account programs, a buyer's owner pinned size = 10 on its agent's permission. The agent and the seller then submitted an order whose terms said size = 11. It was refused, and nothing was held or recorded: no budget hold, no lender promise and no order record. The same order with size = 10 was accepted. A second agent, whose permission the owner did not pin, ordered size 11 and was accepted.

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 57 | The buyer's owner pins size = 10 on agent 1's permission | Accepted |  |
| 77 | Agent 1 and the seller submit order 1 with terms that say size = 11 | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 81 | Order 1 with a list that names size twice, as size = 10 and as size = 11 | Refused | `TermsMismatch` 6052, PrincipalRegistry |
| 85 | Agent 1 and the seller submit order 1 with terms that say size = 10 | Accepted |  |
| 87 | Agent 2, whose permission is not pinned, submits order 2 with size = 11 | Accepted |  |

The programs ran on a local Solana virtual machine with a synthetic token. The main run has 100 signed transactions and 27 refusals, and all 280 of its recorded assertions passed. Three shorter runs repeat the pin with passkey owners, on a recoverable order and on a pool-funded order, and all of their assertions passed. After each of the 30 refusals in the tables below, every account the transaction could write, other than the fee payer, held the same bytes as before. [case-index.json](case-index.json) names every transaction of the four runs, and the four traces hold each one's signed bytes, program log and account state.

## How an order shows its terms

A term is a pair of 32-byte fingerprints, one of a field's name and one of its value. The buyer's owner, the agent and the seller compute the fingerprints the same way, and the programs compare the 32-byte values.

```text
name fingerprint  = sha256("DEXTER:ORDER-TERM-NAME:1" || name)
value fingerprint = sha256("DEXTER:ORDER-TERM-VALUE:1" || value)
order terms hash  = sha256("DEXTER:ORDER-TERMS:2" || u8(count) || name_1 || value_1 || ... || name_n || value_n)
```

- The owner pins with PrincipalRegistry `pin_mandate_terms`. It takes the owner's authority and replaces the permission's pinned list with zero to four terms whose names ascend, so a permission requires one value per name. An empty list unpins. Each change raises the permission's revision by one.
- An order shows its terms through ClaimsCore `accept_pinned_financed_tab`, which is `accept_financed_tab` plus the order's list of 1 to 16 terms. It calls the Registry's `reserve_pinned` in place of `reserve`.
- The Registry checks three things about the list. Its names ascend, so every name appears once. It hashes to the terms the agent and the seller signed. Either failure is `TermsMismatch`. And every pinned name is in it with the pinned value, or the order fails with `TermsNotPermitted`.
- An instruction that shows no list refuses a pinned permission with `TermsRequired`.

The order terms hash is the order's `external_contract_hash`, which ClaimsCore binds into the terms the agent and the seller sign. The fingerprints used in the runs:

| Text | Fingerprint |
|---|---|
| Name `size` | `05b37e4652b5bc6d4e0b3c9b3853eb3fa49ef68411548147e0c9ff5b215c84fb` |
| Name `colour` | `afbc890ab78fe0170cddb1cf432db91c94783f184e2562d512b77abe68ba8a28` |
| Value `10` | `67c64cd99ee9c2941257cd3c1d83fabde2526b1aff20a1cf54758f495d572416` |
| Value `11` | `f05c20c04a654ee163ccc9887013bbf3d6ec1cb82722ebcc1c2969cb77620c60` |
| Value `red` | `20b69583a4955ce56a5f643520e2290f5905722d38959b5f8c244fd5b424b6d3` |
| Value `blue` | `cec0061149fed3ce0b661b96cca0a51fd7c57e515ec227f750ea1a85dc0882c9` |

## The scenario

One buyer, two agents, one seller, one credit issuer and one lender. Every order has a maximum of 100,000 at unit price 1, with no interest and no seller fee. Each agent may order 100,000 per order, the buyer's budget is 500,000, the lender's facility and the credit line are 500,000 each, and the lender's wallet starts with 1,000,000. Amounts are base units of the synthetic token.

The seller's terms for orders 1, 2, 4 and 5 have four fields: item = trail shoe, size, colour = red and ship_to = warehouse 4. Order 3 has sixteen fields.

| Order | Agent | Seller's terms | Permission at the time | Sent through | Outcome |
|---:|---|---|---|---|---|
| 1 | 1 | size = 10 | Pins size = 10 | `accept_pinned_financed_tab` | Accepted |
| 2 | 2 | size = 11 | Nothing pinned | `accept_financed_tab` | Accepted |
| 5 | 2 | size = 11 | Nothing pinned | `accept_pinned_financed_tab` | Accepted, then released by the seller |
| 3 | 1 | size = 10, colour = red, sixteen fields | Pins size = 10 and colour = red | `accept_pinned_financed_tab` | Accepted |
| 6 | 1 | An evidence order | Pins two terms, then unpinned | `accept_evidence_financed_tab` | Refused while pinned, accepted after the unpin |
| 4 | 1 | size = 11 | Pins two terms, then unpinned | `accept_financed_tab` | Refused while pinned, accepted after the unpin |

The main run ends with five orders of 100,000 held against the budget and promised by the lender, nothing consumed, and no tokens moved out of the lender's wallet.

In the tables below, "Row" is the zero-based position in a trace's `transactions` array and equals `index` in the case index for that run's `suite`. "Error" is the name and number in the program log, followed by the program that raised it. Every order refusal reaches the Registry through ClaimsCore, so the Registry fails first and ClaimsCore fails with the same code.

## The pinned order

Trace: [pinned-terms-trace.json.xz](pinned-terms-trace.json.xz), suite `pinned-terms`. Agent 1's permission pins size = 10 from row 57.

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 57 | The buyer's owner pins size = 10 on agent 1's permission | Accepted |  |
| 77 | Agent 1 and the seller submit order 1 with terms that say size = 11 | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 78 | Order 1 with terms that have no size field | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 79 | Order 1's signed terms say size = 11, and the agent shows a list that says size = 10 | Refused | `TermsMismatch` 6052, PrincipalRegistry |
| 80 | Order 1 with size = 10, sent with an empty list | Refused | `TermsMismatch` 6052, PrincipalRegistry |
| 81 | Order 1 with a list that names size twice, as size = 10 and as size = 11 | Refused | `TermsMismatch` 6052, PrincipalRegistry |
| 82 | Order 1 with size = 10 and its four names in descending order | Refused | `TermsMismatch` 6052, PrincipalRegistry |
| 83 | Order 1 with size = 10, sent through `accept_financed_tab`, which shows no list | Refused | `TermsRequired` 6051, PrincipalRegistry |
| 84 | Order 1 with size = 10, prepared against the permission's revision from before the pin | Refused | `MandateRevision` 6008, PrincipalRegistry |
| 85 | Agent 1 and the seller submit order 1, maximum 100,000, with terms that say size = 10 | Accepted |  |
| 86 | Accepted order 1 is sent again | No effect |  |

Rows 77 to 84 are order 1 with one thing changed, and row 85 is order 1 unchanged. In rows 81 and 82 the list shown is the list the signed terms hash to, so its names are the only thing wrong with it. After every refused order the trace asserts that the budget hold, the lender's promise, the facility, the credit line and the lender's wallet are unchanged, and that no order record, Registry reservation, Registry operation record or receipt exists at the order's addresses.

## The agent the owner did not pin

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 87 | Agent 2 submits order 2 with size = 11 through `accept_financed_tab` | Accepted |  |
| 88 | Agent 2 sends order 5, size = 11, with a list that says size = 10 | Refused | `TermsMismatch` 6052, PrincipalRegistry |
| 89 | Agent 2 submits order 5 with size = 11 through `accept_pinned_financed_tab` | Accepted |  |
| 90 | The seller releases order 5's unused 100,000 | Accepted |  |

## Two pinned terms

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 91 | The buyer's owner pins size = 10 and colour = red on agent 1's permission | Accepted |  |
| 92 | Order 3 with size = 10 and colour = blue | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 93 | Order 3 with a seventeenth term added to its sixteen | Refused | `TermsMismatch` 6052, PrincipalRegistry |
| 94 | Order 3, maximum 100,000, with size = 10 and colour = red among its sixteen terms | Accepted |  |

## Who can pin

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 61 | Agent 1 unpins its own permission, naming itself as the owner | Refused | `OwnerAuthority` 6001, PrincipalRegistry |
| 62 | Agent 1 changes the pin to size = 11, naming the owner without its signature | Refused | `OwnerAuthority` 6001, PrincipalRegistry |
| 63 | Agent 1 unpins its own permission, naming the owner without its signature | Refused | `OwnerAuthority` 6001, PrincipalRegistry |
| 64 | A key that is not the owner unpins agent 1's permission, naming itself as the owner | Refused | `OwnerAuthority` 6001, PrincipalRegistry |
| 65 | A key that is not the owner changes the pin to size = 11, naming the owner without its signature | Refused | `OwnerAuthority` 6001, PrincipalRegistry |
| 66 | A key that is not the owner unpins agent 1's permission, naming the owner without its signature | Refused | `OwnerAuthority` 6001, PrincipalRegistry |

The buyer's owner signs every accepted pin and unpin. It signs none of the orders.

## What a pin holds

Rows 67 to 76 use a spare permission.

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 58 | The same pin, size = 10 at expected revision 1, is sent again | No effect |  |
| 59 | The original setup of agent 1's permission is sent again after the pin | Refused | `ConfigurationConflict` 6002, PrincipalRegistry |
| 60 | The original setup of agent 2's unpinned permission is sent again | No effect |  |
| 67 | The buyer's owner pins four terms | Accepted |  |
| 68 | The buyer's owner pins five terms | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 69 | The buyer's owner pins the field size twice, as 10 and as 11 | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 70 | The buyer's owner pins the same term twice | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 71 | The buyer's owner pins two terms with their names out of ascending order | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 72 | The buyer's owner pins a term whose name is all zeros | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 73 | The buyer's owner pins size = 10 against revision 1 when the permission is at revision 2 | Refused | `MandateRevision` 6008, PrincipalRegistry |
| 74 | The buyer's owner revokes the spare permission | Accepted |  |
| 75 | The buyer's owner pins size = 10 on the revoked permission | Refused | `MandateRevoked` 6007, PrincipalRegistry |
| 76 | The revocation is sent again | No effect |  |

## Other kinds of order on a pinned permission

In the main run agent 1's permission pins size = 10 and colour = red until the owner unpins it in row 97.

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 95 | Order 6, an evidence order of 100,000 sent through `accept_evidence_financed_tab` | Refused | `TermsRequired` 6051, PrincipalRegistry |
| 96 | Order 4 with size = 11 through `accept_financed_tab` | Refused | `TermsRequired` 6051, PrincipalRegistry |
| 97 | The buyer's owner unpins agent 1's permission with an empty list | Accepted |  |
| 98 | Order 4 with size = 11 through `accept_financed_tab` | Accepted |  |
| 99 | Order 6, the evidence order of 100,000 | Accepted |  |

Trace: [pinned-terms-recoverable-trace.json.xz](pinned-terms-recoverable-trace.json.xz), suite `pinned-terms-recoverable`.

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 61 | The buyer's owner pins size = 10 on the agent's permission | Accepted |  |
| 62 | Order 1, a recoverable order of 100,000 sent through `accept_recoverable_financed_tab` | Refused | `TermsRequired` 6051, PrincipalRegistry |
| 63 | The buyer's owner unpins the agent's permission with an empty list | Accepted |  |
| 64 | Order 1, the recoverable order of 100,000 | Accepted |  |

Trace: [pinned-terms-pool-funded-trace.json.xz](pinned-terms-pool-funded-trace.json.xz), suite `pinned-terms-pool-funded`.

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 12 | The buyer's owner pins size = 10 on agent 1's permission | Accepted |  |
| 13 | Order 1, a pool-funded order of 100,000 sent through `accept_tab` | Refused | `TermsRequired` 6051, PrincipalRegistry |
| 14 | The buyer's owner unpins agent 1's permission with an empty list | Accepted |  |
| 15 | Order 1, the pool-funded order of 100,000 | Accepted |  |

## Passkey owners

Trace: [pinned-terms-passkey-trace.json.xz](pinned-terms-passkey-trace.json.xz), suite `pinned-terms-passkey`. The buyer's owner and the credit issuer's owner are passkey aliases, which cannot sign a transaction. Each approves one exact instruction with a P-256 proof. The order has sixteen terms.

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 75 | The buyer's owner, a passkey, pins size = 10 on the agent's permission | Accepted |  |
| 76 | Order 1 with size = 11 among its sixteen terms, signed by the agent, the seller and the credit issuer's passkey | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 77 | Order 1, maximum 100,000, with size = 10 among its sixteen terms, signed by the same three | Accepted |  |

## Who holds which key

| Role in the main run | Key |
|---|---|
| Buyer's owner | `3BuW9SR5tG6VFK4MmkQQ3Ak8ny1K1Vv5Uz7is8Aa5pwG` |
| Agent 1 | `Cdkrk8tujFY6mTyGwFgKpnbiGc1hqtXCog1qvUdKAe6D` |
| Agent 2 | `5WcE8o73vmsSZXeeWTLm3ty3fAJKCnBWRF6VuKUme5nu` |
| Seller | `4MfyR4G3NWfVRDWo6iNAHDBZqWMgwZX6FNtMqEW3a9JT` |
| Lender's owner | `6TcyBfPdBt1kjsvDZLzmBFnuMaLWiTaAt4RjUr9VA5YD` |
| Credit issuer's owner | `FMUEmtxhU46GzhKF4FW9MLJdQWiLgjiXP9TYRWSrqTpV` |
| Fee payer of setup and order transactions | `AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9` |
| Key that is not the owner, rows 64 to 66 | `8bcRzB3v6PxbbtkVCiX9ceW2whwakA6gX7qvSYbeMHLq` |

The trace asserts that these eight keys are all different.

| Role in the passkey run | Key |
|---|---|
| Buyer's owner, a passkey alias | `FZXmU3vnccKyM384A4S2dEA4GqVByq7i8tDKyyaX1kBf` |
| Credit issuer's owner, a passkey alias | `DXugwUsviZJK1YWbe2m2TdxoPNTULtiGC5HiqDJ7J9sR` |
| Agent | `Cdkrk8tujFY6mTyGwFgKpnbiGc1hqtXCog1qvUdKAe6D` |
| Seller | `4MfyR4G3NWfVRDWo6iNAHDBZqWMgwZX6FNtMqEW3a9JT` |

The receipt lists the keys of the other two runs.

## The rule in the source

The six program files this change touches are published here as they stand at the commit the binaries were built from, under [source/](source). The source calls a permission a `Mandate`.

A term, and the two counts:

```rust
// programs/principal-registry/src/state.rs
117: pub const MAX_REQUIRED_TERMS: usize = 4;
118: pub const MAX_ORDER_TERMS: usize = 16;
123: pub struct Term {
124:     pub name: [u8; 32],
125:     pub value: [u8; 32],
```

Names ascend, so each appears once, and the order terms hash is over the whole list:

```rust
// programs/principal-registry/src/state.rs
130: pub fn names_ascend(terms: &[Term]) -> bool {
131:     terms.windows(2).all(|pair| pair[0].name < pair[1].name)
165: pub fn order_terms_hash(terms: &[Term]) -> [u8; 32] {
168:     parts.extend([b"DEXTER:ORDER-TERMS:2".as_slice(), &count]);
170:         parts.extend([term.name.as_slice(), &term.value]);
```

The owner pins:

```rust
// programs/principal-registry/src/lib.rs
239:     pub fn pin_mandate_terms(ctx: Context<ManageMandate>, expected_revision: u64,
240:         required_terms: Vec<Term>) -> Result<()> {
242:         require_owner_authority(&ctx.accounts.principal,&ctx.accounts.owner,ctx.remaining_accounts)?;
245:         require!(required_terms.len() <= MAX_REQUIRED_TERMS && names_ascend(&required_terms)
246:             && required_terms.iter().all(|term| term.name != [0; 32]), RegistryError::InvalidPolicy);
```

The Registry checks the order:

```rust
// programs/principal-registry/src/lib.rs
334:     match &shown {
335:         None => require!(mandate.required_terms_count == 0, RegistryError::TermsRequired),
336:         Some(order) => {
337:             require!((1..=MAX_ORDER_TERMS).contains(&order.terms.len()), RegistryError::TermsMismatch);
338:             require!(names_ascend(&order.terms), RegistryError::TermsMismatch);
339:             require!(completed_run_policy(&order.namespace, &order.unit, order.unit_price,
340:                 &order_terms_hash(&order.terms)) == terms.external_terms_hash, RegistryError::TermsMismatch);
341:             require!(mandate.pinned_terms().iter().all(|required| order.terms.contains(required)),
342:                 RegistryError::TermsNotPermitted);
```

ClaimsCore passes the order's list to the Registry inside the same instruction:

```rust
// programs/claims-core/src/lib.rs
108:     pub fn accept_pinned_financed_tab<'info>(ctx:Context<'info,AcceptFinancedTab<'info>>,request_id:[u8;32],terms:TabTerms,
111:         funding::accept(ctx,request_id,terms,maximum_interest,seller_fee_terms,assignment_policy_hash,namespace,service_unit,unit_price,external_contract_hash,Some(order_terms))
// programs/claims-core/src/funding.rs
571:     match order_terms {
572:         Some(order_terms)=>principal_registry::cpi::reserve_pinned(reserve,accepted,namespace,service_unit,unit_price,order_terms)?,
573:         None=>principal_registry::cpi::reserve(reserve,accepted)?,
```

## Where it ran

The compiled programs ran in LiteSVM from solders 0.29.0 under Python 3.12.3, with signature verification on. The main, passkey and recoverable runs use native v1 transactions, and the pool-funded run uses v0 transactions through a lookup table the test writes. The passkey run's setup writes a test upgrade authority into the Registry's loader metadata. Each run has one synthetic six-decimal SPL token, every key is a disposable test key, and no run moves the clock. The runs started on November 18, 2026 at 12:15 PM US Eastern. They sent no transaction to a public network.

Every other November 18 record ran commit `e55c0ad3a224a1c4e72de4c283161345d438caaf` of the Dexter Account repository. This record ran commit `ef9ae9768525512195afe4420a3640737d4880d5`, which is that commit plus this change, and the complete source at `e55c0ad` is in [../source/](../source/README.md). Six of the 69 build inputs differ between the two commits, and they are the six files under [source/](source). PoolIssuer's binary is the same in both.

| Program | Address | SHA-256 of the binary |
|---|---|---|
| ClaimsCore | `727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz` | `97eaa10bd33ca35c167f9f29947e27093930bece32c7eb758d7199b819cca8e2` |
| PrincipalRegistry | `9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G` | `be59a2285adbaec5b7d9a17988093eb6c398539966a4fb4b44a09959f3ea2265` |
| PoolIssuer | `3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm` | `6804757b94ce2da8854388a1f748b614296207682db27cde85958f868b726e65` |
| Swig | `swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB` | `bb744d9811c392e3f343952d1a99bef9c97aa1936248a23b9d7efc1adb8939c1` |

The main, passkey and recoverable traces record the SHA-256 of all 69 source inputs and of each binary, and the pool-funded trace records the three binary hashes. The source lines in the case index (`error_source`) are lines of the six files.

| Run | Transactions | Refusals | Assertions | SHA-256 of the JSON inside the trace |
|---|---:|---:|---:|---|
| Main | 100 | 27 | 280 | `1802e370ac6997db45433549ba377da3c359b11f3369065c9383004502a72b6f` |
| Passkey | 78 | 6 | 105 | `82cfbafbf45b462869e1a2582b4991a022a8a130ffffe31150ce745415542648` |
| Recoverable | 65 | 1 | 65 | `51d0fd8b9dd3a79be40f1fde8937faa22324af8d3b242a87057c1d4ca76fbe7e` |
| Pool-funded | 16 | 1 | 10 | `2704e44f49bb163989c2eaab9c679022145ace03cf226313cd636bdf6f9935f5` |

Five of the passkey run's six refusals are in the setup it shares with the Account repository's passkey test. The sixth is row 76.

## Files

| File | Contents |
|---|---|
| [pinned-terms-trace.json.xz](pinned-terms-trace.json.xz) | The main run's 100 transactions: signed bytes, result, program log, and the observed accounts before and after. About 11 MB uncompressed. |
| [pinned-terms-passkey-trace.json.xz](pinned-terms-passkey-trace.json.xz), [pinned-terms-recoverable-trace.json.xz](pinned-terms-recoverable-trace.json.xz), [pinned-terms-pool-funded-trace.json.xz](pinned-terms-pool-funded-trace.json.xz) | The three shorter runs, in the same form. |
| [case-index.json](case-index.json) | One entry per transaction of the four runs: run, name, outcome, error name and number, the program that raised it and the source line its log names. |
| [pinned-terms-receipt.json](pinned-terms-receipt.json) | Counts and trace hashes for each run, the role keys, the fingerprints and the binary hashes. |
| [pinned-terms.log](pinned-terms.log) | The five lines the runs printed. |
| [pinned_terms.py](pinned_terms.py) | The test module. It reuses the setup and the refusal checks in the commit record's [commit_legs.py](../commit_legs.py) and builds on four of the Account repository's test fixtures. |
| [source/](source) | The six program source files and their [notice](source/NOTICE.md). |
| [provenance.json](provenance.json) | The source commits and the source and published hash of each file. |
| [SHA256SUMS](SHA256SUMS) | Hashes of the sixteen files above. |

The traces, case index, receipt, test module and six source files are byte-identical to their source commits. In the log, the absolute workspace path is replaced with `<fixture-workspace>` in four places. The source files keep their existing copyright, and this release adds no license grant for them. Each of the main, passkey and recoverable traces lists in `fixture_inputs` the hash of every test file the run used.

## Check a row yourself

Download a trace, then print any row's name, result, state comparison and program log. This reads row 77 of the main run, the order for size 11:

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
" pinned-terms-trace.json.xz 77
```

```text
refused: agent 1 and the seller submit order 1 with terms that say size = 11; the owner pinned size = 10
expected: reject | error: {'InstructionError': [0, {'Custom': 6053}]}
state unchanged: True
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz invoke [1]
Program log: Instruction: AcceptPinnedFinancedTab
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G invoke [2]
Program log: Instruction: ReservePinned
Program log: AnchorError thrown in programs/principal-registry/src/lib.rs:341. Error Code: TermsNotPermitted. Error Number: 6053. Error Message: Order terms do not contain a term the owner pinned.
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G failed: custom program error: 0x17a5
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz failed: custom program error: 0x17a5
```

The lines left out here are system program calls and compute-unit counts. `transaction_base64` in the same row is the signed transaction, and `observed_before` and `observed_after` hold each observed account's owner, lamports and data.

To check the files against their hashes, and a decompressed trace against `trace_sha256` in the receipt:

```sh
sha256sum -c SHA256SUMS
xz -dc pinned-terms-trace.json.xz | sha256sum
```

The second command prints `1802e370ac6997db45433549ba377da3c359b11f3369065c9383004502a72b6f`.

The [commit record](../README.md) covers the three checks an order must pass, and this record is the permission check on the order's own terms. The [claim guide](../../../articles/the-account-inversion/README.md) maps the article to its records.
