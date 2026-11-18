# An owner pins who signs for delivery, and the payment follows that signer

An owner can pin who signs for delivery as well as the terms of the deal. On November 18, 2026, on compiled Account programs, a buyer's owner created its agent's permission already pinned to size = 10 and to the carrier as the one who signs for delivery, in one instruction. The agent and the seller then submitted evidence orders, on which payment waits for the delivery signer's signed report. The order for size 11 naming the carrier was refused. The order for size 10 naming the seller, and the one naming an inspector, were refused. None of the refused orders held or recorded anything: no budget hold, no lender promise, no order record and no bill. The order for size 10 naming the carrier was accepted. The carrier signed for 900 against the seller's claim of 1,400, the bill was 900, and a key with no role in the order collected exactly 900 from the lender's wallet to the seller. A report of delivery signed by the seller was refused.

| Run | Row | What happened | Outcome | Error |
|---|---:|---|---|---|
| Main | 54 | The buyer's owner creates agent 1's permission already pinned to size = 10 and delivery_signer = the carrier, in one instruction | Accepted |  |
| Main | 56 | Agent 1 and the seller submit evidence order 1 for size = 11 naming the carrier | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| Main | 57 | Evidence order 1 for size = 10 naming the seller to sign for delivery | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| Main | 58 | Evidence order 1 for size = 10 naming an inspector to sign for delivery | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| Main | 64 | Evidence order 1 for size = 10 naming the carrier | Accepted |  |
| Main | 75 | The seller signs the report of 900 delivered on order 1 | Refused | `BillingSignature` 6063, ClaimsCore |
| Main | 76 | The carrier signs the report of 900 delivered on order 1 | Accepted |  |
| Main | 77 | The seller signs a claim of 1,400 on order 1 | Accepted |  |
| Main | 78 | A key with no role finalizes order 1, and the bill is 900 | Accepted |  |
| Main | 79 | A key with no role collects the bill of 900 from the lender's wallet to the seller | Paid |  |

When the owner pinned the buyer's own key as the delivery signer, the order naming the carrier was refused and the same order naming the buyer was accepted (rows 73 and 74). The same pins held on a recoverable order, whose delivery signer is a Registry principal, and with passkey owners.

The programs ran on a local Solana virtual machine with a synthetic token. The record has three runs with 226 signed transactions, and all 547 recorded assertions passed. After each of the 17 refusals in the tables below, every account the transaction could write, other than the fee payer, held the same bytes as before. [case-index.json](case-index.json) names every transaction of the three runs, and the three traces hold each one's signed bytes, program log and account state.

## How the terms and the delivery signer are bound

An evidence order names its delivery signer in its billing policy, as the policy's `source`. Only that key's signed report can say what was delivered. The order's terms are a list of pairs of 32-byte fingerprints, one of a field's name and one of its value, computed as in the [pinned-terms record](../pinned-terms/README.md#how-an-order-shows-its-terms). One name is reserved:

```text
name fingerprint        = sha256("DEXTER:ORDER-TERM-NAME:1" || name)
value fingerprint       = sha256("DEXTER:ORDER-TERM-VALUE:1" || value)
order terms hash        = sha256("DEXTER:ORDER-TERMS:2" || u8(count) || name_1 || value_1 || ... || name_n || value_n)
delivery_signer         = name fingerprint of "delivery_signer"; its value is the signer's 32 key bytes
evidence order          = policy.request_hash is the order terms hash; policy.source is the delivery signer
```

- The name fingerprint of `delivery_signer` is `59f0d4e2b287345eda1d13cd607e9f34d76c143108ac72494012e10fa214c370`. Its value is the 32 bytes of the key that signs for delivery, not a fingerprint of text. An owner pins it like any other term, with `create_pinned_mandate` or `pin_mandate_terms`.
- The list an evidence order shows must hash to its billing policy's `request_hash`. The policy hash commits to `request_hash` and to `source`, and the terms the agent and the seller sign commit to the policy hash. The Registry recomputes the policy hash from the policy it is shown.
- ClaimsCore `accept_pinned_evidence_financed_tab` and `accept_pinned_recoverable_financed_tab` take the accounts and arguments of `accept_evidence_financed_tab` and `accept_recoverable_financed_tab` plus the order's list of terms. They call the Registry's `reserve_pinned_billing` inside the same instruction.
- The Registry checks four things about the list. Its names ascend, so every name appears once. It hashes to the `request_hash` of a policy the signed terms commit to. Either failure is `TermsMismatch`. Every pinned name is in it with the pinned value, or the order fails with `TermsNotPermitted`. A listed `delivery_signer` is the policy's `source`, or the order fails with `DeliverySignerMismatch`.
- An order that names no delivery signer cannot list one. Through `accept_pinned_financed_tab`, a list that carries `delivery_signer` fails with `DeliverySignerMismatch`, and a list without it fails `TermsNotPermitted` on a permission that pins it.
- An instruction that shows no list refuses a pinned permission with `TermsRequired`.
- The report the delivery signer signs carries a `request_hash`, and ClaimsCore admits the report only when it equals the policy's. The carrier's report on order 1 carries order 1's terms hash.
- On a recoverable order the policy's `source` is the source's Registry principal, so the delivery signer is that principal and the owner pins the principal's address.

Order 1 shows five terms, listed here in ascending order of name fingerprint:

| Field | Value | Name fingerprint | Value |
|---|---|---|---|
| size | 10 | `05b37e4652b5bc6d4e0b3c9b3853eb3fa49ef68411548147e0c9ff5b215c84fb` | `67c64cd99ee9c2941257cd3c1d83fabde2526b1aff20a1cf54758f495d572416` |
| item | trail shoe | `0a4afc41d4416fc657471728707cdc8bf7c2c5a4496ebec58ecf81b8cc766ab1` | `593f316cdacdd20aee24f82f2eed39a68db17719b2ef35f57a78248fdb72de8a` |
| delivery_signer | the carrier, `GFKfRLPKHYRyARLSTc4p94vukKPMFAkSrQT5j55WYoy2` | `59f0d4e2b287345eda1d13cd607e9f34d76c143108ac72494012e10fa214c370` | `e28a8970753332bd72fef413e6b0b2ef1b4aadda7aa2c141f233712a6876b351`, the key's bytes |
| ship_to | warehouse 4 | `6b57b1ca5def698d58053edcf071a8138c4055cdba3f477e053ce2cb09adb4be` | `e69e1a3b5f55190f989e039e738632a6428dbe7603bed236dda22a26ada443f6` |
| colour | red | `afbc890ab78fe0170cddb1cf432db91c94783f184e2562d512b77abe68ba8a28` | `20b69583a4955ce56a5f643520e2290f5905722d38959b5f8c244fd5b424b6d3` |

Its order terms hash is `810b7088023b25dabe41dfb1679a8c7141a012963cb6f8a70850610e1d99c8e2`. The main trace records it as the `request_hash` of order 1's bill (assertion 125) and of the carrier's report (assertion 240). This reproduces it from the four fields in [order_terms_vectors.json](order_terms_vectors.json) and the carrier's key:

```sh
python3 -c "
import hashlib, json, sys
v = json.load(open('order_terms_vectors.json'))
d = v['delivery_signer']
b58 = lambda s: sum(58**i * '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'.index(c) for i, c in enumerate(reversed(s))).to_bytes(32, 'big')
h = lambda domain, text: hashlib.sha256(v[domain].encode() + text.encode()).digest()
terms = [(h('name_domain', n), h('value_domain', x)) for n, x in d['vectors'][0]['entries']]
terms = sorted(terms + [(bytes.fromhex(d['name_fingerprint']), b58(sys.argv[1]))])
print(hashlib.sha256(v['terms_domain'].encode() + bytes([len(terms)]) + b''.join(a + b for a, b in terms)).hexdigest())
" GFKfRLPKHYRyARLSTc4p94vukKPMFAkSrQT5j55WYoy2
```

With the buyer's owner key `3BuW9SR5tG6VFK4MmkQQ3Ak8ny1K1Vv5Uz7is8Aa5pwG` it prints `314dc776b20f956f19118f6ec309ae93f6da3ed9563c394d686378eefd4f86fd`, order 6's terms hash (assertion 236). With the vector's own signer, `4wBqpZM9xaSheZzJSMawUKKwhdpChKbZ5eu5ky4Vigw`, it prints the vector's hash, `46106cd7261eb62f3f079bcb60bea3e1f4ea0f51ab95d41c94af9d4286d7423d`.

## The scenario

One buyer, two agents, one seller, a carrier, an inspector, one credit issuer and one lender, with the lender's wallet starting at 1,000,000. Evidence orders have a maximum of 2,000 at a fixed price of 1 per unit. Work must finish within 20 seconds of acceptance, the delivery signer may report until 40 seconds and the seller may claim until 45. Orders 4 and 5 go through `accept_pinned_financed_tab`, which names no delivery signer, with a maximum of 100,000. Amounts are base units of the synthetic token. The seller's terms for every order have four fields: item = trail shoe, size, colour = red and ship_to = warehouse 4. Evidence orders add the `delivery_signer` term.

| Order | Agent | Permission at the time | Delivery signer named | Sent through | Outcome |
|---:|---|---|---|---|---|
| 1 | 1 | Pins size = 10 and the carrier | The carrier | `accept_pinned_evidence_financed_tab` | Accepted, billed 900 and paid |
| 2 | 2 | Nothing pinned | The carrier | `accept_evidence_financed_tab` | Accepted |
| 3 | 2 | Nothing pinned | The carrier | `accept_pinned_evidence_financed_tab` | Accepted |
| 4 | 2 | Nothing pinned | None | `accept_pinned_financed_tab` | Refused with a `delivery_signer` term, accepted without |
| 5 | 1 | Pins size = 10 and the carrier | None | `accept_pinned_financed_tab` | Refused |
| 6 | 1 | Pins size = 10 and the buyer's own key | The buyer's own key | `accept_pinned_evidence_financed_tab` | Refused naming the carrier, accepted naming the buyer |

In the tables below, "Row" is the zero-based position in a trace's `transactions` array and equals `index` in the case index for that run's `suite`. "Error" is the name and number in the program log, followed by the program that raised it. Every order refusal reaches the Registry through ClaimsCore, so the Registry fails first and ClaimsCore fails with the same code.

## The carrier and size 10, pinned at creation

Trace: [pinned-delivery-trace.json.xz](pinned-delivery-trace.json.xz), suite `pinned-delivery`.

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 54 | The buyer's owner creates agent 1's permission already pinned to size = 10 and delivery_signer = the carrier, in one instruction | Accepted |  |
| 56 | Agent 1 and the seller submit evidence order 1 for size = 11 naming the carrier | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 57 | Evidence order 1 for size = 10 naming the seller's own key to sign for delivery | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 58 | Evidence order 1 for size = 10 naming an inspector's key to sign for delivery | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 59 | Evidence order 1 naming the carrier shows size = 10, and the list its policy commits to says size = 11 | Refused | `TermsMismatch` 6052, PrincipalRegistry |
| 60 | Evidence order 1 for size = 10 naming the carrier, through `accept_evidence_financed_tab`, which shows no list | Refused | `TermsRequired` 6051, PrincipalRegistry |
| 61 | Evidence order 1 whose list names delivery_signer twice, as the carrier and as the seller | Refused | `TermsMismatch` 6052, PrincipalRegistry |
| 62 | Evidence order 1 lists delivery_signer = the carrier, and its policy names the seller to sign for delivery | Refused | `DeliverySignerMismatch` 6054, PrincipalRegistry |
| 63 | Evidence order 1 for size = 10 naming the carrier, with the delivery_signer term left out of its list | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 64 | Agent 1 and the seller submit evidence order 1, maximum 2,000, for size = 10 naming the carrier | Accepted |  |
| 65 | Accepted evidence order 1 is sent again | No effect |  |

Rows 56 to 63 are order 1 with one thing changed, and row 64 is order 1 unchanged. In rows 57 and 58 the order names the other signer in its policy and in its list together. Every refused order targets the same order, reservation and bill addresses as row 64. After each refusal the trace asserts that the budget hold, the lender's promise, the facility, the credit line and the lender's wallet are unchanged, and that no order record, Registry reservation, Registry operation record, receipt or bill exists at the order's addresses.

For accepted order 1 the trace asserts that the bill records the carrier as the delivery signer, that the bill's policy carries the hash of the list shown, that the order record commits to the policy hash and the Registry reservation to the signed terms, that the carrier neither signed the order nor appeared among its accounts, and that the buyer's owner did not sign it. The twelve role keys are all different.

## The payment follows the carrier

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 75 | The seller signs the report of 900 delivered on order 1 | Refused | `BillingSignature` 6063, ClaimsCore |
| 76 | The carrier signs the report of 900 delivered on order 1 | Accepted |  |
| 77 | The seller signs a claim of 1,400 on order 1 | Accepted |  |
| 78 | A key with no role finalizes order 1 at 46 seconds, after both windows close | Accepted |  |
| 79 | A key with no role collects order 1's bill of 900 from the lender's wallet to the seller | Paid |  |

The bill is the smallest of the maximum (2,000), the seller's claim (1,400) and what the carrier signed for (900): 900. The other 1,100 was released. After row 79 the seller held exactly 900 more and the lender's wallet exactly 900 less, and order 1's record shows 900 paid, 900 owed by the buyer and 1,100 released. Rows 76 to 79 each carry one transaction signature, that of the key in the table below that submits, finalizes or collects. The carrier's signature is inside its report, and the seller's inside its claim.

## Agent 2, whose permission pins nothing

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 66 | Agent 2 submits evidence order 2 of 2,000 naming the carrier through `accept_evidence_financed_tab` | Accepted |  |
| 67 | Agent 2 submits evidence order 3 of 2,000 for size = 10 naming the carrier through `accept_pinned_evidence_financed_tab` | Accepted |  |
| 68 | Agent 2 submits order 4 through `accept_pinned_financed_tab`, which names no delivery signer, with a list that says delivery_signer = the carrier | Refused | `DeliverySignerMismatch` 6054, PrincipalRegistry |
| 69 | Order 4, maximum 100,000, with the same seller's terms and no delivery_signer term | Accepted |  |

## An order that names no delivery signer, on the pinned permission

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 70 | Agent 1 sends order 5 for size = 10 through `accept_pinned_financed_tab` with a list that says delivery_signer = the carrier | Refused | `DeliverySignerMismatch` 6054, PrincipalRegistry |
| 71 | Order 5 through `accept_pinned_financed_tab` without the delivery_signer term | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |

## The buyer's own key as the delivery signer

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 72 | The buyer's owner changes agent 1's pin to size = 10 and delivery_signer = the buyer's own key | Accepted |  |
| 73 | Evidence order 6 for size = 10 naming the carrier | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 74 | Evidence order 6, maximum 2,000, for size = 10 naming the buyer's own key | Accepted |  |

After row 72 the permission is at revision 2. The main run ends with orders 2, 3 and 6 of 2,000 and order 4 of 100,000 held against the budget, and order 1 consumed at 900.

## A recoverable order

Trace: [pinned-delivery-recoverable-trace.json.xz](pinned-delivery-recoverable-trace.json.xz), suite `pinned-delivery-recoverable`. The seller is paid into its continuing receiving custody, and the delivery signer is the source's Registry principal, `6GSFTM5NmQwPWJyr1ZZeCHr2LRJKhEmy52DVJQbBZXZR`.

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 61 | The buyer's owner pins size = 10 and delivery_signer = the source's principal on the agent's permission | Accepted |  |
| 62 | Recoverable order 1 for size = 10 through `accept_recoverable_financed_tab`, which shows no list | Refused | `TermsRequired` 6051, PrincipalRegistry |
| 63 | Recoverable order 1 for size = 11 | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 64 | Recoverable order 1 whose delivery_signer term is the key of the principal's current owner in place of the principal | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 65 | Recoverable order 1, maximum 2,000, for size = 10 naming the source's principal | Accepted |  |

Rows 62 to 64 are order 1 with one thing changed. After each refusal the trace asserts that the budget is unchanged and that no order record, reservation, bill or authority binding exists.

## Passkey owners

Trace: [pinned-delivery-passkey-trace.json.xz](pinned-delivery-passkey-trace.json.xz), suite `pinned-delivery-passkey`. The buyer's owner and the credit issuer's owner are passkey aliases, which cannot sign a transaction. Each approves one exact instruction with a P-256 proof.

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 75 | The buyer's owner, a passkey, creates the agent's permission already pinned to size = 10 and delivery_signer = the carrier | Accepted |  |
| 76 | Evidence order 1 for size = 11 naming the carrier, signed by the agent, the seller and the credit issuer's passkey | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 77 | Evidence order 1, maximum 2,000, for size = 10 naming the carrier, signed by the same three | Accepted |  |
| 78 | The buyer's owner, a passkey, creates a second permission already pinned to size = 10 and delivery_signer = the source's principal | Accepted |  |
| 79 | Recoverable order 2, maximum 2,000, for size = 10 naming the source's principal, signed by the same three | Accepted |  |

Each accepted order was approved by one passkey proof, the credit issuer's. Row 79 is the largest transaction in the record: 3,717 signed bytes.

## Who holds which key

| Role in the main run | Key |
|---|---|
| Buyer's owner | `3BuW9SR5tG6VFK4MmkQQ3Ak8ny1K1Vv5Uz7is8Aa5pwG` |
| Agent 1 | `Cdkrk8tujFY6mTyGwFgKpnbiGc1hqtXCog1qvUdKAe6D` |
| Agent 2 | `5WcE8o73vmsSZXeeWTLm3ty3fAJKCnBWRF6VuKUme5nu` |
| Seller | `4MfyR4G3NWfVRDWo6iNAHDBZqWMgwZX6FNtMqEW3a9JT` |
| Carrier | `GFKfRLPKHYRyARLSTc4p94vukKPMFAkSrQT5j55WYoy2` |
| Inspector | `FLCbA5rPHLmgEeQSg1CiewVizRGw6Gy5MXc1m17HyxUG` |
| Lender's owner | `6TcyBfPdBt1kjsvDZLzmBFnuMaLWiTaAt4RjUr9VA5YD` |
| Credit issuer's owner | `FMUEmtxhU46GzhKF4FW9MLJdQWiLgjiXP9TYRWSrqTpV` |
| Fee payer of setup and order transactions | `AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9` |
| Key that submits the signed report and claim | `92CudvFbL7Tyw2RkWC7NU1ehdNBSjy8FEG1TsJaN8wgz` |
| Key that finalizes | `4ap9kAPdHiV82d8D73mrtEgwGxBbUxJgyFr1fRGLhide` |
| Key that collects | `6jQ7iw7szEgN6hp49SAAMagYSYdDAZTQRDF3b8vTLNqM` |

The buyer's owner signs the pins and none of the orders. The carrier's only signature is on its own report.

| Role in the recoverable and passkey runs | Key |
|---|---|
| Buyer's owner in the recoverable run | `3BuW9SR5tG6VFK4MmkQQ3Ak8ny1K1Vv5Uz7is8Aa5pwG` |
| Buyer's owner in the passkey run, a passkey alias | `FZXmU3vnccKyM384A4S2dEA4GqVByq7i8tDKyyaX1kBf` |
| Credit issuer's owner in the passkey run, a passkey alias | `DXugwUsviZJK1YWbe2m2TdxoPNTULtiGC5HiqDJ7J9sR` |
| Agent | `Cdkrk8tujFY6mTyGwFgKpnbiGc1hqtXCog1qvUdKAe6D` |
| Seller, and the seller's owner in the recoverable run | `4MfyR4G3NWfVRDWo6iNAHDBZqWMgwZX6FNtMqEW3a9JT` |
| Carrier, in the passkey run | `GFKfRLPKHYRyARLSTc4p94vukKPMFAkSrQT5j55WYoy2` |
| Owner of the source's principal, in the recoverable run | `7nLFtY5yiR4CZnkB6bYMth3bbAns9DnFhtgjHGqcCiRn` |

The receipt lists the keys of every run.

## The rule in the source

The seven program files that differ from source already published are here as they stand at the commit the binaries were built from, under [source/](source). The source calls a permission a `Mandate`, the delivery signer the `source` and the seller the `supplier`.

The reserved name, and the two ways an order's signed terms commit to its list:

```rust
// programs/principal-registry/src/state.rs
193: pub const DELIVERY_SIGNER_NAME: [u8; 32] = [89, 240, 212, 226, 178, 135, 52, 94, 218, 29, 19, 205,
194:     96, 126, 159, 52, 215, 108, 20, 49, 8, 172, 114, 73, 64, 18, 225, 15, 162, 20, 195, 112];
211:     pub(crate) fn commits_to(&self, signed: &TabTerms) -> bool {
212:         let list = order_terms_hash(&self.terms);
216:             Commitment::Billing { bill, claim, policy } => policy.request_hash == list
217:                 && billing_policy_digest(&signed.domain, bill, claim, policy).is_ok_and(|hash|
218:                     completed_run_policy(&BILLING_NAMESPACE, &BILLING_SETTLEMENT_UNIT, 1, &hash) == signed.external_terms_hash),
```

A listed `delivery_signer` must carry the key the order names, and an order that names none cannot list one:

```rust
// programs/principal-registry/src/state.rs
224:     pub(crate) fn names_its_delivery_signer(&self) -> bool {
225:         let named = match &self.commitment {
226:             Commitment::Run { .. } => None,
227:             Commitment::Billing { policy, .. } => Some(policy.source.to_bytes()),
229:         self.terms.iter().all(|term| term.name != DELIVERY_SIGNER_NAME || Some(term.value) == named)
```

The Registry checks the order:

```rust
// programs/principal-registry/src/lib.rs
356:     match &shown {
357:         None => require!(mandate.required_terms_count == 0, RegistryError::TermsRequired),
358:         Some(order) => {
359:             require!((1..=MAX_ORDER_TERMS).contains(&order.terms.len()), RegistryError::TermsMismatch);
360:             require!(names_ascend(&order.terms), RegistryError::TermsMismatch);
361:             require!(order.commits_to(terms),
362:                 RegistryError::TermsMismatch);
363:             require!(mandate.pinned_terms().iter().all(|required| order.terms.contains(required)),
364:                 RegistryError::TermsNotPermitted);
366:             require!(order.names_its_delivery_signer(), RegistryError::DeliverySignerMismatch);
```

The Registry's entry point for an evidence order, and ClaimsCore's two instructions that show the list:

```rust
// programs/principal-registry/src/lib.rs
250:     pub fn reserve_pinned_billing(ctx: Context<Reserve>, args: AcceptArgs, bill: Pubkey, claim: Pubkey,
251:         policy: BillingPolicy, order_terms: Vec<Term>) -> Result<()> {
// programs/claims-core/src/lib.rs
125:     pub fn accept_pinned_evidence_financed_tab<'info>(ctx:Context<'info,AcceptEvidenceFinancedTab<'info>>,
139:     pub fn accept_pinned_recoverable_financed_tab<'info>(ctx:Context<'info,AcceptRecoverableFinancedTab<'info>>,
// programs/claims-core/src/evidence_billing.rs
182:     let order_terms = match order_terms {
183:         None => OrderTerms::Hidden,
184:         Some(terms) => OrderTerms::Billing { bill: bill_key, policy: Box::new(policy.clone()), terms },
// programs/claims-core/src/funding.rs
577:     match order_terms {
578:         OrderTerms::Hidden=>principal_registry::cpi::reserve(reserve,accepted)?,
579:         OrderTerms::Run(terms)=>principal_registry::cpi::reserve_pinned(reserve,accepted,namespace,service_unit,unit_price,terms)?,
580:         OrderTerms::Billing{bill,policy,terms}=>principal_registry::cpi::reserve_pinned_billing(reserve,accepted,bill,a.claim.key(),*policy,terms)?,
```

A delivery report is admitted only with a signature by the policy's `source` (row 75 is refused at line 231), and only when its `request_hash` is the policy's. The policy type and the report check are unchanged from [../source/](../source/README.md):

```rust
// programs/claims-core/src/evidence_billing.rs
231:     require!(d[0] == 1 && d[1] == 0 && word(2) == 16 && word(4) == u16::MAX
234:         && &d[80..112] == key.as_ref() && &d[112..144] == digest,
235:         CoreError::BillingSignature);
252:     verify_signature(&a.instructions, signature_ix_index, &a.bill.policy.source, &digest)?;
// crates/contracts/src/billing.rs, in ../source/
 37: pub struct BillingPolicy {
 39:     pub source: Pubkey,
 44:     pub request_hash: [u8; 32],
171:             || report.request_hash != self.request_hash
243: pub fn billing_policy_digest(domain: &[u8; 32], bill: &Pubkey, claim: &Pubkey,
```

Bill finalization takes the smallest of the three numbers, as in the [named-signer record](../named-signer/README.md#the-rule-in-the-source): `deterministic_bill_amount`, line 224 of [billing.rs](../source/foundation/onchain/crates/contracts/src/billing.rs), called at line 313 of [evidence_billing.rs](source/foundation/onchain/programs/claims-core/src/evidence_billing.rs).

## Where it ran

The compiled programs ran in LiteSVM from solders 0.29.0 under Python 3.12.3, with signature verification on and native v1 transactions. The passkey run's setup writes a test upgrade authority into the Registry's loader metadata. Each run has one synthetic six-decimal SPL token and every key is a disposable test key. The main run moves the test clock to 46 seconds after order 1's acceptance before finalization. The runs started on November 18, 2026 at 3:01 PM US Eastern. They sent no transaction to a public network.

This record ran commit `8c4667f16b30b96435dfcdb057b3fb7d27e914b4` of the Dexter Account repository, which reached its main branch in merge `641246a`. Its parent `8333b7b` has the same program source as the pinned-terms record's commit `9ab1130`: the complete source at `e55c0ad` in [../source/](../source/README.md) with the six files in [../pinned-terms/source/](../pinned-terms/source/NOTICE.md) at their paths. Seven of the 69 build inputs differ from those published files, and they are the seven files under [source/](source). The other 62 are identical to files under [../source/](../source/README.md). PoolIssuer's binary is the same as in every other November 18 record.

| Program | Address | SHA-256 of the binary |
|---|---|---|
| ClaimsCore | `727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz` | `7d2d318af95707ba937b47fcb5f89a1a4c6f4a278edab36ee6e9894426c5d187` |
| PrincipalRegistry | `9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G` | `56fa8a3409a963665e033f72a0cb086613010033bfe25054db41d06eebc4f7fa` |
| PoolIssuer | `3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm` | `6804757b94ce2da8854388a1f748b614296207682db27cde85958f868b726e65` |
| Swig | `swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB` | `bb744d9811c392e3f343952d1a99bef9c97aa1936248a23b9d7efc1adb8939c1` |

Each trace records the SHA-256 of all 69 source inputs and of each binary. The source lines in the case index (`error_source`) are lines of the files under [source/](source).

| Run | Transactions | Refusals | Assertions | SHA-256 of the JSON inside the trace |
|---|---:|---:|---:|---|
| Main | 80 | 13 | 318 | `9b3568aae256f51734686a4c52f9682d7b4c8ebc18672123533214c5236dbd95` |
| Recoverable | 66 | 3 | 95 | `4abfaf47bdaa5297d2557d836144f4298e798b0b0a98e98b0036a077aaee412e` |
| Passkey | 80 | 6 | 134 | `ce9360100e9d04d3fd2720be20ff638f482cbcccdd48b13fa8293a4365482e52` |

Five of the passkey run's six refusals are in the setup it shares with the Account repository's passkey test. The other is row 76.

## Files

| File | Contents |
|---|---|
| [pinned-delivery-trace.json.xz](pinned-delivery-trace.json.xz) | The main run's 80 transactions: signed bytes, result, program log, and the observed accounts before and after. About 8 MB uncompressed. |
| [pinned-delivery-recoverable-trace.json.xz](pinned-delivery-recoverable-trace.json.xz), [pinned-delivery-passkey-trace.json.xz](pinned-delivery-passkey-trace.json.xz) | The two other runs, in the same form. |
| [case-index.json](case-index.json) | One entry per transaction of the three runs: run, name, outcome, error name and number, the program that raised it and the source line its log names. |
| [pinned-delivery-receipt.json](pinned-delivery-receipt.json) | Counts and trace hashes for each run, the role keys, the fingerprints and the binary hashes. |
| [pinned-delivery.log](pinned-delivery.log) | The four lines the runs printed. |
| [pinned_delivery.py](pinned_delivery.py) | The test module. It builds on `pinned_terms.py` at the same commit, which differs from the [published copy](../pinned-terms/pinned_terms.py) in eight Registry line numbers in its table of where each refusal is raised, and on the setup of the commit record's [commit_legs.py](../commit_legs.py) and the Account repository's test fixtures. |
| [order_terms_vectors.json](order_terms_vectors.json) | The pinned-terms record's test vectors plus the `delivery_signer` name fingerprint and one vector with a delivery signer. |
| [source/](source) | The seven program source files and their [notice](source/NOTICE.md). |
| [provenance.json](provenance.json) | The source commits and the source and published hash of each file. |
| [SHA256SUMS](SHA256SUMS) | Hashes of the seventeen files above. |

The traces, case index, receipt, test module, vectors and seven source files are byte-identical to their source commits: commit `4d1a2c63ac8d1fb38bf6adbfd8ee9843852e9c8c` for the record and the test files, and `8c4667f` for the program source. In the log, the absolute workspace path is replaced with `<fixture-workspace>` in three places. The source files keep their existing copyright, and this release adds no license grant for them. Each trace lists in `fixture_inputs` the hash of every test file the run used.

## Check a row yourself

Download a trace, then print any row's name, result, state comparison and program log. This reads row 57 of the main run, the order naming the seller to sign for delivery:

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
" pinned-delivery-trace.json.xz 57
```

```text
refused: evidence order 1 for size = 10 naming the seller's own key to sign for delivery; the owner pinned the carrier
expected: reject | error: {'InstructionError': [0, {'Custom': 6053}]}
state unchanged: True
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz invoke [1]
Program log: Instruction: AcceptPinnedEvidenceFinancedTab
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G invoke [2]
Program log: Instruction: ReservePinnedBilling
Program log: AnchorError thrown in programs/principal-registry/src/lib.rs:363. Error Code: TermsNotPermitted. Error Number: 6053. Error Message: Order terms do not contain a term the owner pinned.
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G failed: custom program error: 0x17a5
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz failed: custom program error: 0x17a5
```

The lines left out here are system program calls and compute-unit counts. With row 75 in place of 57, the same command prints the seller's refused report and `BillingSignature` 6063 at `evidence_billing.rs:231`. `transaction_base64` in each row is the signed transaction, and `observed_before` and `observed_after` hold each observed account's owner, lamports and data.

To check the files against their hashes, and a decompressed trace against `trace_sha256` in the receipt:

```sh
sha256sum -c SHA256SUMS
xz -dc pinned-delivery-trace.json.xz | sha256sum
```

The second command prints `9b3568aae256f51734686a4c52f9682d7b4c8ebc18672123533214c5236dbd95`.

The [pinned-terms record](../pinned-terms/README.md) covers pins on the terms of the deal, and the [named-signer record](../named-signer/README.md) covers orders that name who signs for delivery. This record is the owner pinning both on the agent's permission. The [claim guide](../../../articles/the-account-inversion/README.md) maps the article to its records.
