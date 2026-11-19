# A rule signs for delivery

An order can name a rule as its delivery signer, and the delivery-rules program signs the delivery report under that rule. On November 18, 2026, on compiled Account programs and the delivery-rules program, a shoe store's rule named the carrier to sign for delivery, the buyer to object and an inspector to decide. On order shoe-1 the carrier signed for 900 units of a maximum of 2,000 and nobody objected. One second after the objection window closed, a key with no role in the order settled it: the program signed the report into ClaimsCore, the bill was 900 against the seller's claim of 1,400, and the seller was paid 900 from the lender's wallet. On order shoe-2 the buyer objected, the inspector decided 600, and the bill was 600.

A rule names up to five signers and how many of them must agree, what a silent signer counts for, who may object and by when, and who decides and by when. Under a rule where two of three carriers must agree, reports of 900, 700 and 800 billed 800. Under a rule that pays the seller unless someone objects, one carrier reported the order's maximum of 2,000, a second reported 500 and the third stayed silent, and the bill was 500. A seeded sweep of 60 orders over 30 rule shapes matched an independent brute force on every order.

The delivery-rules program ran from its retained binary with no upgrade authority, and an order naming the same code with an upgrade authority was refused. ClaimsCore, PrincipalRegistry and PoolIssuer are the [early-settlement record's](../early-settlement/README.md#where-it-ran) binaries, byte for byte.

| Row | Clock | What happened | Outcome | Error |
|---:|---:|---|---|---|
| 134 | 0 s | The carrier signs 900 units delivered on order shoe-1 | Accepted |  |
| 183 | 4 d 21 s | A key with no role settles order shoe-1: the carrier signed 900 and nobody objected | Accepted |  |
| 185 | 4 d 21 s | A key with no role finalizes order shoe-1: bill 900, 1,100 released | Accepted |  |
| 186 | 4 d 21 s | A key with no role collects order shoe-1's 900 from the lender's wallet to the seller | Paid |  |
| 160 | 0 s | The buyer's owner objects on order shoe-2 | Accepted |  |
| 167 | 1 d 21 s | The inspector decides 600 units on order shoe-2, one second after signing closed | Accepted |  |
| 172 | 1 d 21 s | A key with no role finalizes order shoe-2: the rule reported 600 and the seller claimed 1,400; bill 600 | Accepted |  |
| 196 | 4 d 21 s | A key with no role finalizes order trio-1, where the carriers signed 900, 700 and 800: bill 800 | Accepted |  |
| 209 | 4 d 21 s | A key with no role finalizes order collude, where one carrier signed 2,000, a second 500 and the third nothing: bill 500 | Accepted |  |
| 68 | 0 s | Order shoe-1 names the shoe-store rule through the copy of the delivery-rules program that has an upgrade authority | Refused | `BillingSourceProgram` 6078, ClaimsCore |
| 179 | 4 d 20 s | A key with no role settles order shoe-1 in the last second of the objection window | Refused | `ObjectionsOpen` 6016, delivery-rules |
| 166 | 1 d 21 s | The seller decides 1,400 units on order shoe-2 | Refused | `NotTheDecider` 6012, delivery-rules |

The programs ran in LiteSVM, a local Solana virtual machine, with a synthetic token. The record has one run with 226 signed transactions, and all 1,471 recorded assertions passed. After each of the 44 refusals in the tables below, every account the transaction could write, other than the fee payer, held the same bytes as before. [case-index.json](case-index.json) names every transaction, and the trace holds each one's signed bytes, program log and account state.

## What a rule says

A rule is written once, when it is created. It names:

- **Who signs for delivery:** 1 to 5 distinct keys, and `threshold`, how many of them make a group, from 1 to their number.
- **What a silent signer counts for:** while fewer than `threshold` signers have reported, zero under "silence pays nothing", or the order's maximum under "paid unless someone objects".
- **Who may object, and by when:** up to 5 distinct keys, until `object_seconds` after signing closes.
- **Who decides an objection, and by when:** one key, after signing closes and until `decide_seconds` after objections close. A rule with objectors must name a decider and give it at least 3,600 seconds.
- **What a silent decider means:** zero, or the objection lapses and the signers' outcome stands.

The rule lives at `["rule", sha256("DEXTER:DELIVERY-RULE:1" || RuleTerms)]`, the address of its contents. `create_rule` writes it once, and no instruction changes or closes it, so a different signer means a new rule and a new order.

## How an order's case settles

- **The order names the rule.** The order's billing policy has `source_identity` equal to the rule's address and `source` equal to the program's address `["delivery-source", rule]`. ClaimsCore accepts the order through `accept_program_sourced_financed_tab` only while the program has no upgrade authority, as in the [early-settlement record](../early-settlement/README.md#how-an-order-names-a-program), and an owner can pin `delivery_signer` to that address. ClaimsCore does not read the rule: an order naming an address where no rule exists is accepted, its case cannot open, and its bill is 0.
- **Anyone opens the case and pays its rent.** The case lives at `["case", bill]`. `open_case` requires a ClaimsCore bill (`NotABill`) that names this rule (`RuleNotNamed`), holds no report and is not finalized (`BillClosed`). It fixes the three deadlines from the order's work deadline and requires the order's evidence deadline to fall at least 3,600 seconds after the decision deadline (`RuleOutlastsOrder`), which leaves anyone an hour to settle.
- **Each signer reports once, before signing closes,** with units the order's policy admits (`NotASigner`, `AlreadyAttested`, `SigningClosed`, `UnitsOutsidePolicy`).
- **A case takes one objection, from an objector, before objections close** (`NotAnObjector`, `ObjectionsClosed`, `AlreadyObjected`). The decider can decide only after an objection, only once, and only between the close of signing and the close of the decision (`NotTheDecider`, `NoObjection`, `SigningOpen`, `DecisionClosed`, `AlreadyDecided`).
- **Anyone settles once the outcome is fixed:** after objections close when nobody objected (`ObjectionsOpen` before), at once after a decision, or after the decision closes undecided (`DecisionOpen` before). The program then signs the report into ClaimsCore by a call from inside the program, and ClaimsCore applies the digest, deadline and replay checks it applies to every report. A settled case takes no report, objection or decision (`AlreadySettled`).
- **The highest-priced group is reported.** Each group of `threshold` signers supports, per metric, the smallest amount its members reported, and a tie in price goes to the lowest mask (signer i is bit i). Once `threshold` signers have reported, only reporting signers form groups. A decision replaces the signers' outcome, and an undecided objection reports zero or lapses into the signers' outcome.
- **Any key then finalizes the bill.** With the seller's claim already in, finalization needs no wait, and the bill is the smallest of the order's maximum, the seller's claim and the reported amount. The report's `result_hash` is the hash of the case record at settlement, and `settle` logs the same record, so the record can be checked against the bill after the case closes.
- **Anyone closes a finalized case.** A case whose bill is not finalized stays open (`BillNotFinal`), and the rent returns to whoever opened the case.

```text
rule              = ["rule", sha256("DEXTER:DELIVERY-RULE:1" || RuleTerms)]
delivery source   = ["delivery-source", rule] = policy.source;   policy.source_identity = rule
case              = ["case", bill]
signing closes    = work deadline + sign_seconds
objections close  = signing closes + object_seconds
decision closes   = objections close + decide_seconds   (at least 3,600 with objectors)
evidence deadline >= decision closes + 3,600
group             = threshold signers; per metric, the smallest amount its members support
pool              = the reporting signers once threshold have reported; otherwise all signers
silent signer     = 0, or the order's maximum under "paid unless someone objects"
signers' outcome  = the highest-priced group; equal prices: the lowest mask
result_hash       = sha256("DEXTER:DELIVERY-RULE-OUTCOME:1" || case || Borsh(case record at settlement))
bill              = min(maximum, seller claim, reported amount)
```

| Value used in the run | |
|---|---|
| Delivery-rules program, no upgrade authority | `EsR1knVgJG8NAYFjeGCYpTuvjf1DtwAU1ne5XBdcotq3` |
| The same binary with upgrade authority `EVNwqfgkUnJoMqBqiHgDfa3TLZPQocX1hpcbAXbpcSLv` | `66fEp6X3QADW85WJ5CLDFzJvMrC44xUjPdanizFxJSC` |
| Shoe-store rule | `BS3fG3p2f1V5LH11cXbwoq1sUvyNRhkGeKKSZrQ81BuN`, delivery source `2zsLHHTtci3ZySeQV5c36ts3duTLaRCMJThZu9ZM336z` |
| Shoe-store rule whose objection lapses | `3YE9nrbjd9TKYKf9XUnjEFH5irYP5De8Tn64XRWjV7Uy`, delivery source `56fYYU7XofSh32zbAT6FAUGhmG4JuFvqoHELtesvNjqd` |
| Three carriers in groups of two | `7YxTQxupZ6iKfy5uX5jxcBhSZhPBTYygHrVfoDtqxHbP`, delivery source `4ki6U7L2w72trA4EkbuaNXGbiQi6g9ERS1XN3FFCcwLE` |
| Three carriers, paid unless the buyer objects | `BzG79D1fU7NRPtaK2Hamay7nVUDwtWWPhuBT3M9KfY4H`, delivery source `9LwwCNu5ALJVuGFN5oZGs3pGQSvRT4aMdTgPggzyUAkh` |
| Two carriers in one group, paid unless the buyer objects | `5zaVMdu1AaZoVzgL9EeUDmE921LyLDhTy9NM9SJ3odg5`, delivery source `2nMTsHNezVdYhRSsPsLivhrJdeqobbbHFM26UWXDLKUJ` |
| Shoe store with an hour to decide | `9bjCGeWtyLG2LKizUiB9r8KfJ2nLnXoysTx8mUZgpVza`, delivery source `D72EYz6r4b9VrqbdpyprGX33RnLxiwDY8CKpnEabHtU2` |

A rule account is 377 bytes and holds 3,514,800 lamports of rent, paid by whoever creates it. A case is 613 bytes and holds 5,157,360 lamports, paid by whoever opens it and returned to that key when the case closes.

## The scenario

The cast of the [early-settlement record's](../early-settlement/README.md#who-holds-which-key) main run plays the same roles here, with an inspector, a second and a third carrier, and a key that settles and closes cases. Orders have a maximum of 2,000 at a fixed price of 1 per unit, except orders trio-2 and mix, which have two metrics: up to 1,500 pairs at 1 and up to 250 express deliveries at 2. Every order is accepted at the start, at 0 seconds, and its work must finish by 20 seconds. Every rule gives the signers one day after that and objectors three more days, so signing closes at 1 d 20 s and objections at 4 d 20 s. The inspector has five more days, to 9 d 20 s, except under the rule with an hour to decide, where the decision closes at 4 d 3,620 s. Reports and claims are admitted until 10 d 20 s, except that orders short, window and missed admit reports until 9 d 3,619 s, 9 d 3,620 s and 9 d 3,620 s. The seller claims 1,400 on every order at the start. Amounts are base units of the synthetic token.

| Order | Rule | What happened | Settled at | Reported | Bill |
|---|---|---|---:|---:|---:|
| shoe-1 | Shoe store | The carrier signs 900; nobody objects | 4 d 21 s | 900 | 900, collected |
| shoe-2 | Shoe store | The carrier signs 900; the buyer objects at the start; the inspector decides 600 | 1 d 21 s | 600 | 600 |
| shoe-3 | Shoe store | The carrier signs 900; the buyer objects; the inspector stays silent | 9 d 21 s | 0 | 0 |
| lapse-1 | Shoe store, objection lapses | The carrier signs 900; the buyer objects; the inspector stays silent | 9 d 21 s | 900 | 900 |
| shoe-4 | Shoe store | The carrier stays silent; nobody objects | 4 d 21 s | 0 | 0 |
| trio-1 | Three carriers, groups of two | The carriers sign 900, 700 and 800 | 4 d 21 s | 800 | 800 |
| trio-2 | Three carriers, groups of two | (900 pairs, 100 express), (700, 200), (800, 50) | 4 d 21 s | 700 pairs, 100 express | 900 |
| mix | Three carriers, groups of two | (900, 50), (1,500, 250), (800, 100) | 4 d 21 s | 900 pairs, 50 express | 1,000 |
| paid-1 | Three carriers, paid unless the buyer objects | No carrier signs | 4 d 21 s | 2,000 | 1,400, collected |
| paid-2 | Three carriers, paid unless the buyer objects | One carrier signs 500, two stay silent | 4 d 21 s | 2,000 | 1,400 |
| collude | Three carriers, paid unless the buyer objects | The carrier signs 2,000, the second 500, the third stays silent | 4 d 21 s | 500 | 500 |
| three | Three carriers, paid unless the buyer objects | The carriers sign 2,000, 500 and 1,400 | 4 d 21 s | 1,400 | 1,400 |
| duo-1 | Two carriers in one group, paid unless the buyer objects | One carrier signs 500, one stays silent | 4 d 21 s | 500 | 500 |
| hour-1 | Shoe store, an hour to decide | The carrier signs 900; the buyer objects in the last second; the inspector decides 700 in the last second of its hour | 4 d 3,620 s | 700 | 700 |
| window | Shoe store; reports until 9 d 3,620 s | The carrier signs 900; settled in the last second | 9 d 3,620 s | 900 | 900 |
| missed | Shoe store; reports until 9 d 3,620 s | The carrier signs 900; settling one second later is refused | | | 0 |
| short | Shoe store; reports until 9 d 3,619 s | Its case is refused | | | |
| ghost | An address where no rule exists | Accepted by ClaimsCore; its case is refused | | | 0 |
| keyed | The shoe-store rule's address, with the carrier's key as delivery signer | Its case is refused | | | |
| pinned | Shoe store, pinned by the buyer's owner | Accepted | | | |

Each settled bill was finalized at the clock of its settlement, with the seller's claim already in. Orders missed and ghost were finalized at 10 d 21 s, after both deadlines, with no report.

Groups on the two-metric orders, priced at 1 a pair and 2 an express delivery:

| Order | Carrier and second | Carrier and third | Second and third | Reported |
|---|---|---|---|---|
| trio-2 | (700, 100) = 900 | (800, 50) = 900 | (700, 50) = 800 | (700, 100): tied at 900, lower mask |
| mix | (900, 50) = 1,000 | (800, 50) = 900 | (800, 100) = 1,000 | (900, 50): tied at 1,000, lower mask |

In the tables below, "Row" is the zero-based position in the trace's `transactions` array and equals `index` in the case index. "Clock" is the test clock's time since the start. "Error" is the name and number in the program log, followed by the program that raised it. An order refusal raised in the Registry reaches it through ClaimsCore, so the Registry fails first and ClaimsCore fails with the same code.

## Rules are written once

| Row | Clock | What happened | Outcome | Error |
|---:|---:|---|---|---|
| 54 | 0 s | The shoe-store rule is created: the carrier signs for delivery, the buyer may object until three days after signing closes, and the inspector decides within five more; a silent carrier supports nothing and a silent inspector pays nothing | Accepted |  |
| 55 | 0 s | The same rule, except that the objection lapses if the inspector stays silent | Accepted |  |
| 56 | 0 s | A rule with three carriers in groups of two, where a silent carrier supports nothing; the buyer may object and the inspector decides | Accepted |  |
| 57 | 0 s | The three-carrier rule, except that while fewer than two carriers have reported, a silent carrier supports the order's maximum: the seller is paid unless the buyer objects | Accepted |  |
| 58 | 0 s | One lamport is sent to the address of the two-carrier rule before the rule exists | Refused | `InsufficientFundsForRent`, the runtime's rent check |
| 59 | 0 s | 890,880 lamports, the least a new account can hold, are sent to that address | Accepted |  |
| 60 | 0 s | The two-carrier rule is created at its pre-funded address: both carriers form one group, and while fewer than two have reported, a silent carrier supports the order's maximum | Accepted |  |
| 61 | 0 s | A shoe-store rule whose inspector has 3,599 seconds to decide after objections close | Refused | `InvalidRule` 6000, delivery-rules |
| 62 | 0 s | A shoe-store rule whose inspector has 3,600 seconds to decide after objections close | Accepted |  |
| 63 | 0 s | The shoe-store rule is created again | Refused | `AccountAlreadyInUse` 0, System program |
| 64 | 0 s | Terms naming the seller as decider are written to the shoe-store rule's address | Refused | `ConstraintSeeds` 2006, delivery-rules, on the account `rule` |
| 65 | 0 s | A rule naming the carrier twice among its three signers | Refused | `InvalidRule` 6000, delivery-rules |
| 66 | 0 s | A rule with the buyer as objector and no decider | Refused | `InvalidRule` 6000, delivery-rules |

In row 58 the System program's transfer ran, and the runtime refused the transaction because the address would hold less than the 890,880 lamports a new account must hold. Row 63 fails inside `create_rule`, where the System program logs that the address is already in use and returns error 0, which is `AccountAlreadyInUse` in its source.

## Orders that name a rule

| Row | Clock | What happened | Outcome | Error |
|---:|---:|---|---|---|
| 68 | 0 s | Order shoe-1 names the shoe-store rule through the copy of the delivery-rules program that has an upgrade authority | Refused | `BillingSourceProgram` 6078, ClaimsCore |
| 69 | 0 s | Order shoe-1, maximum 2,000, names the shoe-store rule | Accepted |  |
| 70 | 0 s | Order shoe-2, maximum 2,000, names the shoe-store rule | Accepted |  |
| 71 | 0 s | Order shoe-3, maximum 2,000, names the shoe-store rule | Accepted |  |
| 72 | 0 s | Order shoe-4, maximum 2,000, names the shoe-store rule | Accepted |  |
| 73 | 0 s | Order lapse-1, maximum 2,000, names the rule whose objection lapses | Accepted |  |
| 74 | 0 s | Order trio-1, maximum 2,000, names the three-carrier rule | Accepted |  |
| 75 | 0 s | Order paid-1, maximum 2,000, names the rule that pays unless the buyer objects | Accepted |  |
| 76 | 0 s | Order paid-2, maximum 2,000, names the rule that pays unless the buyer objects | Accepted |  |
| 77 | 0 s | Order collude, maximum 2,000, names the rule that pays unless the buyer objects | Accepted |  |
| 78 | 0 s | Order three, maximum 2,000, names the rule that pays unless the buyer objects | Accepted |  |
| 79 | 0 s | Order duo-1, maximum 2,000, names the two-carrier rule | Accepted |  |
| 80 | 0 s | Order hour-1, maximum 2,000, names the rule with an hour to decide | Accepted |  |
| 81 | 0 s | Order trio-2, maximum 2,000 over two metrics, names the three-carrier rule | Accepted |  |
| 82 | 0 s | Order mix, maximum 2,000 over two metrics, names the three-carrier rule | Accepted |  |
| 83 | 0 s | Order short names the shoe-store rule and admits reports until 3,599 seconds after the rule's decision deadline | Accepted |  |
| 84 | 0 s | Order window names the shoe-store rule and admits reports until 3,600 seconds after the rule's decision deadline | Accepted |  |
| 85 | 0 s | Order missed names the shoe-store rule and admits reports until 3,600 seconds after the rule's decision deadline | Accepted |  |
| 86 | 0 s | Order keyed carries the shoe-store rule's address as `source_identity` but names the carrier's key as delivery signer, through the Ed25519 evidence instruction | Accepted |  |
| 87 | 0 s | Order ghost names a rule address where no rule exists | Accepted |  |
| 88 | 0 s | The buyer's owner creates agent 2's permission pinned to `delivery_signer` = the shoe-store rule's delivery-source address | Accepted |  |
| 89 | 0 s | Agent 2's order names the rule whose objection lapses | Refused | `TermsNotPermitted` 6053, PrincipalRegistry |
| 90 | 0 s | Agent 2's order, maximum 2,000, names the pinned shoe-store rule and shows its list | Accepted |  |

Row 68 is order shoe-1 with one thing changed, and row 69 is the same order unchanged. After row 68 the trace asserts that the budget hold, the lender's promise, the facility, the credit line and the lender's wallet are unchanged, and that no order record, Registry reservation, Registry operation record, receipt or bill exists. The SDK's check before signing, `checkDeliveryRuleOrder`, answered `DELIVERY_RULE_NOT_FOUND` for order ghost and `DELIVERY_RULE_WINDOW` for order short, and the trace records both answers as assertions.

## Opening a case

| Row | Clock | What happened | Outcome | Error |
|---:|---:|---|---|---|
| 91 | 0 s | The seller opens the case of order short, whose evidence deadline is 3,599 seconds after the rule's decision deadline | Refused | `RuleOutlastsOrder` 6004, delivery-rules |
| 92 | 0 s | The seller opens the case of order window, whose evidence deadline is 3,600 seconds after the rule's decision deadline | Accepted |  |
| 93 | 0 s | The seller opens the case of order ghost; no rule exists at the address the order names | Refused | `AccountNotInitialized` 3012, delivery-rules, on the account `rule` |
| 94 | 0 s | The seller opens the case of order lapse-1 with the shoe-store rule, not the rule the order names | Refused | `RuleNotNamed` 6002, delivery-rules |
| 95 | 0 s | The seller opens the case of order keyed with the shoe-store rule; the order names the carrier's key as delivery signer | Refused | `RuleNotNamed` 6002, delivery-rules |
| 96 | 0 s | The seller opens a case for order shoe-1's order record instead of its bill | Refused | `NotABill` 6001, delivery-rules |
| 97 | 0 s | The seller opens a case for the shoe-store rule's own account in place of a bill | Refused | `NotABill` 6001, delivery-rules |
| 98 | 0 s | The seller opens the case of order shoe-1 | Accepted |  |
| 99 | 0 s | The seller opens the case of order shoe-3 | Accepted |  |
| 100 | 0 s | The seller opens the case of order shoe-4 | Accepted |  |
| 101 | 0 s | The seller opens the case of order lapse-1 | Accepted |  |
| 102 | 0 s | The seller opens the case of order trio-1 | Accepted |  |
| 103 | 0 s | The seller opens the case of order trio-2 | Accepted |  |
| 104 | 0 s | The seller opens the case of order mix | Accepted |  |
| 105 | 0 s | The seller opens the case of order paid-1 | Accepted |  |
| 106 | 0 s | The seller opens the case of order paid-2 | Accepted |  |
| 107 | 0 s | The seller opens the case of order collude | Accepted |  |
| 108 | 0 s | The seller opens the case of order three | Accepted |  |
| 109 | 0 s | The seller opens the case of order hour-1 | Accepted |  |
| 110 | 0 s | The seller opens the case of order missed | Accepted |  |
| 111 | 0 s | The buyer's owner opens the case of order shoe-2 | Accepted |  |
| 112 | 0 s | 890,880 lamports are sent to the case address of order duo-1 before the case exists | Accepted |  |
| 113 | 0 s | The seller opens the case of order duo-1 at its pre-funded address | Accepted |  |

Rows 114 to 130 are the seller's claims of 1,400, one on each of the 17 orders from shoe-1 to ghost, all accepted at 0 seconds.

## The carrier signs and nobody objects

| Row | Clock | What happened | Outcome | Error |
|---:|---:|---|---|---|
| 131 | 0 s | The carrier signs 2,001 units on order shoe-1, whose policy admits up to 2,000 | Refused | `UnitsOutsidePolicy` 6008, delivery-rules |
| 132 | 0 s | The carrier signs 900 units on order shoe-1, showing the rule whose objection lapses | Refused | `ConstraintHasOne` 2001, delivery-rules, on the account `case` |
| 133 | 0 s | The carrier signs 900 units for order shoe-1's bill on order shoe-3's case | Refused | `ConstraintHasOne` 2001, delivery-rules, on the account `case` |
| 134 | 0 s | The carrier signs 900 units delivered on order shoe-1 | Accepted |  |
| 179 | 4 d 20 s | A key with no role settles order shoe-1 in the last second of the objection window | Refused | `ObjectionsOpen` 6016, delivery-rules |
| 180 | 4 d 21 s | The buyer's owner objects on order shoe-1 one second after the objection window closed | Refused | `ObjectionsClosed` 6010, delivery-rules |
| 181 | 4 d 21 s | A key with no role settles order shoe-1's case, showing the rule whose objection lapses | Refused | `ConstraintHasOne` 2001, delivery-rules, on the account `case` |
| 182 | 4 d 21 s | A key with no role settles order shoe-1's case with order trio-1's bill | Refused | `ConstraintHasOne` 2001, delivery-rules, on the account `case` |
| 183 | 4 d 21 s | A key with no role settles order shoe-1: the carrier signed 900 and nobody objected | Accepted |  |
| 184 | 4 d 21 s | A key with no role settles order shoe-1 again | Refused | `AlreadySettled` 6018, delivery-rules |
| 185 | 4 d 21 s | A key with no role finalizes order shoe-1: bill 900, 1,100 released | Accepted |  |
| 186 | 4 d 21 s | A key with no role collects order shoe-1's 900 from the lender's wallet to the seller | Paid |  |
| 187 | 4 d 21 s | A key with no role closes the case of order shoe-1 with order trio-1's bill | Refused | `ConstraintHasOne` 2001, delivery-rules, on the account `case` |
| 188 | 4 d 21 s | A key with no role closes the case of order shoe-1, naming itself to receive the rent | Refused | `ConstraintHasOne` 2001, delivery-rules, on the account `case` |
| 189 | 4 d 21 s | A key with no role closes the case of order shoe-1; its rent returns to the seller, who opened it | Accepted |  |
| 190 | 4 d 21 s | A key with no role opens the case of order shoe-1 again after it closed | Refused | `BillClosed` 6003, delivery-rules |

For row 183 the trace asserts that the only signer is the key that settles, that the case records the signers' outcome with the carrier's 900 units, that the bill holds the rule's report and its `result_hash` is the hash of the logged case record, and that the report digest is the one ClaimsCore computes for any delivery signer. In row 186 the seller received exactly 900 and the lender's wallet paid exactly 900. After row 189 closed the case, the record in the settle log still hashes to the bill's `result_hash`.

## The buyer objects and the inspector decides

| Row | Clock | What happened | Outcome | Error |
|---:|---:|---|---|---|
| 135 | 0 s | The carrier signs 900 units delivered on order shoe-2 | Accepted |  |
| 138 | 0 s | The carrier signs 900 units delivered on order hour-1 | Accepted |  |
| 159 | 0 s | The buyer's owner objects on order shoe-2, showing the rule whose objection lapses | Refused | `ConstraintHasOne` 2001, delivery-rules, on the account `case` |
| 160 | 0 s | The buyer's owner objects on order shoe-2 | Accepted |  |
| 161 | 0 s | The inspector decides 600 units on order shoe-2 while signing is open | Refused | `SigningOpen` 6020, delivery-rules |
| 162 | 0 s | The inspector decides 600 units on order shoe-2, showing the rule whose objection lapses | Refused | `ConstraintHasOne` 2001, delivery-rules, on the account `case` |
| 163 | 0 s | The inspector decides 600 units for order shoe-2's bill on order shoe-3's case | Refused | `ConstraintHasOne` 2001, delivery-rules, on the account `case` |
| 165 | 1 d 21 s | The inspector decides 600 units on order shoe-1, where nobody objected | Refused | `NoObjection` 6013, delivery-rules |
| 166 | 1 d 21 s | The seller decides 1,400 units on order shoe-2 | Refused | `NotTheDecider` 6012, delivery-rules |
| 167 | 1 d 21 s | The inspector decides 600 units on order shoe-2, one second after signing closed | Accepted |  |
| 168 | 1 d 21 s | The inspector decides 700 units on order shoe-2, which it already decided | Refused | `AlreadyDecided` 6015, delivery-rules |
| 169 | 1 d 21 s | A key with no role settles order shoe-2 at once: the inspector decided 600 | Accepted |  |
| 170 | 1 d 21 s | The inspector decides 700 units on order shoe-2 after it settled | Refused | `AlreadySettled` 6018, delivery-rules |
| 171 | 1 d 21 s | A key with no role closes the case of order shoe-2 before its bill is finalized | Refused | `BillNotFinal` 6019, delivery-rules |
| 172 | 1 d 21 s | A key with no role finalizes order shoe-2: the rule reported 600 and the seller claimed 1,400; bill 600 | Accepted |  |
| 173 | 1 d 21 s | A key with no role closes the case of order shoe-2; its rent returns to the buyer's owner, who opened it | Accepted |  |
| 177 | 4 d 20 s | The buyer's owner objects on order hour-1 in the last second of the objection window | Accepted |  |
| 212 | 4 d 3,620 s | The inspector decides 700 units on order hour-1 in the last second of its hour, 3,600 seconds after objections closed | Accepted |  |
| 213 | 4 d 3,620 s | A key with no role settles order hour-1 at once: the inspector decided 700 | Accepted |  |
| 214 | 4 d 3,620 s | A key with no role finalizes order hour-1: bill 700 | Accepted |  |

## The buyer objects and the inspector stays silent

| Row | Clock | What happened | Outcome | Error |
|---:|---:|---|---|---|
| 136 | 0 s | The carrier signs 900 units delivered on order shoe-3 | Accepted |  |
| 137 | 0 s | The carrier signs 900 units delivered on order lapse-1 | Accepted |  |
| 174 | 4 d 20 s | The seller objects on order shoe-3 | Refused | `NotAnObjector` 6009, delivery-rules |
| 175 | 4 d 20 s | The buyer's owner objects on order shoe-3 in the last second of the objection window | Accepted |  |
| 176 | 4 d 20 s | The buyer's owner objects on order lapse-1 in the last second of the objection window | Accepted |  |
| 178 | 4 d 20 s | The buyer's owner objects on order shoe-3 a second time | Refused | `AlreadyObjected` 6011, delivery-rules |
| 215 | 9 d 20 s | A key with no role settles order shoe-3 in the last second of the decision window, with the buyer's objection undecided | Refused | `DecisionOpen` 6017, delivery-rules |
| 216 | 9 d 21 s | The inspector decides 600 units on order shoe-3 one second after the decision window closed | Refused | `DecisionClosed` 6014, delivery-rules |
| 217 | 9 d 21 s | A key with no role settles order shoe-3: the buyer objected and the inspector stayed silent, so the rule reports zero | Accepted |  |
| 218 | 9 d 21 s | A key with no role finalizes order shoe-3: bill 0, all 2,000 released | Accepted |  |
| 219 | 9 d 21 s | A key with no role settles order lapse-1: the buyer objected, the inspector stayed silent and the objection lapsed, so the rule reports the carrier's 900 | Accepted |  |
| 220 | 9 d 21 s | A key with no role finalizes order lapse-1: bill 900 | Accepted |  |

## Three carriers in groups of two

| Row | Clock | What happened | Outcome | Error |
|---:|---:|---|---|---|
| 141 | 0 s | The inspector signs 800 units on order trio-1; it is not one of the rule's carriers | Refused | `NotASigner` 6005, delivery-rules |
| 142 | 0 s | The carrier signs 900 units on order trio-1 | Accepted |  |
| 143 | 0 s | The second carrier signs 700 units on order trio-1 | Accepted |  |
| 144 | 0 s | The third carrier signs 800 units on order trio-1 | Accepted |  |
| 145 | 0 s | The carrier signs 950 units on order trio-1, where it already signed 900 | Refused | `AlreadyAttested` 6006, delivery-rules |
| 146 | 0 s | The carrier signs 900 pairs and 100 express deliveries on order trio-2 | Accepted |  |
| 147 | 0 s | The second carrier signs 700 pairs and 200 express deliveries on order trio-2 | Accepted |  |
| 148 | 0 s | The third carrier signs 800 pairs and 50 express deliveries on order trio-2 | Accepted |  |
| 149 | 0 s | The carrier signs 900 pairs and 50 express deliveries on order mix | Accepted |  |
| 150 | 0 s | The second carrier signs 1,500 pairs and 250 express deliveries on order mix | Accepted |  |
| 151 | 0 s | The third carrier signs 800 pairs and 100 express deliveries on order mix | Accepted |  |
| 195 | 4 d 21 s | A key with no role settles order trio-1: the carrier and the third carrier both support 800, the most any two support | Accepted |  |
| 196 | 4 d 21 s | A key with no role finalizes order trio-1, where the carriers signed 900, 700 and 800: bill 800 | Accepted |  |
| 197 | 4 d 21 s | A key with no role settles order trio-2: the carrier and the second carrier both support 700 pairs and 100 express deliveries (900), tied with the carrier and the third carrier; the group with the lower mask is reported | Accepted |  |
| 198 | 4 d 21 s | A key with no role finalizes order trio-2: bill 900 | Accepted |  |
| 199 | 4 d 21 s | A key with no role settles order mix: no two carriers support more than 1,000, and the second carrier's report of the maximum changes nothing | Accepted |  |
| 200 | 4 d 21 s | A key with no role finalizes order mix: bill 1,000 | Accepted |  |

## Silent carriers

| Row | Clock | What happened | Outcome | Error |
|---:|---:|---|---|---|
| 152 | 0 s | The carrier signs 500 units on order paid-2; the other two carriers stay silent | Accepted |  |
| 153 | 0 s | The carrier signs 2,000 units, the order's maximum, on order collude | Accepted |  |
| 154 | 0 s | The second carrier signs 500 units on order collude; the third carrier stays silent | Accepted |  |
| 155 | 0 s | The carrier signs 2,000 units on order three | Accepted |  |
| 156 | 0 s | The second carrier signs 500 units on order three | Accepted |  |
| 157 | 0 s | The third carrier signs 1,400 units on order three | Accepted |  |
| 158 | 0 s | The carrier signs 500 units on order duo-1; the second carrier stays silent | Accepted |  |
| 164 | 1 d 21 s | The second carrier signs 500 units on order paid-2, one second after signing closed | Refused | `SigningClosed` 6007, delivery-rules |
| 191 | 4 d 21 s | A key with no role settles order shoe-4: the carrier never signed and nobody objected; a silent carrier supports nothing, so the rule reports zero | Accepted |  |
| 192 | 4 d 21 s | The carrier signs 900 units on order shoe-4 after it settled | Refused | `AlreadySettled` 6018, delivery-rules |
| 193 | 4 d 21 s | The buyer's owner objects on order shoe-4 after it settled | Refused | `AlreadySettled` 6018, delivery-rules |
| 194 | 4 d 21 s | A key with no role finalizes order shoe-4: bill 0, all 2,000 released | Accepted |  |
| 201 | 4 d 21 s | A key with no role settles order paid-1: no carrier signed and nobody objected, and silent carriers support the maximum of 2,000 | Accepted |  |
| 202 | 4 d 21 s | A key with no role finalizes order paid-1: bill 1,400, the seller's claim | Accepted |  |
| 203 | 4 d 21 s | A key with no role collects order paid-1's 1,400 from the lender's wallet to the seller | Paid |  |
| 204 | 4 d 21 s | A key with no role settles order paid-2: only the carrier signed (500), fewer than two, so silence counts and the two silent carriers support the maximum of 2,000 | Accepted |  |
| 205 | 4 d 21 s | A key with no role finalizes order paid-2: bill 1,400 | Accepted |  |
| 206 | 4 d 21 s | A key with no role settles order duo-1: only the carrier signed (500), so silence counts, and the carrier and the silent second carrier together support 500 | Accepted |  |
| 207 | 4 d 21 s | A key with no role finalizes order duo-1: bill 500 | Accepted |  |
| 208 | 4 d 21 s | A key with no role settles order collude: two carriers reported, 2,000 and 500, so the silent third carrier does not count, and the two support 500 | Accepted |  |
| 209 | 4 d 21 s | A key with no role finalizes order collude, where one carrier signed 2,000, a second 500 and the third nothing: bill 500 | Accepted |  |
| 210 | 4 d 21 s | A key with no role settles order three: the carriers signed 2,000, 500 and 1,400, and the carrier and the third carrier both support 1,400 | Accepted |  |
| 211 | 4 d 21 s | A key with no role finalizes order three: bill 1,400 | Accepted |  |

## The hour to settle

| Row | Clock | What happened | Outcome | Error |
|---:|---:|---|---|---|
| 139 | 0 s | The carrier signs 900 units delivered on order window | Accepted |  |
| 140 | 0 s | The carrier signs 900 units delivered on order missed | Accepted |  |
| 221 | 9 d 3,620 s | A key with no role settles order window in the last second its evidence deadline admits, 3,600 seconds after the rule's decision deadline | Accepted |  |
| 222 | 9 d 3,620 s | A key with no role finalizes order window: bill 900 | Accepted |  |
| 223 | 9 d 3,621 s | A key with no role settles order missed one second after its evidence deadline | Refused | `BillingDeadline` 6065, ClaimsCore |
| 224 | 10 d 21 s | A key with no role finalizes order missed after both deadlines; nobody settled in time, and with no report the bill is 0 | Accepted |  |
| 225 | 10 d 21 s | A key with no role finalizes order ghost after both deadlines; no case could open, and with no report the bill is 0 | Accepted |  |

In row 223 ClaimsCore refuses the report the delivery-rules program signs, inside `settle`, so ClaimsCore fails first and the delivery-rules program fails with the same code.

## The group sweep

[group-sweep.log](group-sweep.log) is the summary of [delivery_rules_sweep.py](delivery_rules_sweep.py), run on the retained binary with seed 20261118. For each number of signers from 1 to 5, each group size from 1 to that number and both silence values, it settled two orders with random reports after objections closed: 60 orders over 30 rule shapes. Each case's outcome, group and units, and the bill's report, were compared with an independent brute force of the rule written in the test module. There were 0 mismatches. In 41 orders enough signers had reported and in 19 silence counted, and 5 orders had a tie broken by the lowest mask. Settlement used 34,321 to 61,037 compute units. The log's list `orders_whose_price_the_silence_change_moves` names the 11 orders, all under "paid unless someone objects", whose price would differ if a silent signer counted even after `threshold` signers had reported.

## The other test modules

The Account repository's record ran the 29 LiteSVM test modules recorded before this change on main at `5df2ff1` and on this record's branch, and compared their 50 traces, with 4,590 transactions and 15,306 assertions, transaction by transaction: name, signed bytes, accounts, instructions, error, program logs, compute units, observed account state, assertions and receipts. There is no difference. Six of the 29 modules stop with the same error on both builds, and their partial traces are part of the comparison.

## Who holds which key

| Role | Key |
|---|---|
| Buyer's owner, the objector | `3BuW9SR5tG6VFK4MmkQQ3Ak8ny1K1Vv5Uz7is8Aa5pwG` |
| Agent 1 | `Cdkrk8tujFY6mTyGwFgKpnbiGc1hqtXCog1qvUdKAe6D` |
| Agent 2 | `5WcE8o73vmsSZXeeWTLm3ty3fAJKCnBWRF6VuKUme5nu` |
| Seller, who opens cases | `4MfyR4G3NWfVRDWo6iNAHDBZqWMgwZX6FNtMqEW3a9JT` |
| Carrier | `GFKfRLPKHYRyARLSTc4p94vukKPMFAkSrQT5j55WYoy2` |
| Second carrier | `3wpYnGqceZ8DzN3guiTd9rrYkWTwTHCChBSuo6cvkXTG` |
| Third carrier | `FR5pWwinRBn35GNhg7bsvw8Q13kRept2pm561DwZCQzT` |
| Inspector, the decider | `FLCbA5rPHLmgEeQSg1CiewVizRGw6Gy5MXc1m17HyxUG` |
| Lender's owner | `6TcyBfPdBt1kjsvDZLzmBFnuMaLWiTaAt4RjUr9VA5YD` |
| Credit issuer's owner | `FMUEmtxhU46GzhKF4FW9MLJdQWiLgjiXP9TYRWSrqTpV` |
| Fee payer of setup and order transactions | `AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9` |
| Key that pays for claims, attestations, objections and decisions | `92CudvFbL7Tyw2RkWC7NU1ehdNBSjy8FEG1TsJaN8wgz` |
| Key that settles and closes cases | `4Yk9HoDSfJv9QcmJbLcXdWVgS7nfvdUqiVcvbSu8VBru` |
| Key that finalizes | `4ap9kAPdHiV82d8D73mrtEgwGxBbUxJgyFr1fRGLhide` |
| Key that collects | `6jQ7iw7szEgN6hp49SAAMagYSYdDAZTQRDF3b8vTLNqM` |

The trace asserts that these fifteen roles are held by different keys.

## The rule in the source

The delivery-rules program's source is under [source/](source/foundation/onchain/programs/delivery-rules/src/lib.rs), as it stands at the commit the binary was built from. The source calls a signer's report an attestation and the delivery signer the `source`.

A rule's terms are checked when it is created:

```rust
// programs/delivery-rules/src/lib.rs
125:         require!((1..=MAX_SIGNERS).contains(&n) && (1..=n).contains(&(self.threshold as usize))
126:             && m <= MAX_OBJECTORS && listed(&self.signers, n) && listed(&self.objectors, m)
127:             && (m == 0 || (self.decider != Pubkey::default() && self.decide_seconds >= MIN_DECIDE_SECONDS))
128:             && self.signers_silent <= PAID && self.decider_silent <= LAPSE, RuleError::InvalidRule);
```

The signers' outcome is the highest-priced group, with silence counted only while fewer than `threshold` have reported:

```rust
// programs/delivery-rules/src/lib.rs
139:         let silent = if self.signers_silent == PAID { policy.max_units } else { [0; MAX_BILLING_METRICS] };
141:         let enough = case.attested.count_ones() >= self.threshold as u32;
143:         for mask in 1..(1u8 << n) {
144:             if mask.count_ones() != self.threshold as u32 || (enough && mask & !case.attested != 0) { continue; }
146:             for i in (0..n).filter(|i| mask >> i & 1 == 1) {
147:                 let member = if case.attested >> i & 1 == 1 { &case.attestations[i] } else { &silent };
148:                 for (out, value) in units.iter_mut().zip(member) { *out = (*out).min(*value); }
150:             let price = policy.supported_amount(&units).map_err(|_| error!(RuleError::UnitsOutsidePolicy))?;
151:             if best.map_or(true, |(top, _, _)| price > top) { best = Some((price, mask, units)); }
```

A case opens only for a bill that names the rule and leaves an hour to settle:

```rust
// programs/delivery-rules/src/lib.rs
189:         let (source, _) = Pubkey::find_program_address(&[DELIVERY_SOURCE_SEED, rule.as_ref()], &crate::ID);
190:         require!(bill.policy.source_identity == rule.to_bytes() && bill.policy.source == source, RuleError::RuleNotNamed);
191:         require!(bill.source_report.is_none() && !bill.finalized, RuleError::BillClosed);
197:         require!(after(decision_closes, MIN_SETTLE_SECONDS)? <= bill.policy.evidence_deadline, RuleError::RuleOutlastsOrder);
```

Settlement waits until the outcome is fixed:

```rust
// programs/delivery-rules/src/lib.rs
257:         let (outcome, group, units) = match (a.case.objector, a.case.decision) {
258:             (Some(_), Some(decided)) => (DECIDED, 0, decided),
259:             (Some(_), None) => {
260:                 require!(now > a.case.decision_closes, RuleError::DecisionOpen);
261:                 let (group, units) = if t.decider_silent == LAPSE { t.signers_outcome(&a.case, &bill.policy)? }
262:                     else { (0, [0; MAX_BILLING_METRICS]) };
263:                 (DECIDER_SILENT, group, units)
264:             }
265:             (None, _) => {
266:                 require!(now > a.case.objections_close, RuleError::ObjectionsOpen);
```

The program then signs the report as the rule's delivery-source address:

```rust
// programs/delivery-rules/src/lib.rs
285:         let seeds: &[&[u8]] = &[DELIVERY_SOURCE_SEED, rule.as_ref(), &[ctx.bumps.delivery_source]];
286:         claims_core::cpi::submit_program_bill_evidence(CpiContext::new_with_signer(a.claims_core.key(),
```

ClaimsCore's side is the code the [early-settlement record](../early-settlement/README.md#the-rule-in-the-source) quotes, at the same lines of [evidence_billing.rs](source/foundation/onchain/programs/claims-core/src/evidence_billing.rs): the check that the program has no upgrade authority at lines 160 to 169, and the check that only the program's address signs its report at lines 319 to 324. This record's copies of `evidence_billing.rs` and [lib.rs](source/foundation/onchain/programs/claims-core/src/lib.rs) differ from the early-settlement record's only in comments on lines 29, 89 and 142, which were blank or held code before, and in the test `rule_program_layout` after line 423 of `evidence_billing.rs`. That test pins the bill layout and the `submit_program_bill_evidence` interface the delivery-rules program was built against. ClaimsCore's binary is unchanged.

## Where it ran

The compiled programs ran in LiteSVM from solders 0.29.0 under Python 3.12.3, with signature verification on and native v1 transactions. The run has one synthetic SPL token and every key is a disposable test key. The test clock moves from 0 seconds to 10 d 21 s after the orders are accepted. The run started on November 18, 2026 at 8:18 PM US Eastern and took 6.7 seconds. It sent no transaction to a public network.

This record ran commit `58a997a3047bc0743b3ff532a09a96b5fd4be3b1` of the Dexter Account repository, which reached its main branch in merge `7b2cac0`. The record itself is commit `0c268ba`. Its base, main at `5df2ff1`, has the same program source as the [early-settlement record](../early-settlement/README.md#where-it-ran), and the same checkout first built that base and reproduced its three binary hashes. Seven of the 71 build inputs differ from published files, and they are the seven build inputs under [source/](source): the delivery-rules program's `Cargo.toml` and `src/lib.rs`, ClaimsCore's `evidence_billing.rs` and `lib.rs`, and the workspace's `Anchor.toml`, `Cargo.toml` and `Cargo.lock`. One more is identical to a file under [../early-settlement/source/](../early-settlement/source/NOTICE.md), three to files under [../pinned-price/source/](../pinned-price/source/NOTICE.md), one to a file under [../pinned-delivery/source/](../pinned-delivery/source/NOTICE.md) and the other 59 to files under [../source/](../source/README.md).

| Program | Address | SHA-256 of the binary |
|---|---|---|
| ClaimsCore | `727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz` | `a96472c1e26f35fcabe9a5af9122c2d807c1048ba8a7a511dc0b2ea5473b69f2` |
| PrincipalRegistry | `9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G` | `f6f4052ceb824561f98718176bb4d32d7851a603a949b750aecf2b9465587659` |
| PoolIssuer | `3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm` | `6804757b94ce2da8854388a1f748b614296207682db27cde85958f868b726e65` |
| Delivery rules | `EsR1knVgJG8NAYFjeGCYpTuvjf1DtwAU1ne5XBdcotq3` and `66fEp6X3QADW85WJ5CLDFzJvMrC44xUjPdanizFxJSC` | `704831da874812b856d47806083726bd4bfefe9b43fe526b3fba3c70029f9f0f` |
| Swig | `swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB` | `bb744d9811c392e3f343952d1a99bef9c97aa1936248a23b9d7efc1adb8939c1` |

The delivery-rules program ran from its retained binary, [delivery-rules-retained.so](source/foundation/onchain/programs/delivery-rules/artifacts/delivery-rules-retained.so), 262,976 bytes, which the test module loads by its SHA-256. The trace asserts that this commit's build of the program is the same file. The binary was loaded twice: at its declared address with no upgrade authority, and at `66fEp6X3QADW85WJ5CLDFzJvMrC44xUjPdanizFxJSC` with upgrade authority `EVNwqfgkUnJoMqBqiHgDfa3TLZPQocX1hpcbAXbpcSLv`, and the trace asserts the loader records of both copies. Every result of the delivery-rules program in this record is a result of that file.

The trace records the SHA-256 of all 71 build inputs and of the four program binaries. The source lines in the case index (`error_source`) are lines of the files under [source/](source), except `principal-registry/src/lib.rs`, which is in [../pinned-price/source/](../pinned-price/source/NOTICE.md).

| Run | Transactions | Refusals | Assertions | SHA-256 of the JSON inside the trace |
|---|---:|---:|---:|---|
| Main | 226 | 44 | 1,471 | `4fca1d48cfd1a5527c82355fc5fd504c935685eeed7f2b4e3d823c7a0d8d4dce` |

The largest signed transactions in the trace are 3,221 bytes, rows 89 and 90.

## Files

| File | Contents |
|---|---|
| [delivery-rules-trace.json.xz](delivery-rules-trace.json.xz) | All 226 transactions: signed bytes, result, program log, and the observed accounts before and after. About 117 MB uncompressed. |
| [case-index.json](case-index.json) | One entry per transaction: name, outcome, error name and number, the program that raised it, the source line its log names and the test clock. |
| [delivery-rules-receipt.json](delivery-rules-receipt.json) | Counts, the trace hashes, the role keys, the binary hashes, and the delivery-rules program's addresses, rules, SDK check answers and account sizes. |
| [delivery-rules.log](delivery-rules.log) | The two lines the run printed. |
| [group-sweep.log](group-sweep.log) | The group sweep's summary. |
| [delivery_rules.py](delivery_rules.py) | The test module. It builds on the early-settlement record's [early_settlement.py](../early-settlement/early_settlement.py), which is byte-identical at this commit, and on the pinned-delivery record's `pinned_delivery.py`, which at this commit differs from the [published copy](../pinned-delivery/pinned_delivery.py) in one line, the source line it expects for a refused report signature. It also uses `pinned_terms.py`, the setup of the commit record's [commit_legs.py](../commit_legs.py), which is byte-identical at this commit, and the Account repository's `evidence_billing.py` and `recovery.py` fixtures. It runs the SDK check through the Account repository's `delivery_rules_sdk_check.mjs`. |
| [delivery_rules_sweep.py](delivery_rules_sweep.py) | The group sweep and its brute force, on the fixture of `delivery_rules.py`. |
| [source/](source) | The delivery-rules program's source, README and retained binary, the five other source files that differ from those already published, and their [notice](source/NOTICE.md). |
| [provenance.json](provenance.json) | The source commits and the source and published hash of each file. |
| [SHA256SUMS](SHA256SUMS) | Hashes of the eighteen files above. |

The trace, case index, receipt, group-sweep log and both test modules are byte-identical to commit `0c268babd626d18c8ada696f48942b8c9a35d3c6`, and the nine files under `source/foundation/` to commit `58a997a`, with two exceptions. In the log, the absolute workspace path is replaced with `<fixture-workspace>` in one place. The delivery-rules program's README is the copy on the Account repository's main branch, from commit `7c5f123`, and its link to the record points to this guide. The source files keep their existing copyright, and this release adds no license grant for them. The trace lists in `fixture_inputs` the hash of every test file the run used.

## Check a row yourself

Download the trace, then print any row's name, result, state comparison and program log. This reads row 179, the settlement refused in the last second of the objection window:

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
" delivery-rules-trace.json.xz 179
```

```text
refused: an outside key settles order shoe-1 in the last second of the objection window
expected: reject | error: {'InstructionError': [0, {'Custom': 6016}]}
state unchanged: True
Program EsR1knVgJG8NAYFjeGCYpTuvjf1DtwAU1ne5XBdcotq3 invoke [1]
Program log: Instruction: Settle
Program log: AnchorError thrown in programs/delivery-rules/src/lib.rs:266. Error Code: ObjectionsOpen. Error Number: 6016. Error Message: The objection window is still open.
Program EsR1knVgJG8NAYFjeGCYpTuvjf1DtwAU1ne5XBdcotq3 failed: custom program error: 0x1780
```

The line left out here is the compute-unit count. The trace calls a key with no role in the order an outside key. Line 266 is the objection-window check quoted above. With row 183 in place of 179, the same command prints the settlement of order shoe-1: the `Program data:` line with the logged case record, and the call into ClaimsCore's `SubmitProgramBillEvidence`. `transaction_base64` in each row is the signed transaction, and `observed_before` and `observed_after` hold each observed account's owner, lamports and data.

To check the files against their hashes, and the decompressed trace against `trace_sha256` in the receipt:

```sh
sha256sum -c SHA256SUMS
xz -dc delivery-rules-trace.json.xz | sha256sum
```

The second command prints `4fca1d48cfd1a5527c82355fc5fd504c935685eeed7f2b4e3d823c7a0d8d4dce`.

The [named-signer record](../named-signer/README.md) covers orders that name a key as the delivery signer, and the [early-settlement record](../early-settlement/README.md) orders that name a program and settle as soon as both signed records are in. This record adds a program that signs under a rule with several signers, objections and a decider. The [claim guide](../../../articles/the-account-inversion/README.md) maps the article to its records.
