# One standing permission serves many orders, and an order outside it never commits

An owner's permission can hold rules on the order's terms and name up to four sellers. On November 18, 2026, on compiled Account programs, a buyer's owner gave its agent one standing permission, with its rules, in one instruction: sellers 1, 2 or 3; size 10 or 10.5; black or white; 1 to 3 pairs; at most 185 per unit of one service; at most 100,000 per order. The agent then placed order after order under it, changing the size, colour, quantity, price and seller, and the commit accepted the 17 orders inside it. Orders outside it were refused at the commit and nothing was held or recorded: size 11, red, 0 pairs, 4 pairs, 186 per unit, 185 per thousandth of the service's unit, the same price for another service, and seller 4, whom the permission did not name. Each refused order, with its one difference put right, was accepted.

A rule is a list of up to four allowed values, a whole-number range checked against a number the order shows, or a price ceiling that names the services it covers. The owner then added seller 4 and allowed black only, which raised the permission's revision, and an order signed against the earlier revision was refused. The per-order limit and the budget all the owner's agents share applied as before. An order of 100,001 was refused, and so was an order of 100,000 when the budget had 81,000 left; the same order for 81,000 was accepted. The Registry refused 17 sets of rules it cannot store, among them a seller named twice, five sellers, five rules, a list of five values and a range from 2 to 1.

The four programs were built from this record's commit. PoolIssuer's and the delivery-rules program's binaries are byte for byte those of the [delivery-rules record](../delivery-rules/README.md#where-it-ran).

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 67 | The buyer's owner gives agent 1 the standing permission, rules included, in one instruction | Accepted |  |
| 68 | Agent 1 orders size 11; the permission allows size 10 or 10.5 | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 69 | Agent 1 orders size 10, black, 1 pair at 185 per unit from seller 1 | Accepted |  |
| 70 | Agent 1 orders size 10.5 in red; the permission allows black or white | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 72 | Agent 1 orders 0 pairs; the permission allows 1 to 3 | Refused | `NumberOutOfRange` 6057, PrincipalRegistry |
| 77 | Agent 1 orders 4 pairs | Refused | `NumberOutOfRange` 6057, PrincipalRegistry |
| 83 | Agent 1 orders at 186 per unit; the permission allows at most 185 | Refused | `NumberOutOfRange` 6057, PrincipalRegistry |
| 85 | Agent 1 orders at 185 per thousandth of the unit the ceiling names | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 86 | Agent 1 orders consulting, priced per second, at 185 | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 87 | Agent 1 orders at 185 per unit of the service the ceiling names | Accepted |  |
| 91 | Agent 1 orders from seller 4, whom the permission does not name | Refused | `IntentMismatch` 6010, PrincipalRegistry |
| 126 | Agent 1's order signed against revision 1, after the owner changed the rules | Refused | `MandateRevision` 6008, PrincipalRegistry |
| 117 | The buyer's owner sets rules naming five sellers | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 137 | Agent 1 orders 100,000 when the budget all agents share has 81,000 left | Refused | `BudgetExhausted` 6012, PrincipalRegistry |
| 138 | The same order for 81,000 | Accepted |  |

The programs ran in LiteSVM, a local Solana virtual machine, with a synthetic token. The record has one run with 139 signed transactions, and all 618 recorded assertions passed. After each of the 42 refusals in the tables below, every account the transaction could write, other than the fee payer, held the same bytes as before. [case-index.json](case-index.json) names every transaction, and the trace holds each one's signed bytes, program log and account state.

## A permission's sellers and rules

A permission names one to four sellers and holds up to four rules on the order's terms, one per name. The order's seller must be one of the sellers, or the order is refused with `IntentMismatch` (6010), the error a permission with one seller has always returned. A rule is a list, a range or a price ceiling:

| Rule | The order passes when |
|---|---|
| A list of 1 to 4 values | its term for the rule's name has one of the listed value fingerprints. A list of one value requires exactly that value, as a pin did. |
| A range from `min` to `max` | its term for the rule's name shows a whole number from `min` to `max` |
| A price ceiling: a range on `unit_price` that names 1 to 4 services | its own service and completed-run unit are one the ceiling names, and its own price per unit is from `min` to `max` |

An order whose list leaves out a rule's name is refused with `TermsNotPermitted` (6053), and so is a value outside a list and an order for a service or unit the ceiling does not name. A number or a price outside a range is refused with `NumberOutOfRange` (6057).

- **A range bounds the number the order shows, not what the seller bills.** Row 131's order shows 1 pair at 150 per unit with a maximum of 1,000. The seller then reports 6 units of completed work, and the order is billed 900 (row 132). What an order can spend is bounded by its maximum, and the per-order limit bounds the maximum.
- **A price ceiling reads the order's own service and price.** Each service is a seller's namespace and the completed-run unit its price is per, stored in the rule's value slots as a fingerprint. For an order through `accept_pinned_financed_tab`, the Registry computes that fingerprint from the order's own namespace and unit and requires it to be one the ceiling names, then compares the order's own price per unit with the bounds. A `unit_price` rule, list or ceiling, admits only an order outside the billing namespace at a price above zero. An evidence order is priced by a tariff of up to eight metrics and has no price per unit, so it cannot meet a `unit_price` rule (rows 98 and 99). An owner bounds evidence orders with a `billing_tariff` list.
- **`delivery_signer` and `billing_tariff` take lists only,** because their values are a key and a hash. `unit_price` takes a list, the exact price as before, or a ceiling.
- **A rule has one stored form.** The sellers must be one to four distinct keys, none of them all zero, in ascending order, and the rules at most four, with distinct names that are not all zero, in ascending order. A list holds one to four values in ascending order, with its unused slots and its bounds zero. A range holds `min <= max` and no values. A price ceiling holds `min <= max` and one to four services in ascending order. Anything else is refused with `InvalidPolicy` (6003), the error invalid pins returned.
- **Two instructions set the rules.** `create_mandate(args, rules)` creates the permission with its sellers and rules at revision 1, so it is never live without them. Sending the same creation again changes nothing, and a creation with other rules is refused with `ConfigurationConflict` (6002). `set_mandate_rules(expected_revision, rules)` replaces the sellers and rules, takes the accounts and owner check of `revoke_mandate`, and raises the revision by one, so an order prepared against the earlier revision is refused with `MandateRevision` (6008). Sending the same change again changes nothing. `create_mandate` takes the place of `create_pinned_mandate`, and `set_mandate_rules` of `pin_mandate_terms`.

## Numbers and services in an order's terms

The terms are the list of pairs of 32-byte fingerprints described in the [pinned-terms record](../pinned-terms/README.md#how-an-order-shows-its-terms). Each term now also carries the whole number its value fingerprints, or none for text. The Registry computes the fingerprint of every number an order shows and requires it to equal the term's value, or refuses the order with `NumberMismatch` (6056), whether or not a rule names the term. A ranged term shown without its number is refused with `TermsNotPermitted`. An order shows each name once, in ascending order, and a name shown twice is refused with `TermsMismatch` (6052).

```text
name fingerprint    = sha256("DEXTER:ORDER-TERM-NAME:1" || name)
value fingerprint   = sha256("DEXTER:ORDER-TERM-VALUE:1" || value)       text
number fingerprint  = sha256("DEXTER:ORDER-TERM-NUMBER:1" || i64le(n))   whole number n
order terms hash    = sha256("DEXTER:ORDER-TERMS:2" || u8(count) || name_1 || value_1 || ... || name_n || value_n)
shown term          = (name, value, number)    number: none for text, n for a whole number
service             = sha256("DEXTER:PRICE-UNIT:1" || namespace || unit)  a service a price ceiling names
```

A decimal is a whole number of its smallest step, and its name says the step: size 10.5 is `size_tenths` = 105, and a price of 185.00 is `price_cents` = 18500. The order terms hash covers names and values, and a number's value commits to the number, so the hash is computed as before. A text term costs one byte more than before, for the absent number, and a number term nine. The six-term orders of this run, two of them numbers, carry 22 bytes more than the same list would have.

A service's namespace and unit are 32-byte identifiers. In this run each is the SHA-256 of a text label. The fingerprints the run uses, from the receipt and the test vectors:

| Text, number or service | Fingerprint |
|---|---|
| name `size_tenths` | `05ccfe129bfd5db3019cca9f714fca1bd6a5d75addecc80ecc92f319d3a976e9` |
| name `colour` | `afbc890ab78fe0170cddb1cf432db91c94783f184e2562d512b77abe68ba8a28` |
| name `quantity` | `20fcc8b52d32de8a91a5ce2c4c044b545f0a526198c2f4922a79545164eb9209` |
| name `unit_price` | `a8e0024018d0e2020fb31727f77326bfff85226b3ea709f192610f5df1b19e55` |
| name `delivery_signer` | `59f0d4e2b287345eda1d13cd607e9f34d76c143108ac72494012e10fa214c370` |
| value `black` | `b585470007bd4122c5e151eff579346b96975ad8e1dc2465f50ba53aca80ff5c` |
| value `white` | `daf4c42d1a91479a7147a8ca09b5a0cd898cedbe33610986dee4dfd7cdbaf746` |
| value `red` | `20b69583a4955ce56a5f643520e2290f5905722d38959b5f8c244fd5b424b6d3` |
| number 100 (size 10) | `e356a773e8f59a97a0af03146a5e765ba1e2bf4c062903c5b77bee2866f4a374` |
| number 105 (size 10.5) | `2258d121687c5b5a5c4141d0544f9b49af7604448d8110da0af44e739f4cf68b` |
| number 110 (size 11) | `906438240d2c711e73eb1a25641755a4d5e62e7555c075574cb117ef1665c353` |
| number 1 | `187eb552570621c87358b9f727c22f401591f7f59f6e6695e3a362c677d867d1` |
| number 2 | `74d430868b83d89971c156eec24500051d35cdd36ea08538521af3f0ee1664c4` |
| service `provider-namespace`, unit `completed-inference-unit`: the inference service the ceiling names | `50520fe7013f3363d93a121a1daddb4744c6a01d0c5a13c66d475d9e614d3861` |
| service `provider-namespace`, unit `one-thousandth-of-a-completed-inference-unit` (row 85) | `963eb227859542d7663363663ffb8351893042375406f8ec377c49d985a5b69b` |
| service `consulting`, unit `second` (row 86) | `663ae63ea63e04e562eab54ce4de86624e976550d58a19f8ef9395c034c1d587` |

[order_terms_vectors.json](order_terms_vectors.json) adds `number` and `price_unit` sections to the vectors of the [pinned-price record](../pinned-price/README.md#how-a-price-is-pinned), with both ends of `i64` and the three services above. The Registry's unit tests, the SDK's test and this record's test module check them. This recomputes every number and service vector from the file:

```sh
python3 -c "
import hashlib, json
v = json.load(open('order_terms_vectors.json'))
for n in v['number']['vectors']:
    raw = int(n['number']).to_bytes(8, 'little', signed=True)
    print(hashlib.sha256(v['number']['domain'].encode() + raw).hexdigest() == n['value'], n['number'])
for p in v['price_unit']['vectors']:
    raw = bytes.fromhex(p['namespace']) + bytes.fromhex(p['unit'])
    print(hashlib.sha256(v['price_unit']['domain'].encode() + raw).hexdigest() == p['value'], p['name'])
"
```

It prints `True` on each of its thirteen lines.

## The scenario

One buyer with two agents, four registered sellers, three carriers, one credit issuer and one lender. The buyer's owner sets the budget all its agents share to 400,000. The lender's facility and the credit line allow 500,000 and the lender's wallet holds 1,000,000, so the budget is what runs out first. Orders have a maximum of 1,000, except evidence orders, which have a maximum of 2,000 at 1 per unit, and the orders of the last section. Every order through `accept_pinned_financed_tab` is for completed-run units of the inference service unless its row says otherwise, and shows six terms: item = trail shoe, `size_tenths`, `colour`, `quantity`, `unit_price` and ship_to = warehouse 4, where `size_tenths` and `quantity` are numbers. Amounts are base units of the synthetic token.

| Permission | Agent | Sellers | Rules |
|---|---|---|---|
| Standing permission, revision 1 | 1 | 1, 2, 3 | size 10 or 10.5; black or white; 1 to 3 pairs; at most 185 per unit of the inference service |
| Standing permission, revision 2 | 1 | 1, 2, 3, 4 | size 10 or 10.5; black; 1 to 3 pairs; at most 185 per unit of the inference service |
| Carriers | 2 | 1 | delivery signer carrier A or carrier B; revision 2 adds a price ceiling of 185 per unit of the inference service, and revision 3 removes it |
| Spare | 2 | 1, 2 | size 10 or 10.5; 1 to 3 pairs, then 1 to 2 pairs |

Every permission allows at most 100,000 per order. The run ends with the budget fully used: 399,100 held and 900 consumed by the bill of row 132. The lender has promised 400,000, the credit line holds 400,000 reserved or owed, and no tokens left the lender's wallet.

In the tables below, "Row" is the zero-based position in the trace's `transactions` array and equals `index` in the case index. "Error" is the name and number in the program log, followed by the program that raised it. An order refused in the Registry reaches it through ClaimsCore, so the Registry fails first and ClaimsCore fails with the same code. Each refused order is the next accepted order in its table with the change its row names, at the same addresses, and the case index records each change in `changed`.

After every refused order the trace asserts that the budget hold, the lender's promise, the facility, the credit line and the lender's wallet are unchanged, and that no order record, Registry reservation, Registry operation record or receipt exists at the order's addresses, and after a refused evidence order, that no bill exists. After every accepted order it asserts the order record's terms hash, maximum and price, the reservation's maximum and seller, the budget hold and the lender's promise, and that the buyer's owner did not sign.

## The standing permission

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 67 | The buyer's owner gives agent 1 one standing permission, in one instruction: sellers 1, 2 or 3; size 10 or 10.5; black or white; 1 to 3 pairs; at most 185 per unit of the inference service; at most 100,000 per order | Accepted |  |

The trace asserts that the permission holds its sellers and rules at revision 1, that its record is 1,070 bytes and holds the runtime's rent-exempt minimum for that size, and that creating it took one instruction, signed by the fee payer and the buyer's owner only.

## Lists: size and colour

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 68 | Agent 1 orders size 11 from seller 1; the permission allows size 10 or 10.5 | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 69 | Agent 1 orders size 10, black, 1 pair at 185 per unit from seller 1 | Accepted |  |
| 70 | Agent 1 orders size 10.5 in red; the permission allows black or white | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 71 | Agent 1 orders size 10.5, white, 1 pair at 185 per unit from seller 1 | Accepted |  |

## A range: one to three pairs

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 72 | Agent 1 orders 0 pairs; the permission allows 1 to 3 | Refused | `NumberOutOfRange` 6057, PrincipalRegistry |
| 73 | Agent 1's order carries the fingerprint of 1 pair and shows the number 2 beside it | Refused | `NumberMismatch` 6056, PrincipalRegistry |
| 74 | Agent 1's order carries the fingerprint of 1 pair and shows no number beside it | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 75 | Agent 1's order leaves quantity out of its terms | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 76 | Agent 1 orders 1 pair, the least the permission allows | Accepted |  |
| 77 | Agent 1 orders 4 pairs; the permission allows 1 to 3 | Refused | `NumberOutOfRange` 6057, PrincipalRegistry |
| 78 | Agent 1 orders 3 pairs, the most the permission allows | Accepted |  |

The order of row 73 hashes exactly as the order of row 76, which the trace asserts. Only the number shown beside the fingerprint differs.

## An order's shown terms

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 79 | Agent 1's order shows quantity twice, as the number 1 and as the text 1 | Refused | `TermsMismatch` 6052, PrincipalRegistry |
| 80 | Agent 1's order shows the number 185 beside its unit_price term | Refused | `NumberMismatch` 6056, PrincipalRegistry |
| 81 | Agent 1's order shows the number 0 beside its text term colour | Refused | `NumberMismatch` 6056, PrincipalRegistry |
| 82 | Agent 1's order shows each term once, with numbers beside size and quantity only | Accepted |  |

## The price ceiling

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 83 | Agent 1 orders at 186 per unit; the permission allows at most 185 | Refused | `NumberOutOfRange` 6057, PrincipalRegistry |
| 84 | Agent 1's order at 185 per unit leaves its unit_price term out of its terms | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 85 | Agent 1 orders at 185 per thousandth of a completed-run unit of the same service; the ceiling names the whole unit | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 86 | Agent 1 orders consulting, per second, at 185; the ceiling names the inference service only | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 87 | Agent 1 orders at 185 per unit of the inference service, the ceiling | Accepted |  |
| 88 | Agent 1 orders at 1 per unit in the billing namespace and unit through the completed-run path | Refused | `BillingProfile` 6059, ClaimsCore |
| 89 | Agent 1 orders at 1 per unit of the inference service | Accepted |  |

At 185 per thousandth of a unit, row 85's order would have charged 185,000 per unit of the service the ceiling names, and it is refused before anything is held. The trace asserts that row 87's order record prices 185 per completed-run unit of the named service. ClaimsCore refuses row 88 before it calls the Registry.

## Sellers

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 90 | Agent 1 orders size 10, black, 1 pair at 185 per unit from seller 2 | Accepted |  |
| 91 | Agent 1 orders the same shoe from seller 4, whom the permission does not name | Refused | `IntentMismatch` 6010, PrincipalRegistry |
| 92 | Agent 1 orders the shoe from seller 3 | Accepted |  |

Row 91 changes the seller's principal, payout key and signature. Seller 1's accepted orders are rows 69, 71, 76, 78, 82, 87 and 89.

## Two carriers for delivery

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 93 | The buyer's owner gives agent 2 a permission whose delivery signer is carrier A or carrier B | Accepted |  |
| 94 | Agent 2's evidence order of 2,000 names carrier A to sign for delivery | Accepted |  |
| 95 | Agent 2's evidence order names carrier C to sign for delivery | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 96 | Agent 2's evidence order of 2,000 names carrier B to sign for delivery | Accepted |  |

## A price ceiling and an evidence order

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 97 | The buyer's owner adds a price ceiling of 185 per unit of the inference service to agent 2's permission | Accepted |  |
| 98 | Agent 2's evidence order naming carrier A, under the price ceiling; it has no unit_price term | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 99 | Agent 2's evidence order naming carrier A lists a unit_price term under the price ceiling; an evidence order has no unit price | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 100 | The buyer's owner removes the price ceiling from agent 2's permission | Accepted |  |
| 101 | Agent 2's evidence order of 2,000 naming carrier A, without the price ceiling | Accepted |  |

The orders of rows 98 and 101 are the same order. It names revision 3 instead of 2 because removing the ceiling raised the revision.

## Rules the Registry refuses to store

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 102 | The buyer's owner creates a permission naming seller 1 twice | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 103 | The buyer's owner creates a spare permission for agent 2: sellers 1 and 2; size 10 or 10.5; 1 to 3 pairs | Accepted |  |
| 104 | The same creation is sent again | No effect |  |
| 105 | The creation is sent again naming seller 1 only | Refused | `ConfigurationConflict` 6002, PrincipalRegistry |
| 106 | The buyer's owner sets rules on the spare permission with its two rules' names out of ascending order | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 107 | The buyer's owner sets rules on the spare permission with size twice, as 10 or 10.5 and as 11 | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 108 | The buyer's owner sets rules on the spare permission with a size list with no values | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 109 | The buyer's owner sets rules on the spare permission with a size list that counts five values; a list holds at most four | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 110 | The buyer's owner sets rules on the spare permission with a size list of two values and a value in an unused slot | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 111 | The buyer's owner sets rules on the spare permission with the size list's two values in descending order | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 112 | The buyer's owner sets rules on the spare permission with quantity from 2 to 1 | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 113 | The buyer's owner sets rules on the spare permission with a range on delivery_signer | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 114 | The buyer's owner sets rules on the spare permission with a range on billing_tariff | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 115 | The buyer's owner sets rules on the spare permission with a price ceiling that names no service | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 116 | The buyer's owner sets rules on the spare permission with five rules; a permission holds at most four | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 117 | The buyer's owner sets rules on the spare permission with five sellers; a permission names at most four | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 118 | The buyer's owner sets rules on the spare permission with seller 1 twice | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 119 | The buyer's owner sets rules on the spare permission with the all-zero key as a seller | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 120 | The buyer's owner sets rules on the spare permission with sellers 1 and 2 out of ascending order | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 121 | The buyer's owner sets rules on the spare permission with no seller | Refused | `InvalidPolicy` 6003, PrincipalRegistry |
| 122 | The buyer's owner sets rules on the spare permission: sellers 1 and 2; size 10 or 10.5; 1 to 2 pairs | Accepted |  |

Rows 106 to 121 are the rules of row 122 with one thing changed. The trace asserts that the spare permission is unchanged after them, still at revision 1, and that row 104 changed no observed account.

## The owner changes the rules

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 123 | The buyer's owner adds seller 4 to agent 1's permission and allows black only | Accepted |  |
| 124 | The same rules are sent again against revision 1 | No effect |  |
| 125 | The original rules are sent again against revision 1; the permission is at revision 2 | Refused | `MandateRevision` 6008, PrincipalRegistry |
| 126 | Agent 1's order from seller 2, signed against revision 1 before the change | Refused | `MandateRevision` 6008, PrincipalRegistry |
| 127 | The same order from seller 2 signed against revision 2 | Accepted |  |
| 128 | Agent 1 orders white; the permission now allows black only | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 129 | Agent 1 orders black | Accepted |  |
| 130 | Agent 1 orders from seller 4, whom the owner added | Accepted |  |

Black only is a list of one value, which requires exactly that value, as a pin did. The trace asserts that row 124 changed no observed account and that the permission holds the new sellers and rules at revision 2.

## A range and the bill

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 131 | Agent 1 orders 1 pair at 150 per unit, maximum 1,000 | Accepted |  |
| 132 | Seller 1 reports 6 completed-run units at 150 on the order that shows 1 pair: 900 earned | Accepted |  |

The quantity range bounds the number the order shows, 1 pair. The bill is the seller's report of completed-run units times the order's price, bounded by the order's maximum. The trace asserts that the order is billed 900 with 100 of its 1,000 unused, and that 900 moves from held to consumed in the budget.

## One permission, order after order

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 133 | Agent 1 orders size 10, 1 pair at 150 per unit from seller 1, maximum 100,000 | Accepted |  |
| 134 | Agent 1 orders size 10.5, 2 pairs at 185 per unit from seller 2, maximum 100,000 | Accepted |  |
| 135 | Agent 1 orders size 10, 3 pairs at 120 per unit from seller 3 with a maximum of 100,001; the per-order limit is 100,000 | Refused | `PerOperationLimit` 6011, PrincipalRegistry |
| 136 | The same order with a maximum of 100,000 | Accepted |  |
| 137 | Agent 1 orders size 10.5, 1 pair at 185 per unit from seller 4 with a maximum of 100,000; the budget all agents share has 81,000 left | Refused | `BudgetExhausted` 6012, PrincipalRegistry |
| 138 | The same order with a maximum of 81,000, what the budget has left | Accepted |  |

Before row 133 the orders above use 19,000 of the budget: thirteen orders of 1,000 by agent 1 and three evidence orders of 2,000 by agent 2. Of that, 18,100 is held and 900 consumed by the bill of row 132. After row 138 all 400,000 is held or consumed.

## Size and cost

The permission record grows from 517 to 1,070 bytes. One seller becomes a count and four seller slots, 97 bytes more, and the four pinned terms become four rules of 178 bytes each: a name, a kind, a value count, four values and two bounds, 456 bytes more. A price ceiling's services use the value slots. The record's rent is paid once, when the permission is created: 8,338,080 lamports in this run's runtime, which charges (bytes + 128) × 6,960, and 6,085,840 lamports, 0.0061 SOL, at 5,080 lamports per byte plus 650,240 per account. That is 2,809,240 lamports more than the 517-byte record at the same rate.

| Transaction | Signed bytes | Compute units |
|---|---:|---:|
| Create the standing permission: three sellers, four rules (row 67) | 1,357 | 20,699 |
| Change its rules: four sellers, four rules (row 123) | 1,217 | 11,903 |
| An order showing six terms, two of them numbers (the 17 accepted orders of agent 1) | 2,709 | 369,976 to 419,475 |
| An evidence order showing five terms (rows 94, 96 and 101) | 3,159 | 421,837 to 435,332 |
| The largest transaction in the run (row 99, refused) | 3,224 | 148,395 |

The harness accepts signed transactions up to 4,096 bytes. Against main at `865bd86`, an accepted order costs 1,016 to 3,353 compute units more, because ClaimsCore and the Registry each read the larger permission record and an order that shows its list runs the rule checks. Creating, changing or revoking a permission costs 2,360 to 3,636 more.

## The other test modules

The Account repository's record ran the 30 LiteSVM test modules recorded before this change on main at `865bd86` and on this record's branch, and compared their 51 traces, with 4,822 transactions, 530 of them refused, and 16,777 assertions, transaction by transaction: name, expected result, error, signed instructions, accounts, program logs, compute units, observed account state, assertions and receipts. No transaction changed its name, expected result, pass or error, and no amount, budget hold, promise, order record or token balance changed, with one exception. On main, `create_pinned_mandate` refused a pinned creation with no terms (`InvalidPolicy`, row 61 of the pinned-terms record's `pinned-terms-created-pinned` trace). Here one instruction creates every permission, and a permission created with no term rules is the plain permission, so the same request is accepted.

Every other difference falls in a class that [compare-traces.py](compare-traces.py) checks exactly, and [trace-comparison.log](trace-comparison.log) counts each class: the permission record in its new layout, decoded with the same fields and its one seller as a list of one; the data of the instructions that create a permission or set its pins, which carry the seller as a list of one and each pin as a list rule of one value, and of the instructions that show a list, where each shown term carries no number; the passkey approvals of those instructions; log lines for the renamed instructions, two reworded messages, source lines that moved and the two keys the old seller check printed; compute units, from 126 fewer to 3,636 more; and 91 assertions. The log ends with 0 unexplained differences. Six of the 30 modules stop with the same error on both builds, and their partial traces are part of the comparison. The delivery-rules group sweep of 60 orders printed the same result on both builds.

## Who holds which key

| Role | Key |
|---|---|
| Buyer's owner | `3BuW9SR5tG6VFK4MmkQQ3Ak8ny1K1Vv5Uz7is8Aa5pwG` |
| Agent 1 | `Cdkrk8tujFY6mTyGwFgKpnbiGc1hqtXCog1qvUdKAe6D` |
| Agent 2 | `5WcE8o73vmsSZXeeWTLm3ty3fAJKCnBWRF6VuKUme5nu` |
| Seller 1 | `4MfyR4G3NWfVRDWo6iNAHDBZqWMgwZX6FNtMqEW3a9JT` |
| Seller 2 | `BELn9TCk4uWEihwdgoUx8xUfd5ujL9pKrwtJCesiQAev` |
| Seller 3 | `DXyM8yAHqpMzDtvwz5DXoEUQmPxjJPMKCASCPhLMKpEx` |
| Seller 4 | `5jotNjSnm9ZJRefVfqHato9FC4d9t6CHb2xS1726MTyY` |
| Carrier A | `GFKfRLPKHYRyARLSTc4p94vukKPMFAkSrQT5j55WYoy2` |
| Carrier B | `FLCbA5rPHLmgEeQSg1CiewVizRGw6Gy5MXc1m17HyxUG` |
| Carrier C | `E6Lu6twRJzX5iXouS9o23fBHX9upgXbApcZyRY3cLYVL` |
| Lender's owner | `6TcyBfPdBt1kjsvDZLzmBFnuMaLWiTaAt4RjUr9VA5YD` |
| Credit issuer's owner | `FMUEmtxhU46GzhKF4FW9MLJdQWiLgjiXP9TYRWSrqTpV` |
| Fee payer of setup and order transactions | `AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9` |

The trace asserts that these thirteen keys are all different. The buyer's owner signs the permissions and their changes, and the trace asserts that it signed none of the accepted orders.

## The rule in the source

The three PrincipalRegistry files that differ from source already published are here as they stand at the commit the binaries were built from, under [source/](source). The source calls a permission a `Mandate` and the seller the `supplier`.

A permission's sellers and rules have one stored form:

```rust
// programs/principal-registry/src/state.rs
183:     fn well_formed(&self) -> bool {
184:         let count = usize::from(self.value_count);
185:         let listed = (1..=MAX_RULE_VALUES).contains(&count) && self.values().windows(2).all(|pair| pair[0] < pair[1]);
186:         self.name != [0; 32] && self.values.iter().skip(count).all(|value| *value == [0; 32]) && match self.kind {
187:             RuleKind::List => listed && self.min == 0 && self.max == 0,
188:             RuleKind::Range if self.name == UNIT_PRICE_NAME => listed && self.min <= self.max,
189:             RuleKind::Range => count == 0 && self.min <= self.max
190:                 && self.name != DELIVERY_SIGNER_NAME && self.name != BILLING_TARIFF_NAME,
205:     pub fn valid(&self) -> bool {
206:         (1..=MAX_SELLERS).contains(&self.sellers.len())
207:             && self.sellers.windows(2).all(|pair| pair[0] < pair[1])
208:             && self.sellers.iter().all(|seller| *seller != Pubkey::default())
209:             && self.terms.len() <= MAX_TERM_RULES
210:             && self.terms.windows(2).all(|pair| pair[0].name < pair[1].name)
211:             && self.terms.iter().all(TermRule::well_formed)
```

The owner creates the permission with its rules, or replaces them against the current revision:

```rust
// programs/principal-registry/src/lib.rs
220:         require!(rules.valid(), RegistryError::InvalidPolicy);
222:         if mandate.active && mandate.revision == next && mandate.holds(&rules) {
226:         require!(mandate.revision == expected_revision, RegistryError::MandateRevision);
227:         mandate.set_rules(&rules);
228:         mandate.revision = next;
273:     require!(rules.valid(), RegistryError::InvalidPolicy);
282:             && mandate.holds(&rules), RegistryError::ConfigurationConflict);
```

Lines 220 to 228 are `set_mandate_rules`, and lines 273 to 282 are the creation that `create_mandate` runs. The Registry checks the order's seller and maximum, then the shown list, then every rule:

```rust
// programs/principal-registry/src/lib.rs
336:     require!(mandate.sellers().contains(&terms.supplier), RegistryError::IntentMismatch);
338:     require!(terms.cap > 0 && terms.cap <= mandate.per_operation_limit, RegistryError::PerOperationLimit);
343:         None => require!(mandate.term_rules().is_empty(), RegistryError::TermsRequired),
345:             require!((1..=MAX_ORDER_TERMS).contains(&order.terms.len()), RegistryError::TermsMismatch);
346:             require!(names_ascend(&order.terms), RegistryError::TermsMismatch);
347:             require!(order.commits_to(terms),
349:             require!(order.numbers_match(), RegistryError::NumberMismatch);
350:             for rule in mandate.term_rules() {
351:                 order.meets(rule)?;
```

A rule reads the order's term, and a price ceiling reads the order's own service and price:

```rust
// programs/principal-registry/src/state.rs
372:         self.terms.iter().all(|term| term.number.is_none_or(|n| number_value(n) == term.value))
381:     pub(crate) fn meets(&self, rule: &TermRule) -> Result<()> {
382:         let Some(shown) = self.terms.iter().find(|term| term.name == rule.name) else {
383:             return err!(RegistryError::TermsNotPermitted);
386:         if rule.name == UNIT_PRICE_NAME {
387:             let Commitment::Run { namespace, unit, unit_price } = &self.commitment else {
388:                 return err!(RegistryError::TermsNotPermitted);
390:             require!(*namespace != BILLING_NAMESPACE && *unit_price > 0, RegistryError::TermsNotPermitted);
391:             require!(rule.kind == RuleKind::List || rule.values().contains(&price_unit_value(namespace, unit)),
393:             number = Some(i128::from(*unit_price));
395:         match rule.kind {
396:             RuleKind::List => require!(rule.values().contains(&shown.value), RegistryError::TermsNotPermitted),
398:                 let Some(number) = number else { return err!(RegistryError::TermsNotPermitted) };
399:                 require!(i128::from(rule.min) <= number && number <= i128::from(rule.max), RegistryError::NumberOutOfRange);
```

Line 372 is `numbers_match`, and lines 381 to 399 are `meets`. The fingerprints are `number_value` at line 146 and `price_unit_value` at line 307 of the same file. The programs compare 32-byte values and whole numbers, and they never read a field name or a text value. In `passkey.rs`, lines 202 and 203 list `create_mandate` and `set_mandate_rules` among the Registry instructions a passkey may approve, in place of `create_pinned_mandate` and `pin_mandate_terms`. The budget check, `BudgetExhausted` at line 95 of `state.rs`, is unchanged.

## Where it ran

The compiled programs ran in LiteSVM from solders 0.29.0 under Python 3.12.3, with signature verification on and native v1 transactions. The run has one synthetic six-decimal SPL token, every key is a disposable test key, and the test never moves the clock. The run started on November 18, 2026 at 10:59 PM US Eastern. It sent no transaction to a public network.

This record ran commit `a430f7edb8fd30d352b777be8bfc09f229188dbb` of the Dexter Account repository, which reached its main branch in merge `8805fd3`. The record itself is commit `ea4b273`. Its base, main at `865bd86`, has the same program source as the [delivery-rules record](../delivery-rules/README.md#where-it-ran), and a separate checkout of that base, built the same way, reproduced its four binary hashes. Three of the 71 build inputs differ from published files, and they are the three PrincipalRegistry files under [source/](source): `lib.rs`, `state.rs` and `passkey.rs`. Seven more are identical to files under [../delivery-rules/source/](../delivery-rules/source/NOTICE.md), one to a file under [../early-settlement/source/](../early-settlement/source/NOTICE.md), one to a file under [../pinned-price/source/](../pinned-price/source/NOTICE.md) and the other 59 to files under [../source/](../source/README.md).

| Program | Address | SHA-256 of the binary |
|---|---|---|
| ClaimsCore | `727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz` | `50a7c92e22204062d929f76993cf6cb1eb5c3efc2bfb5897a72d97c46147ebe2` |
| PrincipalRegistry | `9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G` | `569d6ac0a09f16376ce7b41b7b6ae9d47bb0d7518de10520333e230053b2e865` |
| PoolIssuer | `3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm` | `6804757b94ce2da8854388a1f748b614296207682db27cde85958f868b726e65` |
| Delivery rules | `EsR1knVgJG8NAYFjeGCYpTuvjf1DtwAU1ne5XBdcotq3` | `704831da874812b856d47806083726bd4bfefe9b43fe526b3fba3c70029f9f0f` |
| Swig | `swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB` | `bb744d9811c392e3f343952d1a99bef9c97aa1936248a23b9d7efc1adb8939c1` |

ClaimsCore's source has no change. Its binary differs from the delivery-rules record's because it compiles two Registry types: it reads the permission record (`Mandate`) in its order accounts, and its order instructions take the shown list (`Term`) and pass it to the Registry. It also embeds the source line numbers of the Registry code it compiles, which moved when `state.rs` changed. ClaimsCore's test `evidence_billing::rule_program_layout`, which pins what the delivery-rules program depends on, passes.

The trace records the SHA-256 of all 71 build inputs and of the four program binaries. The source lines in the case index (`error_source`) are lines of the files under [source/](source), except `claims-core/src/funding.rs`, which is in [../pinned-price/source/](../pinned-price/source/NOTICE.md).

| Run | Transactions | Refusals | Assertions | SHA-256 of the JSON inside the trace |
|---|---:|---:|---:|---|
| Main | 139 | 42 | 618 | `159c609dc83a89387b74e7ba5cf199a6d70b8ee8f3d79c7bfe2eb7f88c66456a` |

A second run produced the same hash. The largest signed transaction in the trace is 3,224 bytes, row 99.

## Files

| File | Contents |
|---|---|
| [permission-rules-trace.json.xz](permission-rules-trace.json.xz) | All 139 transactions: signed bytes, result, program log, and the observed accounts before and after. About 32 MB uncompressed. |
| [case-index.json](case-index.json) | One entry per transaction: name, outcome, what changed, error name and number, the program that raised it and the source line its log names. |
| [permission-rules-receipt.json](permission-rules-receipt.json) | Counts, the trace hashes, the role keys, the fingerprints, the permission record's size and the binary hashes. |
| [permission-rules.log](permission-rules.log) | The two lines the run printed. |
| [permission_rules.py](permission_rules.py) | The test module. It builds on `pinned_price.py`, `pinned_delivery.py`, `pinned_terms.py` and `commit_legs.py` at this commit, which differ from the copies published with the [pinned-price](../pinned-price/pinned_price.py), [pinned-delivery](../pinned-delivery/pinned_delivery.py), [pinned-terms](../pinned-terms/pinned_terms.py) and [commit](../commit_legs.py) records, and on the Account repository's test fixtures. |
| [order_terms_vectors.json](order_terms_vectors.json) | The pinned-price record's test vectors plus the `number` and `price_unit` sections. |
| [trace-comparison.log](trace-comparison.log) | The comparison of the 51 traces of the earlier test modules on main and on this record's branch. |
| [compare-traces.py](compare-traces.py) | The script that wrote the comparison. |
| [source/](source) | The three PrincipalRegistry source files and their [notice](source/NOTICE.md). |
| [provenance.json](provenance.json) | The source commits and the source and published hash of each file. |
| [SHA256SUMS](SHA256SUMS) | Hashes of the thirteen files above. |

The trace, case index, receipt, comparison log, comparison script, test module and vectors are byte-identical to commit `ea4b27331ae7e9212289594d1b308f99c46c7016`, and the three files under `source/foundation/` to commit `a430f7e`. In the log, the absolute workspace path is replaced with `<fixture-workspace>` in one place. The source files keep their existing copyright, and this release adds no license grant for them. The trace lists in `fixture_inputs` the hash of every test file the run used.

## Check a row yourself

Download the trace, then print any row's name, result, state comparison and program log. This reads row 68, the order for size 11:

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
" permission-rules-trace.json.xz 68
```

```text
refused: agent 1 orders size 11 from seller 1; the permission allows size 10 or 10.5
expected: reject | error: {'InstructionError': [0, {'Custom': 6053}]}
state unchanged: True
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz invoke [1]
Program log: Instruction: AcceptPinnedFinancedTab
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G invoke [2]
Program log: Instruction: ReservePinned
Program log: AnchorError thrown in programs/principal-registry/src/state.rs:396. Error Code: TermsNotPermitted. Error Number: 6053. Error Message: Order terms do not meet a rule the owner set.
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G failed: custom program error: 0x17a5
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz failed: custom program error: 0x17a5
```

The lines left out here are system program calls and compute-unit counts. Line 396 is the list check quoted above. With row 85 in place of 68, the same command prints the order priced per thousandth of the unit, refused at line 391, where the ceiling's services are checked. `transaction_base64` in each row is the signed transaction, and `observed_before` and `observed_after` hold each observed account's owner, lamports and data.

To check the files against their hashes, and the decompressed trace against `trace_sha256` in the receipt:

```sh
sha256sum -c SHA256SUMS
xz -dc permission-rules-trace.json.xz | sha256sum
```

The second command prints `159c609dc83a89387b74e7ba5cf199a6d70b8ee8f3d79c7bfe2eb7f88c66456a`.

The [pinned-terms record](../pinned-terms/README.md) covers pins on the terms of the deal, the [pinned-delivery record](../pinned-delivery/README.md) pins on who signs for delivery and the [pinned-price record](../pinned-price/README.md) pins on the price. This record puts lists, ranges and a price ceiling in one permission with up to four sellers. The [claim guide](../../../articles/the-account-inversion/README.md) maps the article to its records.
