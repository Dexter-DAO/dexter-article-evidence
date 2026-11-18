# One account buys on one order and sells on another

One company bought on one order and sold on another, and its single book showed both positions at the same time. On November 18, 2026, on the compiled Account programs, the company ordered 100,000 from a supplier on order A, backed by lender 1, and took an order of 150,000 from a customer on order B, backed by lender 2. Its book for that token showed 100,000 on the buying side and 150,000 on the selling side at once. It was paid 150,000 straight from lender 2's wallet, and the supplier was paid 100,000 straight from lender 1's wallet. Neither transfer touched the buyer's own token account. Between those steps the company's owner replaced its agent and then its owner key, and both orders were byte-identical across each change.

The one place that holds both positions is the book, a ClaimsCore `FinanceBook` account, one per principal per token. The two order records are separate accounts, and the company's debt of 100,000 on order A is recorded as owed to the credit issuer.

The programs ran on a local Solana virtual machine with a synthetic token. The run has 74 signed transactions, all accepted, and all 233 recorded assertions passed. The scenario expects no refusal and the programs refused nothing. [case-index.json](case-index.json) names every transaction, and [both-sides-trace.json.xz](both-sides-trace.json.xz) holds each one's signed bytes, program log and account state.

## The scenario

```text
customer  --order B, maximum 150,000-->  middle company  --order A, maximum 100,000-->  supplier
              backed by lender 2                              backed by lender 1
```

Six principals: the customer, the middle company, the supplier, lender 1, lender 2 and one credit issuer that stands behind both orders. Each lender has its own wallet holding 1,000,000. Each buyer has its own budget of 500,000, its own agent and its own credit line. Orders carry no interest and no seller fee. Amounts are base units of the synthetic token.

The shared test fixture names the parties differently in its setup rows: `company` is the middle company, `assignee` the customer, `sponsor` lender 2 and `financier` lender 1.

In the tables below, "Row" is the zero-based position in the trace's `transactions` array and equals `index` in the case index. Rows 0 to 63 are setup.

## In order

| Row | What happened | Outcome |
|---:|---|---|
| 64 | Order A: the middle company's agent orders from the supplier, maximum 100,000, backed by lender 1 | Accepted |
| 65 | Order B: the customer's agent orders from the middle company, maximum 150,000, backed by lender 2 | Accepted |
| 66 | The middle company's owner revokes its agent | Accepted |
| 67 | The middle company's owner enrolls a new agent with a new key | Accepted |
| 68 | The middle company's owner permits the new agent to order from the supplier on lender 1's facility | Accepted |
| 69 | The middle company reports 150,000 of completed work on order B | Accepted |
| 70 | A key with no role collects order B's 150,000 from lender 2's wallet to the middle company | Paid |
| 71 | The middle company's owner hands control to a new owner key | Accepted |
| 72 | The supplier reports 100,000 of completed work on order A | Accepted |
| 73 | A key with no role collects order A's 100,000 from lender 1's wallet to the supplier | Paid |

## The middle company's book

The book is at the address derived from `"financial-book"`, the company's principal and the token. In this run it is `2E5fSxAazJMYG8xpKPtzrsCfWAv9Y8UXyxwZDhjFTgyp` for principal `3j5rAy9V4FQTbposk4Uow2eb1xHhgC9GyNZmpFn6Dmr8`. Its `positions` have separate fields for the buying side and the selling side. The table lists every non-zero field at each point, and all other fields are zero.

| When | Buying side (order A) | Selling side (order B) |
|---|---|---|
| Before any order | Nothing | Nothing |
| After order A is accepted (row 64) | `customer_reserved` 100,000 | Nothing |
| After order B is accepted (row 65) | `customer_reserved` 100,000 | `supplier_committed` 150,000 |
| After the change of agent (rows 66 to 68) | `customer_reserved` 100,000 | `supplier_committed` 150,000 |
| After its work on order B (row 69) | `customer_reserved` 100,000 | `supplier_receivable` 150,000 |
| After it is paid on order B (row 70) | `customer_reserved` 100,000 | `cash_received` 150,000 |
| After the change of owner (row 71) | `customer_reserved` 100,000 | `cash_received` 150,000 |
| After the supplier's work on order A (row 72) | `customer_principal` 100,000 | `cash_received` 150,000 |
| After both collections (row 73) | `customer_principal` 100,000 | `cash_received` 150,000 |

Other accounts record parts of this, and they are separate accounts:

- The two order records are two accounts. Order A's is stored under the middle company as buyer. Order B's is stored under the customer as buyer and names the middle company as seller.
- The middle company's budget in the Registry holds only its buying side: 100,000 held after order A, then 100,000 consumed.
- A PoolIssuer projection mirrors the book. The trace asserts it equals the book at every point in the table.

## Where the money went

- In row 70, order B's collection contains exactly one token transfer: 150,000 from lender 2's wallet token account to the middle company's payout token account. The customer's own token account and the customer's owner key are not among the transaction's accounts, and the customer's balance did not change. Lender 1's wallet did not change.
- In row 73, order A's collection contains exactly one token transfer: 100,000 from lender 1's wallet token account to the supplier's payout token account. The middle company's token account, both of its owner keys and both of its agent keys are not among the transaction's accounts, and its balance did not change. Lender 2's wallet did not change.

Each collection is signed by one key with no role and by nobody else. The work records name the credit issuer as the owner of both debts: the middle company's 100,000 on order A and the customer's 150,000 on order B.

## The changes of agent and owner

- Before either collection, in rows 66 to 68, the owner revoked the agent that placed order A and enrolled a new one. The records of orders A and B, their reservations and their acceptance receipts are byte-identical across the change, and both orders went on to be collected.
- Between the two collections, in row 71, the owner handed control to a new key. The same six accounts are byte-identical across the change, and the book's address and contents did not change. Order A was then worked and collected.

Order B's payout account is the token account of the key that owned the middle company when the order was accepted, fixed in the order's terms. The 150,000 stayed there after the owner key changed.

## Who holds which key

| Role | Key |
|---|---|
| Customer's owner | `7nLFtY5yiR4CZnkB6bYMth3bbAns9DnFhtgjHGqcCiRn` |
| Customer's agent | `3F5qRPtKg8GhGNnbd3qCj6nVJxWsGxq7pvH84okYLAqf` |
| Middle company's first owner | `3BuW9SR5tG6VFK4MmkQQ3Ak8ny1K1Vv5Uz7is8Aa5pwG` |
| Middle company's first agent | `Cdkrk8tujFY6mTyGwFgKpnbiGc1hqtXCog1qvUdKAe6D` |
| Middle company's second agent | `5F8wMRFMdYGMkjWQUye6WfbgRVWEo9yyKo9aFPk2TLaD` |
| Middle company's second owner | `7xwJ3uceuBV7KyCsdJsBs9Ljfh1bL3WB7NbGpwUNeJ2o` |
| Supplier's owner | `4MfyR4G3NWfVRDWo6iNAHDBZqWMgwZX6FNtMqEW3a9JT` |
| Lender 1's owner | `6TcyBfPdBt1kjsvDZLzmBFnuMaLWiTaAt4RjUr9VA5YD` |
| Lender 2's owner | `32kSi9P4AoNDrV7B2GVa6iGaXADP539vT5tpixnfrRej` |
| Credit issuer's owner | `FMUEmtxhU46GzhKF4FW9MLJdQWiLgjiXP9TYRWSrqTpV` |
| Fee payer of setup and order transactions | `AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9` |
| Key that collects | `6jQ7iw7szEgN6hp49SAAMagYSYdDAZTQRDF3b8vTLNqM` |

The trace asserts that these twelve keys are all different and that the six principals are all different.

## The rule in the source

The book's fields are `FinancialPositions`, line 25 of [funded.rs](../source/foundation/onchain/crates/contracts/src/funded.rs), and its address is `book_address`, line 146 of the same file. Collection is `collect`, line 15 of [funding_collection.rs](../source/foundation/onchain/programs/claims-core/src/funding_collection.rs). The change of owner key is `rotate`, line 53 of [ownership.rs](../source/foundation/onchain/programs/principal-registry/src/ownership.rs), and the agent's revocation is `revoke_actor`, line 209 of [lib.rs](../source/foundation/onchain/programs/principal-registry/src/lib.rs).

## Where it ran

The compiled programs ran in LiteSVM from solders 0.29.0 under Python 3.12.3, with signature verification on and native v1 transactions. The token is one synthetic six-decimal SPL token and every key is a disposable test key. The run started on November 18, 2026 at 10:42 AM US Eastern. It sent no transaction to a public network.

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
| [both-sides-trace.json.xz](both-sides-trace.json.xz) | All 74 transactions: signed bytes, result, program log, and the observed accounts before and after. About 6 MB uncompressed. |
| [case-index.json](case-index.json) | One entry per transaction: name, outcome, error name and number, the program that raised it and the source line its log names. |
| [both-sides-receipt.json](both-sides-receipt.json) | Counts, the trace hashes, the twelve role keys, the book and wallet addresses and the binary hashes. |
| [both-sides.log](both-sides.log) | The 2 lines the run printed. |
| [both_sides.py](both_sides.py) | The test module. It reuses the helpers in the commit record's [commit_legs.py](../commit_legs.py). |
| [provenance.json](provenance.json) | The source commit and the source and published hash of each file. |
| [SHA256SUMS](SHA256SUMS) | Hashes of the six files above. |

The trace, case index, receipt and test module are byte-identical to the source commit. In the log, one absolute workspace path is replaced with `<fixture-workspace>`. The trace's `fixture_inputs` lists the hash of every test file the run used.

## Check a row yourself

Download the trace, then print any row's name, result, state comparison and program log. This reads row 70, the collection that pays the middle company on order B:

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
" both-sides-trace.json.xz 70
```

```text
ACCEPTED: an outside key collects order B's 150,000 from lender 2's wallet to the middle company
expected: success | error: None
state unchanged: False
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G invoke [1]
Program log: Instruction: AuthorizeFundingWallet
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G success
Program swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB invoke [1]
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz invoke [2]
Program log: Instruction: CollectFinancier
Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [3]
Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success
Program 3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm invoke [3]
Program log: Instruction: ProjectFinancingBatch
Program 3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm success
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz success
Program swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB success
```

The lines left out here are system program calls and compute-unit counts. `transaction_base64` in the same row is the signed transaction, and `observed_before` and `observed_after` hold each observed account's owner, lamports and data.

To check the files against their hashes, and the decompressed trace against `trace_sha256` in the receipt:

```sh
sha256sum -c SHA256SUMS
xz -dc both-sides-trace.json.xz | sha256sum
```

The second command prints `c78d23e46b6147e4bf76e88116453992cea01cc3172b1e76d2e4e5926dd0a4cb`.

The [commit record](../README.md) holds the refusals of the same instructions. The [claim guide](../../../articles/the-account-inversion/README.md) maps the article to its records.
