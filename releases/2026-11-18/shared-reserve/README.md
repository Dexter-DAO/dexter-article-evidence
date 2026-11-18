# Two agents and two sellers share one budget and one lender wallet

One buyer budget backed three open orders at once, and one lender wallet backed two. On November 18, 2026, on the compiled Account programs, two agents of one buyer ordered from two sellers. An order that would overdraw the shared budget or the shared wallet was refused, and the same order was accepted at the amount that fit. Each seller was then paid from the shared wallet by a key with no role in either order, and the other seller's share stayed untouched. The lender could withdraw unused backing only after the seller released it.

The programs ran on a local Solana virtual machine with a synthetic token, and the transactions ran one after another. The record has 136 signed transactions in two runs, and all 299 recorded assertions passed. After each of the 10 refusals, every account the transaction could write, other than the fee payer, held the same bytes as before. [case-index.json](case-index.json) names every transaction, and the two traces hold each one's signed bytes, program log and account state.

## The scenario

One buyer with two enrolled agents, two sellers, one credit issuer and one lender with one wallet. Agent A may order from seller 1 and agent B from seller 2, each up to 100,000 per order. Both agents' permissions name the same budget account. Amounts are base units of the synthetic token, and orders carry no interest and no seller fee.

The commit checks the lender's cash before the buyer's budget. With both at 150,000, a second order would always be refused for cash and the budget refusal would never appear. So the same cast runs twice, with one of the two tight each time, and each refusal has one cause.

| | Budget run | Wallet run |
|---|---:|---:|
| Buyer's budget | 150,000 | 500,000 |
| Lender wallet | 1,000,000 | 150,000 |
| Each agent's per-order limit | 100,000 | 100,000 |
| Transactions | 63 | 73 |
| `suite` in the case index | `shared-reserve-budget` | `shared-reserve-wallet` |

Before each refused order, the trace records that the facility limit, the credit line, and whichever of the budget and the lender's free cash is not the tight one still cover it.

Seller 2 is the party the shared test fixture names `assignee`, so its setup rows in the case index carry that name. It starts with 200,000 of test funds in its own token account, and the trace's balance checks subtract that.

In the tables below, "Row" is the zero-based position in that run's `transactions` array and equals `index` in the case index. "Error" is the name and number in the program log, followed by the program that raised it.

## Budget run: two agents, one budget of 150,000

Trace: [shared-reserve-budget-trace.json.xz](shared-reserve-budget-trace.json.xz).

| Row | Attempt | Outcome | Error |
|---:|---|---|---|
| 57 | Agent A orders 100,000 from seller 1. Nothing is held against the budget of 150,000. | Accepted | |
| 58 | Agent B orders 100,000 from seller 2. The budget has 50,000 left after agent A's order. | Refused | `BudgetExhausted` 6012, PrincipalRegistry |
| 59 | Agent B orders 50,000 from seller 2, exactly what the budget has left | Accepted | |
| 60 | Agent A orders 1 unit from seller 1. The two agents' orders hold the whole 150,000. | Refused | `BudgetExhausted` 6012, PrincipalRegistry |
| 62 | The same 1-unit order after the owner raised the budget to 150,001 | Accepted | |

The trace records the amount held against the budget after each row: 100,000 after row 57 and after the refusal in row 58, 150,000 after row 59 and after the refusal in row 60, and 150,001 after row 62. Row 61 is the owner raising the budget by 1 unit. Agent B's 100,000 in row 58 is inside agent B's own per-order limit, so the shared budget is what refuses it.

## Wallet run: two sellers, one lender wallet of 150,000

Trace: [shared-reserve-wallet-trace.json.xz](shared-reserve-wallet-trace.json.xz).

### Orders

| Row | Attempt | Outcome | Error |
|---:|---|---|---|
| 56 | Agent A orders 100,000 from seller 1. The wallet holds 150,000 with nothing promised. | Accepted | |
| 57 | Agent B orders 100,000 from seller 2. 50,000 of the wallet is free after seller 1's order. | Refused | `FundingCashShortfall` 6033, ClaimsCore |
| 58 | Agent B orders 50,000 from seller 2, exactly the wallet's free cash | Accepted | |

One token account held all 150,000 before row 56 and after rows 56, 57 and 58, so accepting an order moved no tokens. The lender's promised total was 100,000 after row 56 and after the refusal in row 57, and 150,000 after row 58. Its free cash was 50,000, then 0. Both sellers' order records point at the same lender cash record, and each names its own payout key.

### Collection

Row 60 is the buyer's owner revoking agent A. Seller 1 reports 100,000 of completed work in row 61, after the revocation, and seller 2 reports 20,000 in row 67. One key signs and pays for every transaction in this table. It has no role in either order.

| Row | Attempt | Outcome | Error |
|---:|---|---|---|
| 62 | Collection of 50,000 on seller 2's order using seller 1's work record. Seller 2 has reported no work. | Refused | `FundingRun` 6028, ClaimsCore |
| 63 | Collection of 50,000 on seller 2's order naming a work record that does not exist | Refused | `AccountNotInitialized` 3012, ClaimsCore |
| 64 | Seller 1's earned 100,000, with agent A already revoked | Paid | |
| 65 | 1 more unit on seller 1's order, which is paid in full | Refused | `FundingTerms` 6031, ClaimsCore |
| 68 | Seller 2's earned 20,000 | Paid | |

After row 64 seller 1 held exactly 100,000 and the wallet held 50,000, all of it promised to seller 2. Seller 2's order record and its hold on the budget were byte-identical to before. Row 64's transaction carried one signature, and agent A's key is not among its accounts. After row 68 seller 2 had received exactly 20,000, seller 1's balance was unchanged, and the wallet held 30,000, all of it still promised.

`AccountNotInitialized` 3012 in row 63 comes from the Anchor framework's account check inside ClaimsCore, which runs before ClaimsCore's own handler.

### Lender withdrawal

| Row | Attempt | Outcome | Error |
|---:|---|---|---|
| 59 | 1 unit while all 150,000 is promised to the two sellers | Refused | `FundingCashShortfall` 6033, ClaimsCore |
| 66 | 1 unit of the 50,000 still promised to seller 2, after seller 1 is paid | Refused | `FundingCashShortfall` 6033, ClaimsCore |
| 69 | 30,000 while it is still promised to seller 2's order | Refused | `FundingCashShortfall` 6033, ClaimsCore |
| 71 | 30,001 with 30,000 free, after seller 2 released its unused 30,000 | Refused | `FundingCashShortfall` 6033, ClaimsCore |
| 72 | The released 30,000 | Accepted | |

Row 70 is seller 2 releasing the unused 30,000 of its order. The same withdrawal of 30,000 is refused in row 69, before the release, and accepted in row 72, after it. At the end the wallet holds 0. Of its 150,000, 100,000 went to seller 1, 20,000 to seller 2 and 30,000 back to the lender's owner. The budget shows 120,000 consumed and nothing held.

## The other rows

The tables above hold all 10 refusals and all 8 accepted cases. The remaining 118 transactions set up the cast or are the steps between cases named above: rows 60, 61, 67 and 70 of the wallet run and row 61 of the budget run.

## Where it ran

The compiled programs ran in LiteSVM from solders 0.29.0 under Python 3.12.3, with signature verification on and native v1 transactions. The token is one synthetic six-decimal SPL token and every key is a disposable test key. The runs started on November 18, 2026 at 9:03 AM US Eastern. They sent no transaction to a public network.

The binaries are the ones the [commit record](../README.md) ran, built from commit `e55c0ad3a224a1c4e72de4c283161345d438caaf` of the Dexter Account repository. Each trace records the SHA-256 of all 69 source inputs and of each binary:

| Program | Address | SHA-256 of the binary |
|---|---|---|
| ClaimsCore | `727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz` | `3ae2dc3c913c3ec696144c8e06104616f82a4d3305ff33e123c418eb446e0bb5` |
| PrincipalRegistry | `9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G` | `92a21620018609eb98f504f0bbfe8370eda6d10081ef1cc1d5c540903bf35214` |
| PoolIssuer | `3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm` | `6804757b94ce2da8854388a1f748b614296207682db27cde85958f868b726e65` |
| Swig | `swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB` | `bb744d9811c392e3f343952d1a99bef9c97aa1936248a23b9d7efc1adb8939c1` |

The source lines in the case index (`error_source`) refer to that commit. Its complete source, all 69 inputs, is in [../source/](../source/README.md). The [program source under `evidence/`](../../../evidence/account-evidence/execution/foundation/onchain/programs) is the earlier pin of the November 1 records, `fb2a3a4`.

## Files

| File | Contents |
|---|---|
| [shared-reserve-budget-trace.json.xz](shared-reserve-budget-trace.json.xz) | The budget run's 63 transactions: signed bytes, result, program log, and the observed accounts before and after. About 4 MB uncompressed. |
| [shared-reserve-wallet-trace.json.xz](shared-reserve-wallet-trace.json.xz) | The wallet run's 73 transactions, in the same form. About 6 MB uncompressed. |
| [case-index.json](case-index.json) | One entry per transaction of both runs: run, name, outcome, error name and number, the program that raised it and the source line its log names. |
| [shared-reserve-receipt.json](shared-reserve-receipt.json) | Counts and trace hashes for both runs, and the binary hashes. |
| [shared-reserve-budget-receipt.json](shared-reserve-budget-receipt.json), [shared-reserve-wallet-receipt.json](shared-reserve-wallet-receipt.json) | The same counts and hashes for each run on its own. |
| [shared-reserve.log](shared-reserve.log) | The three lines the runs printed. |
| [shared_reserve.py](shared_reserve.py) | The test module. It reuses the helpers in the commit record's [commit_legs.py](../commit_legs.py). |
| [provenance.json](provenance.json) | The source commit and the source and published hash of each file. |
| [SHA256SUMS](SHA256SUMS) | Hashes of the nine files above. |

The traces, case index, receipts and test module are byte-identical to the source commit. In the log, the absolute workspace path is replaced with `<fixture-workspace>` in two places. Each trace's `fixture_inputs` lists the hash of every test file it ran with, including `commit_legs.py`.

## Check a row yourself

Download a trace, then print any row's name, result, state comparison and program log. This reads row 58 of the budget run:

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
" shared-reserve-budget-trace.json.xz 58
```

```text
budget refused: agent B orders 100,000 from seller 2; the shared budget has 50,000 left after agent A's order
expected: reject | error: {'InstructionError': [0, {'Custom': 6012}]}
state unchanged: True
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz invoke [1]
Program log: Instruction: AcceptFinancedTab
...
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G invoke [2]
Program log: Instruction: Reserve
...
Program log: AnchorError thrown in programs/principal-registry/src/state.rs:92. Error Code: BudgetExhausted. Error Number: 6012. Error Message: Shared lifetime expenditure budget is exhausted.
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G failed: custom program error: 0x177c
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz failed: custom program error: 0x177c
```

The elided lines are system program calls and compute-unit counts. `transaction_base64` in the same row is the signed transaction, and `observed_before` and `observed_after` hold each observed account's owner, lamports and data.

To check the files against their hashes, and each decompressed trace against `trace_sha256` in its receipt:

```sh
sha256sum -c SHA256SUMS
xz -dc shared-reserve-budget-trace.json.xz | sha256sum
xz -dc shared-reserve-wallet-trace.json.xz | sha256sum
```

The second command prints `6a779f8d8ca6531ecf286de78973fbf7a7dfdce90f2a244a9abd13bbe9214c27` and the third prints `7db4bd293b5fd868f5dad4e1d35cbd8163d0344d9c9d4eb772c6f4e22dad40a1`.

The [commit record](../README.md) covers the three checks on a single order. The [claim guide](../../../articles/the-account-inversion/README.md) maps the article to its records.
