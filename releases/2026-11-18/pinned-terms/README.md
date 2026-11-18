# An owner pins terms of the deal, and an order that breaks them never commits

An owner can pin terms of the deal, a permission can be created already pinned, and an order that breaks the pin never commits. On November 18, 2026, on compiled Account programs, a buyer's owner created its agent's permission already pinned to size = 10, in one instruction. Straight afterwards the agent and the seller submitted an order whose terms said size = 11. It was refused, and nothing was held or recorded: no budget hold, no lender promise and no order record. The same order with size = 10 was accepted. In the main run a second agent, whose permission the owner did not pin, ordered size 11 and was accepted.

| Run | Row | What happened | Outcome | Error |
|---|---:|---|---|---|
| Created pinned | 54 | The buyer's owner creates agent 1's permission already pinned to size = 10, in one instruction | Accepted |  |
| Created pinned | 55 | Straight afterwards, order 1 with size = 11 through the ordinary instruction | Refused | `TermsRequired` 6051, PrincipalRegistry |
| Created pinned | 56 | Order 1 with size = 11 through the instruction that shows its terms | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| Created pinned | 57 | Order 1 with size = 10 | Accepted |  |
| Main | 81 | Order 1 with a list that names size twice, as size = 10 and as size = 11 | Refused | `TermsMismatch` 6052, PrincipalRegistry |
| Main | 87 | Agent 2, whose permission is not pinned, submits order 2 with size = 11 | Accepted |  |

The programs ran on a local Solana virtual machine with a synthetic token. The record has five runs with 339 signed transactions, and all 654 recorded assertions passed. After each of the 42 refusals in the tables below, every account the transaction could write, other than the fee payer, held the same bytes as before. [case-index.json](case-index.json) names every transaction of the five runs, and the five traces hold each one's signed bytes, program log and account state.

## How an order shows its terms

A term is a pair of 32-byte fingerprints, one of a field's name and one of its value. The buyer's owner, the agent and the seller compute the fingerprints the same way, and the programs compare the 32-byte values.

```text
name fingerprint  = sha256("DEXTER:ORDER-TERM-NAME:1" || name)
value fingerprint = sha256("DEXTER:ORDER-TERM-VALUE:1" || value)
order terms hash  = sha256("DEXTER:ORDER-TERMS:2" || u8(count) || name_1 || value_1 || ... || name_n || value_n)
```

- The owner creates a permission already pinned with PrincipalRegistry `create_pinned_mandate`. It is `create_mandate` plus one to four terms, stored in the instruction that creates the permission.
- The owner changes a pin with `pin_mandate_terms`. It takes the owner's authority and replaces the permission's pinned list with zero to four terms whose names ascend, so a permission requires one value per name. An empty list unpins. Each change raises the permission's revision by one.
- An order shows its terms through ClaimsCore `accept_pinned_financed_tab`, which is `accept_financed_tab` plus the order's list of 1 to 16 terms. It calls the Registry's `reserve_pinned` in place of `reserve`.
- The Registry checks three things about the list. Its names ascend, so every name appears once. It hashes to the terms the agent and the seller signed. Either failure is `TermsMismatch`. And every pinned name is in it with the pinned value, or the order fails with `TermsNotPermitted`.
- An instruction that shows no list refuses a pinned permission with `TermsRequired`.

The order terms hash is the order's `external_contract_hash`, which ClaimsCore binds into the terms the agent and the seller sign.

[order_terms_vectors.json](order_terms_vectors.json) holds test vectors for these rules. Its four-term vector is the terms of order 1, listed here in ascending order of name fingerprint:

| Field | Value | Name fingerprint | Value fingerprint |
|---|---|---|---|
| size | 10 | `05b37e4652b5bc6d4e0b3c9b3853eb3fa49ef68411548147e0c9ff5b215c84fb` | `67c64cd99ee9c2941257cd3c1d83fabde2526b1aff20a1cf54758f495d572416` |
| item | trail shoe | `0a4afc41d4416fc657471728707cdc8bf7c2c5a4496ebec58ecf81b8cc766ab1` | `593f316cdacdd20aee24f82f2eed39a68db17719b2ef35f57a78248fdb72de8a` |
| ship_to | warehouse 4 | `6b57b1ca5def698d58053edcf071a8138c4055cdba3f477e053ce2cb09adb4be` | `e69e1a3b5f55190f989e039e738632a6428dbe7603bed236dda22a26ada443f6` |
| colour | red | `afbc890ab78fe0170cddb1cf432db91c94783f184e2562d512b77abe68ba8a28` | `20b69583a4955ce56a5f643520e2290f5905722d38959b5f8c244fd5b424b6d3` |

Its order terms hash is `12ba268ad7af896884f76de88116dfa74a0db5b40b8200db3a54f0c029f1120c`, and order 1's record stores that hash. This reproduces it from the text of the four fields:

```sh
python3 -c "
import hashlib, json
v = json.load(open('order_terms_vectors.json'))
h = lambda domain, text: hashlib.sha256(v[domain].encode() + text.encode()).digest()
terms = sorted((h('name_domain', n), h('value_domain', x)) for n, x in v['vectors'][1]['entries'])
print(hashlib.sha256(v['terms_domain'].encode() + bytes([len(terms)]) + b''.join(a + b for a, b in terms)).hexdigest())
"
```

The other fingerprints the runs use:

| Text | Fingerprint |
|---|---|
| Value `11` | `f05c20c04a654ee163ccc9887013bbf3d6ec1cb82722ebcc1c2969cb77620c60` |
| Value `blue` | `cec0061149fed3ce0b661b96cca0a51fd7c57e515ec227f750ea1a85dc0882c9` |

## The scenario

One buyer, two agents, one seller, one credit issuer and one lender. Every order has a maximum of 100,000 at unit price 1, with no interest and no seller fee. Each agent may order 100,000 per order, the buyer's budget is 500,000, the lender's facility and the credit line are 500,000 each, and the lender's wallet starts with 1,000,000. Amounts are base units of the synthetic token. The main run and the created-pinned run both use this cast.

The seller's terms for orders 1, 2, 4 and 5 of the main run have four fields: item = trail shoe, size, colour = red and ship_to = warehouse 4. Order 3 has sixteen fields.

In the tables below, "Row" is the zero-based position in a trace's `transactions` array and equals `index` in the case index for that run's `suite`. "Error" is the name and number in the program log, followed by the program that raised it. Every order refusal except the two direct calls in rows 67 and 68 of the created-pinned run reaches the Registry through ClaimsCore, so the Registry fails first and ClaimsCore fails with the same code.

## A permission created already pinned

Trace: [pinned-terms-created-pinned-trace.json.xz](pinned-terms-created-pinned-trace.json.xz), suite `pinned-terms-created-pinned`.

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 54 | The buyer's owner creates agent 1's permission already pinned to size = 10, in one instruction | Accepted |  |
| 55 | Straight afterwards, agent 1 and the seller submit order 1 with size = 11 through `accept_financed_tab` | Refused | `TermsRequired` 6051, PrincipalRegistry |
| 56 | Order 1 with size = 11 through `accept_pinned_financed_tab` | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 57 | Order 1, maximum 100,000, with size = 10 through `accept_pinned_financed_tab` | Accepted |  |

After row 54 the permission exists at revision 1 with the term size = 10. After each of the two refusals the trace asserts that the budget hold, the lender's promise, the facility, the credit line and the lender's wallet are unchanged, and that no order record, Registry reservation, Registry operation record or receipt exists at the order's addresses. After row 57 the hold on the budget and the lender's promise are each 100,000, and no tokens have left the lender's wallet. The buyer's owner does not sign the order.

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 58 | The same creation, size = 10, is sent again | No effect |  |
| 59 | `create_mandate` is sent for the permission that was created pinned | Refused | `ConfigurationConflict` 6002, PrincipalRegistry |
| 60 | The creation is sent again with size = 11 in place of size = 10 | Refused | `ConfigurationConflict` 6002, PrincipalRegistry |
| 61 | The buyer's owner creates a pinned permission with no terms | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 62 | The buyer's owner creates a pinned permission with the field size twice, as 10 and as 11 | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 63 | The buyer's owner creates a pinned permission with five terms | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 64 | The buyer's owner creates agent 2's permission pinned to size with an all-zero value fingerprint | Accepted |  |
| 65 | Agent 2 submits order 2 with size = 10 | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 66 | Order 2, maximum 100,000, whose size term carries the all-zero value | Accepted |  |
| 67 | A direct call to the Registry's `reserve_pinned` for order 3, naming ClaimsCore's authority address, which no key can sign for | Refused | `AccountNotSigner` 3010, PrincipalRegistry |
| 68 | A direct call to the Registry's `reserve_pinned` for order 3, naming the caller's own key in place of ClaimsCore's authority | Refused | `ConstraintSeeds` 2006, PrincipalRegistry |

The permission is unchanged after rows 58 to 60, and no permission exists after rows 61 to 63.

## An accepted order after the pin changes

The pin governs new orders. An accepted order keeps the terms it was accepted on, and the seller is still paid. These rows follow order 1 of the created-pinned run, accepted in row 57 with size = 10.

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 69 | The buyer's owner changes the pin on agent 1's permission to size = 12 | Accepted |  |
| 70 | Accepted order 1 is sent again through `accept_pinned_financed_tab` with a list that says size = 11 | No effect |  |
| 71 | Accepted order 1 is sent again through `accept_financed_tab` | No effect |  |
| 72 | The buyer's owner unpins agent 1's permission with an empty list | Accepted |  |
| 73 | The buyer's owner revokes agent 1's permission | Accepted |  |
| 74 | The seller reports 60,000 of completed work on order 1 | Accepted |  |
| 75 | A key with no role collects order 1's 60,000 from the lender's wallet to the seller | Paid |  |

The pin change, the unpin and the revocation left order 1's records byte-identical. The re-sent order in rows 70 and 71 changes no account and does not reach the Registry. The seller received exactly 60,000 and the lender's wallet paid exactly 60,000, in a transaction signed only by the collecting key. Order 1's record still carries the terms hash it was accepted with.

## The pinned order in the main run

Trace: [pinned-terms-trace.json.xz](pinned-terms-trace.json.xz), suite `pinned-terms`. Agent 1's permission is created without a pin, and the owner pins size = 10 in row 57.

| Order | Agent | Seller's terms | Permission at the time | Sent through | Outcome |
|---:|---|---|---|---|---|
| 1 | 1 | size = 10 | Pins size = 10 | `accept_pinned_financed_tab` | Accepted |
| 2 | 2 | size = 11 | Nothing pinned | `accept_financed_tab` | Accepted |
| 5 | 2 | size = 11 | Nothing pinned | `accept_pinned_financed_tab` | Accepted, then released by the seller |
| 3 | 1 | size = 10, colour = red, sixteen fields | Pins size = 10 and colour = red | `accept_pinned_financed_tab` | Accepted |
| 6 | 1 | An evidence order | Pins two terms, then unpinned | `accept_evidence_financed_tab` | Refused while pinned, accepted after the unpin |
| 4 | 1 | size = 11 | Pins two terms, then unpinned | `accept_financed_tab` | Refused while pinned, accepted after the unpin |

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

Rows 77 to 84 are order 1 with one thing changed, and row 85 is order 1 unchanged. In rows 81 and 82 the list shown is the list the signed terms hash to, so its names are the only thing wrong with it. After every refused order the trace makes the same assertions as in the created-pinned run: nothing held, nothing promised and no record at the order's addresses. The main run ends with five orders of 100,000 held against the budget and promised by the lender, nothing consumed, and no tokens moved out of the lender's wallet.

### The agent the owner did not pin

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 87 | Agent 2 submits order 2 with size = 11 through `accept_financed_tab` | Accepted |  |
| 88 | Agent 2 sends order 5, size = 11, with a list that says size = 10 | Refused | `TermsMismatch` 6052, PrincipalRegistry |
| 89 | Agent 2 submits order 5 with size = 11 through `accept_pinned_financed_tab` | Accepted |  |
| 90 | The seller releases order 5's unused 100,000 | Accepted |  |

### Two pinned terms

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 91 | The buyer's owner pins size = 10 and colour = red on agent 1's permission | Accepted |  |
| 92 | Order 3 with size = 10 and colour = blue | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 93 | Order 3 with a seventeenth term added to its sixteen | Refused | `TermsMismatch` 6052, PrincipalRegistry |
| 94 | Order 3, maximum 100,000, with size = 10 and colour = red among its sixteen terms | Accepted |  |

### Who can pin

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 61 | Agent 1 unpins its own permission, naming itself as the owner | Refused | `OwnerAuthority` 6001, PrincipalRegistry |
| 62 | Agent 1 changes the pin to size = 11, naming the owner without its signature | Refused | `OwnerAuthority` 6001, PrincipalRegistry |
| 63 | Agent 1 unpins its own permission, naming the owner without its signature | Refused | `OwnerAuthority` 6001, PrincipalRegistry |
| 64 | A key that is not the owner unpins agent 1's permission, naming itself as the owner | Refused | `OwnerAuthority` 6001, PrincipalRegistry |
| 65 | A key that is not the owner changes the pin to size = 11, naming the owner without its signature | Refused | `OwnerAuthority` 6001, PrincipalRegistry |
| 66 | A key that is not the owner unpins agent 1's permission, naming the owner without its signature | Refused | `OwnerAuthority` 6001, PrincipalRegistry |

The buyer's owner signs every accepted pin and unpin. It signs none of the orders.

### What a pin holds

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
| 14 | Order 2, a metered pool-funded order of 100,000 sent through `accept_metered_tab` | Refused | `TermsRequired` 6051, PrincipalRegistry |
| 15 | The buyer's owner unpins agent 1's permission with an empty list | Accepted |  |
| 16 | Order 1, the pool-funded order of 100,000 | Accepted |  |
| 17 | Order 2, the metered pool-funded order of 100,000 | Accepted |  |

## Passkey owners

Trace: [pinned-terms-passkey-trace.json.xz](pinned-terms-passkey-trace.json.xz), suite `pinned-terms-passkey`. The buyer's owner and the credit issuer's owner are passkey aliases, which cannot sign a transaction. Each approves one exact instruction with a P-256 proof. Each order has sixteen terms.

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 74 | The buyer's owner, a passkey, creates the agent's permission already pinned to size = 10, in one approved instruction | Accepted |  |
| 75 | Straight afterwards, order 1 with size = 11 through `accept_financed_tab`, signed by the agent, the seller and the credit issuer's passkey | Refused | `TermsRequired` 6051, PrincipalRegistry |
| 76 | Order 1, maximum 100,000, with size = 10 among its sixteen terms, signed by the same three | Accepted |  |
| 77 | The buyer's owner, a passkey, pins size = 10 and colour = red on the agent's permission | Accepted |  |
| 78 | Order 2 with colour = blue among its sixteen terms, with a seller fee and interest | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 79 | Order 2, maximum 100,000 with a seller fee and interest of up to 1,000, with colour = red among its sixteen terms, signed by the same three | Accepted |  |

Row 79 is the largest order in the record: 3,821 signed bytes.

## Who holds which key

| Role in the main and created-pinned runs | Key |
|---|---|
| Buyer's owner | `3BuW9SR5tG6VFK4MmkQQ3Ak8ny1K1Vv5Uz7is8Aa5pwG` |
| Agent 1 | `Cdkrk8tujFY6mTyGwFgKpnbiGc1hqtXCog1qvUdKAe6D` |
| Agent 2 | `5WcE8o73vmsSZXeeWTLm3ty3fAJKCnBWRF6VuKUme5nu` |
| Seller | `4MfyR4G3NWfVRDWo6iNAHDBZqWMgwZX6FNtMqEW3a9JT` |
| Lender's owner | `6TcyBfPdBt1kjsvDZLzmBFnuMaLWiTaAt4RjUr9VA5YD` |
| Credit issuer's owner | `FMUEmtxhU46GzhKF4FW9MLJdQWiLgjiXP9TYRWSrqTpV` |
| Fee payer of setup and order transactions | `AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9` |
| Key that is not the owner, in the refusals | `8bcRzB3v6PxbbtkVCiX9ceW2whwakA6gX7qvSYbeMHLq` |

| Role in the passkey run | Key |
|---|---|
| Buyer's owner, a passkey alias | `FZXmU3vnccKyM384A4S2dEA4GqVByq7i8tDKyyaX1kBf` |
| Credit issuer's owner, a passkey alias | `DXugwUsviZJK1YWbe2m2TdxoPNTULtiGC5HiqDJ7J9sR` |
| Agent | `Cdkrk8tujFY6mTyGwFgKpnbiGc1hqtXCog1qvUdKAe6D` |
| Seller | `4MfyR4G3NWfVRDWo6iNAHDBZqWMgwZX6FNtMqEW3a9JT` |

The receipt lists the keys of every run.

## The rule in the source

The six program files this change touches are published here as they stand at the commit the binaries were built from, under [source/](source). The source calls a permission a `Mandate`.

A term, the two counts, and what a pin may be:

```rust
// programs/principal-registry/src/state.rs
117: pub const MAX_REQUIRED_TERMS: usize = 4;
118: pub const MAX_ORDER_TERMS: usize = 16;
123: pub struct Term {
124:     pub name: [u8; 32],
125:     pub value: [u8; 32],
130: pub fn names_ascend(terms: &[Term]) -> bool {
131:     terms.windows(2).all(|pair| pair[0].name < pair[1].name)
172: pub fn pinnable(terms: &[Term]) -> bool {
173:     terms.len() <= MAX_REQUIRED_TERMS && names_ascend(terms) && terms.iter().all(|term| term.name != [0; 32])
```

The order terms hash is over the whole list:

```rust
// programs/principal-registry/src/state.rs
178: pub fn order_terms_hash(terms: &[Term]) -> [u8; 32] {
181:     parts.extend([b"DEXTER:ORDER-TERMS:2".as_slice(), &count]);
183:         parts.extend([term.name.as_slice(), &term.value]);
```

A permission is created already pinned, and the owner changes a pin:

```rust
// programs/principal-registry/src/lib.rs
165:     pub fn create_pinned_mandate(ctx: Context<CreateMandate>, args: CreateMandateArgs,
166:         required_terms: Vec<Term>) -> Result<()> {
167:         create_permission(ctx, args, Some(required_terms))
275:             require!(!terms.is_empty() && pinnable(terms), RegistryError::InvalidPolicy);
219:     pub fn pin_mandate_terms(ctx: Context<ManageMandate>, expected_revision: u64,
220:         required_terms: Vec<Term>) -> Result<()> {
222:         require_owner_authority(&ctx.accounts.principal,&ctx.accounts.owner,ctx.remaining_accounts)?;
225:         require!(pinnable(&required_terms), RegistryError::InvalidPolicy);
```

The Registry checks the order:

```rust
// programs/principal-registry/src/lib.rs
351:     match &shown {
352:         None => require!(mandate.required_terms_count == 0, RegistryError::TermsRequired),
353:         Some(order) => {
354:             require!((1..=MAX_ORDER_TERMS).contains(&order.terms.len()), RegistryError::TermsMismatch);
355:             require!(names_ascend(&order.terms), RegistryError::TermsMismatch);
356:             require!(completed_run_policy(&order.namespace, &order.unit, order.unit_price,
357:                 &order_terms_hash(&order.terms)) == terms.external_terms_hash, RegistryError::TermsMismatch);
358:             require!(mandate.pinned_terms().iter().all(|required| order.terms.contains(required)),
359:                 RegistryError::TermsNotPermitted);
```

Only ClaimsCore's authority can call the Registry's reserve instructions:

```rust
// programs/principal-registry/src/lib.rs
563:     #[account(seeds = [REGISTRY_CONSUMER_SEED], bump, seeds::program = CORE_ID)]
564:     pub core_authority: Signer<'info>,
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

The compiled programs ran in LiteSVM from solders 0.29.0 under Python 3.12.3, with signature verification on. The main, created-pinned, passkey and recoverable runs use native v1 transactions, and the pool-funded run uses v0 transactions through a lookup table the test writes. The passkey run's setup writes a test upgrade authority into the Registry's loader metadata. Each run has one synthetic six-decimal SPL token, every key is a disposable test key, and no run moves the clock. The runs started on November 18, 2026 at 12:49 PM US Eastern. They sent no transaction to a public network.

Every other November 18 record ran commit `e55c0ad3a224a1c4e72de4c283161345d438caaf` of the Dexter Account repository. This record ran commit `9ab113008822d698ddde00db8d207b216a85a9db`, which is that commit plus this change, and the complete source at `e55c0ad` is in [../source/](../source/README.md). Six of the 69 build inputs differ between the two commits, and they are the six files under [source/](source). PoolIssuer's binary is the same in both.

| Program | Address | SHA-256 of the binary |
|---|---|---|
| ClaimsCore | `727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz` | `1d866dede6ce0a5f449f8f845c77eedae31858c8d505d82ae608f30b732983be` |
| PrincipalRegistry | `9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G` | `0aad6b0090bcc1a0c5fc1576f73771824869199c281dafea47e386bb8bd609cd` |
| PoolIssuer | `3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm` | `6804757b94ce2da8854388a1f748b614296207682db27cde85958f868b726e65` |
| Swig | `swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB` | `bb744d9811c392e3f343952d1a99bef9c97aa1936248a23b9d7efc1adb8939c1` |

The main, created-pinned, passkey and recoverable traces record the SHA-256 of all 69 source inputs and of each binary, and the pool-funded trace records the three binary hashes. The source lines in the case index (`error_source`) are lines of the six files.

| Run | Transactions | Refusals | Assertions | SHA-256 of the JSON inside the trace |
|---|---:|---:|---:|---|
| Main | 100 | 27 | 288 | `40f72c94f8c1a6e62c055c610dea461505fe28a4471c6746b9dc0c155188d94d` |
| Created pinned | 76 | 10 | 163 | `7868320034854a8f02fb89c08453548639a7e0c3581bb6965ddb3aeac6e28f2c` |
| Passkey | 80 | 7 | 124 | `b74f62b3c53dc2ee0f57af610077297c39bf19a7b75d7467423e935a4a9bb090` |
| Recoverable | 65 | 1 | 65 | `5173d31c5e11d1cba96a7a9a3980edcff874f3a5140f57e9488c5b6cd0063483` |
| Pool-funded | 18 | 2 | 14 | `0ef6fa7da2efc20f380218f60ac41809cc6a3f97685b873ee267ea00ff773f64` |

Five of the passkey run's seven refusals are in the setup it shares with the Account repository's passkey test. The other two are rows 75 and 78.

## Files

| File | Contents |
|---|---|
| [pinned-terms-trace.json.xz](pinned-terms-trace.json.xz) | The main run's 100 transactions: signed bytes, result, program log, and the observed accounts before and after. About 11 MB uncompressed. |
| [pinned-terms-created-pinned-trace.json.xz](pinned-terms-created-pinned-trace.json.xz), [pinned-terms-passkey-trace.json.xz](pinned-terms-passkey-trace.json.xz), [pinned-terms-recoverable-trace.json.xz](pinned-terms-recoverable-trace.json.xz), [pinned-terms-pool-funded-trace.json.xz](pinned-terms-pool-funded-trace.json.xz) | The four other runs, in the same form. |
| [case-index.json](case-index.json) | One entry per transaction of the five runs: run, name, outcome, error name and number, the program that raised it and the source line its log names. |
| [pinned-terms-receipt.json](pinned-terms-receipt.json) | Counts and trace hashes for each run, the role keys, the fingerprints and the binary hashes. |
| [pinned-terms.log](pinned-terms.log) | The six lines the runs printed. |
| [pinned_terms.py](pinned_terms.py) | The test module. It reuses the setup and the refusal checks in the commit record's [commit_legs.py](../commit_legs.py) and builds on four of the Account repository's test fixtures. |
| [order_terms_vectors.json](order_terms_vectors.json) | Three test vectors for the fingerprints and the order terms hash, and one list with a repeated name. |
| [source/](source) | The six program source files and their [notice](source/NOTICE.md). |
| [provenance.json](provenance.json) | The source commits and the source and published hash of each file. |
| [SHA256SUMS](SHA256SUMS) | Hashes of the eighteen files above. |

The traces, case index, receipt, test module, vectors and six source files are byte-identical to their source commits. In the log, the absolute workspace path is replaced with `<fixture-workspace>` in five places. The source files keep their existing copyright, and this release adds no license grant for them. Each of the four native traces lists in `fixture_inputs` the hash of every test file the run used.

## Check a row yourself

Download a trace, then print any row's name, result, state comparison and program log. This reads row 56 of the created-pinned run, the order for size 11:

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
" pinned-terms-created-pinned-trace.json.xz 56
```

```text
refused: order 1 with size = 11 through the pinned instruction
expected: reject | error: {'InstructionError': [0, {'Custom': 6053}]}
state unchanged: True
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz invoke [1]
Program log: Instruction: AcceptPinnedFinancedTab
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G invoke [2]
Program log: Instruction: ReservePinned
Program log: AnchorError thrown in programs/principal-registry/src/lib.rs:358. Error Code: TermsNotPermitted. Error Number: 6053. Error Message: Order terms do not contain a term the owner pinned.
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G failed: custom program error: 0x17a5
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz failed: custom program error: 0x17a5
```

The lines left out here are system program calls and compute-unit counts. `transaction_base64` in the same row is the signed transaction, and `observed_before` and `observed_after` hold each observed account's owner, lamports and data.

To check the files against their hashes, and a decompressed trace against `trace_sha256` in the receipt:

```sh
sha256sum -c SHA256SUMS
xz -dc pinned-terms-created-pinned-trace.json.xz | sha256sum
```

The second command prints `7868320034854a8f02fb89c08453548639a7e0c3581bb6965ddb3aeac6e28f2c`.

The [commit record](../README.md) covers the three checks an order must pass, and this record is the permission check on the order's own terms. The [claim guide](../../../articles/the-account-inversion/README.md) maps the article to its records.
