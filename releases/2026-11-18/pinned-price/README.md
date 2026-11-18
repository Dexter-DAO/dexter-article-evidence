# An owner pins the price, and an order at another price never commits

An owner can pin the price as well as the size and who signs for delivery. On November 18, 2026, on compiled Account programs, a buyer's owner created its agent's permission already pinned to size = 10, to the carrier as the one who signs for delivery and to a price of 1 per unit, in one instruction. The evidence order at 2 per unit was refused, and so was the same order with a list that claimed the pinned price. Orders whose tariff differed in its denominator, its maximum units, its metric or its number of metrics were refused. The order at 1 per unit was accepted. A second permission pinned the price of an ordinary order, 1 per completed-run unit of the seller's service: the order at 2 per unit was refused, and so was the order at 1 per unit of another service or another unit. None of the refused orders held or recorded anything. An accepted order sent again with any list but its own was refused, and sent again with its own list it changed nothing.

| Run | Row | What happened | Outcome | Error |
|---|---:|---|---|---|
| Main | 54 | The buyer's owner creates agent 1's permission already pinned to size = 10, the carrier and 1 per unit, in one instruction | Accepted |  |
| Main | 55 | Evidence order 1 at 2 per unit | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| Main | 56 | Evidence order 1 at 2 per unit whose list says 1 per unit | Refused | `PriceMismatch` 6055, PrincipalRegistry |
| Main | 57 | Evidence order 1 priced 1/2 per unit | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| Main | 63 | Evidence order 1 at 1 per unit | Accepted |  |
| Main | 66 | Accepted order 1 is sent again with a list that says size = 11 | Refused | `OrderTermsMismatch` 6077, ClaimsCore |
| Main | 69 | Order 2 at 2 per completed-run unit, on a permission pinned to 1 | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| Main | 71 | Order 2 at 1 per unit of another of the seller's services | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| Main | 72 | Order 2 at 1 per unit of another unit | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| Main | 74 | Order 2 at 1 per completed-run unit of the pinned service | Accepted |  |

A recoverable order pinned the same way was refused at 2 per unit and accepted at 1 per unit, and was then billed and paid: the source's former owner key could not sign the delivery report after the source rotated its key, the current owner's report of 900 was accepted, the bill was 900, and the seller's receiving custody received 900.

The programs ran on a local Solana virtual machine with a synthetic token. The record has two runs with 166 signed transactions, and all 489 recorded assertions passed. After each of the 24 refusals in the tables below, every account the transaction could write, other than the fee payer, held the same bytes as before. [case-index.json](case-index.json) names every transaction of the two runs, and the two traces hold each one's signed bytes, program log and account state.

## How a price is pinned

The terms are the list of pairs of 32-byte fingerprints described in the [pinned-terms record](../pinned-terms/README.md#how-an-order-shows-its-terms), with the reserved term `delivery_signer` of the [pinned-delivery record](../pinned-delivery/README.md#how-the-terms-and-the-delivery-signer-are-bound). Two more names are reserved. Like `delivery_signer`, their values are not fingerprints of text: each is the order's own price, computed from the order.

| Name | Name fingerprint | Value | Orders that have one |
|---|---|---|---|
| `billing_tariff` | `5d95c80f76655ec667d9ec6ed7df6726430aa936fe75b57ee3c35ecdaa48f8a0` | A hash of the billing policy's whole tariff | Evidence and recoverable orders |
| `unit_price` | `a8e0024018d0e2020fb31727f77326bfff85226b3ea709f192610f5df1b19e55` | A hash of the service, the unit and the price per unit | Orders through `accept_pinned_financed_tab` |

```text
billing_tariff value = sha256("DEXTER:BILLING-TARIFF:1" || u8(metric_count) || metric_ids[8]
                       || price_numerators[8] || price_denominators[8] || max_units[8])
                       all eight slots of each array, integers u64 little-endian, unused slots zero
unit_price value     = sha256("DEXTER:UNIT-PRICE:1" || namespace || unit || u64le(unit_price))
```

- An owner pins a price like any other term, with `create_pinned_mandate` or `pin_mandate_terms`. An order whose own price differs from the pinned value, or whose list leaves the pinned term out, fails the pin check with `TermsNotPermitted`.
- A listed `billing_tariff` or `unit_price` term must carry the order's own price, or the order fails with `PriceMismatch`. So does a listed price name the order does not have: an evidence order has a tariff and no unit price, and an order through `accept_pinned_financed_tab` has a unit price and no tariff.
- The tariff value covers the number of metrics, each metric's id, each price numerator and denominator and each maximum number of units. The unit price value covers the seller's service (the namespace), its unit and the price per unit.
- A retry of an accepted order through a pinned instruction must show the list the order committed to: the list that hashes to its external contract hash, or to its billing policy's `request_hash`. ClaimsCore checks this before it returns the original outcome, and refuses any other list with `OrderTermsMismatch`. A retry through a plain instruction shows no list and returns the original outcome.

The values this record's runs use, from the receipt:

| Text or price | Fingerprint |
|---|---|
| Tariff of 1 per unit, at most 2,000 units of one metric | `db99d6d16133ed2dc34564b93896d5e2cb748d2c2ccd03d0f21c3c8ec21b1b17` |
| Tariff of 2 per unit, at most 2,000 units of one metric | `6f0305cbf87bf27441cf7b04a5393acf356a68e552da1430b770375b697f9fe6` |
| Unit price of 1 per completed-run unit of the seller's service | `06880b8cc7ef99d9dfa5d5bbfe348fc34cbb91af5ab094949cbd35ff0f4f1d75` |
| Unit price of 2 per completed-run unit of the seller's service | `e84fa6d26568acb2a216bfe39d1e85b8c30e6b18affd9e61486a63e97c24a1f0` |

[order_terms_vectors.json](order_terms_vectors.json) adds `billing_tariff` and `unit_price` sections to the vectors of the pinned-delivery record, with an eight-metric tariff and values at `u64::MAX`. This recomputes every price vector and both name fingerprints from the file:

```sh
python3 -c "
import hashlib, json
v = json.load(open('order_terms_vectors.json'))
u64 = lambda xs: b''.join(int(x).to_bytes(8, 'little') for x in xs)
for t in v['billing_tariff']['vectors']:
    raw = bytes([t['metric_count']]) + b''.join(bytes.fromhex(m) for m in t['metric_ids'])
    raw += u64(t['price_numerators']) + u64(t['price_denominators']) + u64(t['max_units'])
    print(hashlib.sha256(v['billing_tariff']['domain'].encode() + raw).hexdigest() == t['value'], t['name'])
for p in v['unit_price']['vectors']:
    raw = bytes.fromhex(p['namespace']) + bytes.fromhex(p['unit']) + int(p['unit_price']).to_bytes(8, 'little')
    print(hashlib.sha256(v['unit_price']['domain'].encode() + raw).hexdigest() == p['value'], p['name'])
for n in ('billing_tariff', 'unit_price'):
    print(hashlib.sha256(v['name_domain'].encode() + n.encode()).hexdigest() == v[n]['name_fingerprint'], n)
"
```

It prints `True` on each of its nine lines.

## The scenario

The cast of the pinned-delivery record's main run: one buyer, two agents, one seller, the carrier, one credit issuer and one lender, with the lender's wallet starting at 1,000,000. Evidence orders have a maximum of 2,000 and one metric of at most 2,000 units, priced 1 or 2 per unit. Orders through `accept_pinned_financed_tab` and `accept_financed_tab` have a maximum of 100,000, priced 1 or 2 per completed-run unit. Amounts are base units of the synthetic token. The seller's terms for every order have four fields: item = trail shoe, size, colour = red and ship_to = warehouse 4.

| Order | Agent | Permission at the time | Sent through | Outcome |
|---:|---|---|---|---|
| 1 | 1 | Pins size = 10, the carrier and 1 per unit | `accept_pinned_evidence_financed_tab` | Accepted at 1 per unit; retries with another list refused |
| 2 | 2 | Pins size = 10 and 1 per completed-run unit | `accept_pinned_financed_tab` | Accepted at 1 per unit; retries with another list refused |
| 3 | 1 | Pins size = 10 only | `accept_pinned_evidence_financed_tab`, no `delivery_signer` term | Accepted |
| 4 | 2 | Nothing pinned | `accept_pinned_evidence_financed_tab`, no `delivery_signer` term | Accepted |
| 5 | 2 | Nothing pinned | `accept_evidence_financed_tab`, then the pinned instruction | Accepted, then the pinned retry refused |
| 6 | 2 | Nothing pinned | `accept_evidence_financed_tab`, then the pinned instruction | Accepted, then the pinned retry changes nothing |
| 7 | 2 | Nothing pinned | `accept_financed_tab`, then `accept_pinned_financed_tab` | Accepted, then a retry with another list refused and one with its own list changes nothing |

In the tables below, "Row" is the zero-based position in a trace's `transactions` array and equals `index` in the case index for that run's `suite`. "Error" is the name and number in the program log, followed by the program that raised it. An order refusal raised in the Registry reaches it through ClaimsCore, so the Registry fails first and ClaimsCore fails with the same code. A retry refusal is raised in ClaimsCore before the Registry is called.

## A pinned tariff on an evidence order

Trace: [pinned-price-trace.json.xz](pinned-price-trace.json.xz), suite `pinned-price`.

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 54 | The buyer's owner creates agent 1's permission already pinned to size = 10, delivery_signer = the carrier and billing_tariff = 1 per unit for at most 2,000 units, in one instruction | Accepted |  |
| 55 | Evidence order 1 at 2 per unit, size = 10, naming the carrier | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 56 | Evidence order 1 at 2 per unit whose list says the pinned tariff of 1 per unit | Refused | `PriceMismatch` 6055, PrincipalRegistry |
| 57 | Evidence order 1 priced 1/2 per unit (denominator 2), listing its own tariff | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 58 | Evidence order 1 for at most 1,999 units, listing its own tariff | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 59 | Evidence order 1 metering a different metric id, listing its own tariff | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 60 | Evidence order 1 with a second metric, listing its own tariff | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 61 | Evidence order 1 at 1 per unit whose list leaves out the billing_tariff term | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 62 | Evidence order 1 whose list also carries a unit_price term | Refused | `PriceMismatch` 6055, PrincipalRegistry |
| 63 | Evidence order 1, maximum 2,000, at 1 per unit, size = 10, naming the carrier | Accepted |  |

Rows 55 to 62 are order 1 with one thing changed, at the same order, reservation and bill addresses as row 63, which is order 1 unchanged. After each refusal the trace asserts that the budget hold, the lender's promise, the facility, the credit line and the lender's wallet are unchanged, and that no order record, Registry reservation, Registry operation record, receipt or bill exists at the order's addresses. For accepted order 1 the trace asserts that the bill's policy prices 1 per unit for at most 2,000 units.

## A pinned unit price on an ordinary order

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 68 | The buyer's owner creates agent 2's permission already pinned to size = 10 and unit_price = 1 per completed-run unit of the seller's service | Accepted |  |
| 69 | Agent 2 submits order 2 through `accept_pinned_financed_tab` at 2 per unit | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 70 | Order 2 at 2 per unit whose list says the pinned unit_price of 1 | Refused | `PriceMismatch` 6055, PrincipalRegistry |
| 71 | Order 2 at 1 per unit of another of the seller's services, listing its own unit_price | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 72 | Order 2 at 1 per unit of a different unit, listing its own unit_price | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 73 | Order 2 whose list also carries a billing_tariff term | Refused | `PriceMismatch` 6055, PrincipalRegistry |
| 74 | Order 2, maximum 100,000, at 1 per completed-run unit, size = 10 | Accepted |  |

Rows 69 to 73 are order 2 with one thing changed, and row 74 is order 2 unchanged. The trace makes the same assertions after each refusal as for order 1, and asserts that accepted order 2's record prices 1 per unit of the pinned unit.

## A retried order shows its own list

A retry through a pinned instruction must show the order's own list: row 66 sends accepted order 1 again with a list that says size = 11, and it is refused, where the [pinned-terms record](../pinned-terms/README.md#an-accepted-order-after-the-pin-changes), at its earlier commit, recorded the same kind of retry as no effect (its created-pinned row 70).

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 64 | Accepted order 1 is sent again with the list it was accepted with | No effect |  |
| 65 | Accepted order 1 is sent again with a list that adds note = gift | Refused | `OrderTermsMismatch` 6077, ClaimsCore |
| 66 | Accepted order 1 is sent again with a list that says size = 11 | Refused | `OrderTermsMismatch` 6077, ClaimsCore |
| 67 | Accepted order 1 is sent again through `accept_evidence_financed_tab`, which shows no list | No effect |  |
| 75 | Accepted order 2 is sent again with the list it was accepted with | No effect |  |
| 76 | Accepted order 2 is sent again with a list that adds note = gift | Refused | `OrderTermsMismatch` 6077, ClaimsCore |
| 77 | Accepted order 2 is sent again with a list that says size = 11 | Refused | `OrderTermsMismatch` 6077, ClaimsCore |
| 78 | Accepted order 2 is sent again through `accept_financed_tab`, which shows no list | No effect |  |
| 83 | Agent 2's evidence order 5 through `accept_evidence_financed_tab`; its policy's `request_hash` is not the hash of a list | Accepted |  |
| 84 | Order 5 is sent again through the pinned evidence instruction with a list | Refused | `OrderTermsMismatch` 6077, ClaimsCore |
| 85 | Agent 2's evidence order 6 through `accept_evidence_financed_tab`; its policy's `request_hash` is the hash of its list | Accepted |  |
| 86 | Order 6 is sent again through the pinned evidence instruction with that list | No effect |  |
| 87 | Agent 2's order 7, maximum 100,000, through `accept_financed_tab`; its external contract hash is the hash of its list | Accepted |  |
| 88 | Order 7 is sent again through `accept_pinned_financed_tab` with a list that says size = 11 | Refused | `OrderTermsMismatch` 6077, ClaimsCore |
| 89 | Order 7 is sent again through `accept_pinned_financed_tab` with its own list | No effect |  |

After every retry, refused or not, the trace asserts that the accepted order's record, reservation, receipt and bill are byte-identical and that the retry did not reach the Registry.

## Orders with no delivery_signer term

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 79 | The buyer's owner creates a second permission for agent 1 already pinned to size = 10 only | Accepted |  |
| 80 | Evidence order 3 through the pinned evidence instruction, with no delivery_signer term | Accepted |  |
| 82 | Agent 2's evidence order 4 through the pinned evidence instruction, with no delivery_signer term, on a permission that pins nothing | Accepted |  |

Row 81 sets up agent 2's unpinned permission. The main run ends with evidence orders 1, 3, 4, 5 and 6 of 2,000 and orders 2 and 7 of 100,000 held against the budget.

## A pinned recoverable order, paid after the source rotates its key

Trace: [pinned-price-recoverable-trace.json.xz](pinned-price-recoverable-trace.json.xz), suite `pinned-price-recoverable`. The seller is paid into its continuing receiving custody, and the delivery signer is the source's Registry principal.

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 61 | Recoverable order 1 on an unpinned permission, whose delivery_signer term is the key of the principal's owner in place of the principal | Refused | `DeliverySignerMismatch` 6054, PrincipalRegistry |
| 62 | Recoverable order 1, maximum 2,000, naming the source's principal, on the unpinned permission | Accepted |  |
| 64 | The buyer's owner pins size = 10, delivery_signer = the source's principal and billing_tariff = 1 per unit | Accepted |  |
| 65 | Recoverable order 2 at 2 per unit | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 66 | Recoverable order 2 at 2 per unit whose list says the pinned tariff of 1 per unit | Refused | `PriceMismatch` 6055, PrincipalRegistry |
| 67 | Recoverable order 2, maximum 2,000, at 1 per unit, size = 10, naming the source's principal | Accepted |  |
| 68 | Accepted recoverable order 2 is sent again with a list that says size = 11 | Refused | `OrderTermsMismatch` 6077, ClaimsCore |
| 69 | Accepted recoverable order 2 is sent again with its own list | No effect |  |
| 70 | The source principal's owner rotates to a new key | Accepted |  |
| 71 | The source principal's former owner key signs the report of 900 on order 2 | Refused | `BillingAuthorityGeneration` 6070, ClaimsCore |
| 72 | The source principal's current owner signs 900 units delivered on order 2 | Accepted |  |
| 73 | The seller signs a claim of 1,400 on order 2 | Accepted |  |
| 74 | A key with no role finalizes order 2 at 46 seconds | Accepted |  |
| 75 | A key with no role collects order 2's bill of 900 to the seller's receiving custody | Paid |  |

Row 63 sets up the second permission. The bill is the smallest of the maximum (2,000), the seller's claim (1,400) and what the source signed for (900): 900. After row 75 the seller's receiving custody held exactly 900 more, and order 2's record shows 900 paid, 900 owed by the buyer and 1,100 released.

## Who holds which key

| Role in the main run | Key |
|---|---|
| Buyer's owner | `3BuW9SR5tG6VFK4MmkQQ3Ak8ny1K1Vv5Uz7is8Aa5pwG` |
| Agent 1 | `Cdkrk8tujFY6mTyGwFgKpnbiGc1hqtXCog1qvUdKAe6D` |
| Agent 2 | `5WcE8o73vmsSZXeeWTLm3ty3fAJKCnBWRF6VuKUme5nu` |
| Seller | `4MfyR4G3NWfVRDWo6iNAHDBZqWMgwZX6FNtMqEW3a9JT` |
| Carrier | `GFKfRLPKHYRyARLSTc4p94vukKPMFAkSrQT5j55WYoy2` |
| Lender's owner | `6TcyBfPdBt1kjsvDZLzmBFnuMaLWiTaAt4RjUr9VA5YD` |
| Credit issuer's owner | `FMUEmtxhU46GzhKF4FW9MLJdQWiLgjiXP9TYRWSrqTpV` |
| Fee payer of setup and order transactions | `AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9` |

The eight keys are all different. The buyer's owner signs the pins and none of the orders.

| Role in the recoverable run | Key |
|---|---|
| Buyer's owner | `3BuW9SR5tG6VFK4MmkQQ3Ak8ny1K1Vv5Uz7is8Aa5pwG` |
| Agent | `Cdkrk8tujFY6mTyGwFgKpnbiGc1hqtXCog1qvUdKAe6D` |
| Seller's owner | `4MfyR4G3NWfVRDWo6iNAHDBZqWMgwZX6FNtMqEW3a9JT` |
| Source principal's first owner | `7nLFtY5yiR4CZnkB6bYMth3bbAns9DnFhtgjHGqcCiRn` |
| Source principal's owner after rotation | `GZXyfzSTCBmA7rpeDLrmUbiSA45H5EEwJhoBeR6ZNb3u` |

## The rule in the source

The four program files that differ from source already published are here as they stand at the commit the binaries were built from, under [source/](source). The source calls a permission a `Mandate` and the delivery signer the `source`.

The two reserved names and their values:

```rust
// programs/principal-registry/src/state.rs
198: pub const BILLING_TARIFF_NAME: [u8; 32] = [93, 149, 200, 15, 118, 101, 94, 198, 103, 217, 236, 110,
202: pub const UNIT_PRICE_NAME: [u8; 32] = [168, 224, 2, 64, 24, 208, 226, 2, 15, 179, 23, 39, 247, 115, 38,
208: pub fn billing_tariff_hash(policy: &BillingPolicy) -> [u8; 32] {
210:     encoded.push(policy.metric_count);
211:     for id in &policy.metric_ids { encoded.extend_from_slice(id); }
212:     for values in [&policy.price_numerators, &policy.price_denominators, &policy.max_units] {
215:     hashv(&[b"DEXTER:BILLING-TARIFF:1", &encoded]).to_bytes()
220: pub fn unit_price_value(namespace: &[u8; 32], unit: &[u8; 32], unit_price: u64) -> [u8; 32] {
221:     hashv(&[b"DEXTER:UNIT-PRICE:1", namespace, unit, &unit_price.to_le_bytes()]).to_bytes()
```

A listed reserved term must carry the order's own value, and an order without one cannot list it:

```rust
// programs/principal-registry/src/state.rs
251:     fn own(&self, name: &[u8; 32]) -> Option<[u8; 32]> {
253:             (Commitment::Billing { policy, .. }, DELIVERY_SIGNER_NAME) => Some(policy.source.to_bytes()),
254:             (Commitment::Billing { policy, .. }, BILLING_TARIFF_NAME) => Some(billing_tariff_hash(policy)),
255:             (Commitment::Run { namespace, unit, unit_price }, UNIT_PRICE_NAME) =>
256:                 Some(unit_price_value(namespace, unit, *unit_price)),
257:             _ => None,
262:     fn lists_own(&self, name: &[u8; 32]) -> bool {
264:         self.terms.iter().all(|term| term.name != *name || Some(term.value) == own)
273:     pub(crate) fn names_its_price(&self) -> bool {
274:         self.lists_own(&BILLING_TARIFF_NAME) && self.lists_own(&UNIT_PRICE_NAME)
```

The Registry checks the order:

```rust
// programs/principal-registry/src/lib.rs
363:             require!(mandate.pinned_terms().iter().all(|required| order.terms.contains(required)),
364:                 RegistryError::TermsNotPermitted);
366:             require!(order.names_its_delivery_signer(), RegistryError::DeliverySignerMismatch);
368:             require!(order.names_its_price(), RegistryError::PriceMismatch);
```

ClaimsCore checks a retried order's list before it returns the original outcome:

```rust
// programs/claims-core/src/funding.rs
508:     if replay(&a.receipt,&digest)?{
511:         let committed=match &order_terms {
512:             OrderTerms::Hidden=>None,
513:             OrderTerms::Run(terms)=>Some((terms,external_contract_hash)),
514:             OrderTerms::Billing{policy,terms,..}=>Some((terms,policy.request_hash)),
516:         if let Some((terms,hash))=committed {
517:             require!(principal_registry::order_terms_hash(terms)==hash,CoreError::OrderTermsMismatch);
519:         return Ok(());
```

The rest of the Registry's check, lines 356 to 362 of the same `lib.rs`, is unchanged from the [pinned-delivery record](../pinned-delivery/README.md#the-rule-in-the-source).

## Where it ran

The compiled programs ran in LiteSVM from solders 0.29.0 under Python 3.12.3, with signature verification on and native v1 transactions. Each run has one synthetic six-decimal SPL token and every key is a disposable test key. The recoverable run moves the test clock to 46 seconds after order 2's acceptance before finalization. The runs started on November 18, 2026 at 3:48 PM US Eastern. They sent no transaction to a public network.

This record ran commit `7188786b2a0ee1ea9605fa3eaae40bb672487d9c` of the Dexter Account repository, which reached its main branch in merge `cb58bfb`. Its base, main at `641246a`, has the same program source as the [pinned-delivery record](../pinned-delivery/README.md#where-it-ran). Four of the 69 build inputs differ from published files, and they are the four files under [source/](source). Three more are identical to files under [../pinned-delivery/source/](../pinned-delivery/source/NOTICE.md) and the other 62 to files under [../source/](../source/README.md). PoolIssuer's binary is the same as in every other November 18 record.

| Program | Address | SHA-256 of the binary |
|---|---|---|
| ClaimsCore | `727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz` | `4d47f5658e3fefda376e73eb21906d77e7714d2ab8409bb4288cd7823983f7fe` |
| PrincipalRegistry | `9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G` | `f6f4052ceb824561f98718176bb4d32d7851a603a949b750aecf2b9465587659` |
| PoolIssuer | `3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm` | `6804757b94ce2da8854388a1f748b614296207682db27cde85958f868b726e65` |
| Swig | `swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB` | `bb744d9811c392e3f343952d1a99bef9c97aa1936248a23b9d7efc1adb8939c1` |

Each trace records the SHA-256 of all 69 source inputs and of each binary. The source lines in the case index (`error_source`) are lines of the files under [source/](source), except `recoverable_billing.rs`, which is in [../pinned-delivery/source/](../pinned-delivery/source/NOTICE.md).

| Run | Transactions | Refusals | Assertions | SHA-256 of the JSON inside the trace |
|---|---:|---:|---:|---|
| Main | 90 | 19 | 305 | `e0b74d215d58e55acbf13e0c587abaf3245f436f84d0c951ce9ef1c831eae497` |
| Recoverable | 76 | 5 | 184 | `8e6cfa5ab64584e350590d819263ee9495671827108914872203e693ff43c1ca` |

The largest signed transactions in the two traces are 3,351 bytes, recoverable rows 65 to 69.

## Files

| File | Contents |
|---|---|
| [pinned-price-trace.json.xz](pinned-price-trace.json.xz) | The main run's 90 transactions: signed bytes, result, program log, and the observed accounts before and after. About 11 MB uncompressed. |
| [pinned-price-recoverable-trace.json.xz](pinned-price-recoverable-trace.json.xz) | The recoverable run, in the same form. |
| [case-index.json](case-index.json) | One entry per transaction of the two runs: run, name, outcome, error name and number, the program that raised it and the source line its log names. |
| [pinned-price-receipt.json](pinned-price-receipt.json) | Counts and trace hashes for each run, the role keys, the fingerprints, the price values and the binary hashes. |
| [pinned-price.log](pinned-price.log) | The three lines the runs printed. |
| [pinned_price.py](pinned_price.py) | The test module. It builds on the pinned-delivery record's [pinned_delivery.py](../pinned-delivery/pinned_delivery.py), byte-identical at this commit, on `pinned_terms.py` and the setup of the commit record's [commit_legs.py](../commit_legs.py), and on the Account repository's test fixtures. |
| [order_terms_vectors.json](order_terms_vectors.json) | The pinned-delivery record's test vectors plus the `billing_tariff` and `unit_price` sections. |
| [source/](source) | The four program source files and their [notice](source/NOTICE.md). |
| [provenance.json](provenance.json) | The source commits and the source and published hash of each file. |
| [SHA256SUMS](SHA256SUMS) | Hashes of the thirteen files above. |

The traces, case index, receipt, test module, vectors and four source files are byte-identical to their source commits: commit `59bb72f85343b2e79596e22540de22f098eba850` for the record and the test files, and `7188786` for the program source. In the log, the absolute workspace path is replaced with `<fixture-workspace>` in two places. The source files keep their existing copyright, and this release adds no license grant for them. Each trace lists in `fixture_inputs` the hash of every test file the run used.

## Check a row yourself

Download a trace, then print any row's name, result, state comparison and program log. This reads row 56 of the main run, the order at 2 per unit whose list claims the pinned price:

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
" pinned-price-trace.json.xz 56
```

```text
refused: evidence order 1 at 2 per unit whose list says the pinned tariff of 1 per unit
expected: reject | error: {'InstructionError': [0, {'Custom': 6055}]}
state unchanged: True
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz invoke [1]
Program log: Instruction: AcceptPinnedEvidenceFinancedTab
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G invoke [2]
Program log: Instruction: ReservePinnedBilling
Program log: AnchorError thrown in programs/principal-registry/src/lib.rs:368. Error Code: PriceMismatch. Error Number: 6055. Error Message: The order's billing_tariff or unit_price term is not the order's own price.
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G failed: custom program error: 0x17a7
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz failed: custom program error: 0x17a7
```

The lines left out here are system program calls and compute-unit counts. With row 66 in place of 56, the same command prints the refused retry: ClaimsCore logs `OrderTermsMismatch` 6077 at `funding.rs:517`, and the Registry is not called. `transaction_base64` in each row is the signed transaction, and `observed_before` and `observed_after` hold each observed account's owner, lamports and data.

To check the files against their hashes, and a decompressed trace against `trace_sha256` in the receipt:

```sh
sha256sum -c SHA256SUMS
xz -dc pinned-price-trace.json.xz | sha256sum
```

The second command prints `e0b74d215d58e55acbf13e0c587abaf3245f436f84d0c951ce9ef1c831eae497`.

The [pinned-terms record](../pinned-terms/README.md) covers pins on the terms of the deal and the [pinned-delivery record](../pinned-delivery/README.md) pins on who signs for delivery. This record adds the price. The [claim guide](../../../articles/the-account-inversion/README.md) maps the article to its records.
