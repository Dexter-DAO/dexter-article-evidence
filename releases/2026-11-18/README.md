# One transaction checks the permission, the money and the deal

When a seller accepts an order, one Solana transaction checks three things: the owner's permission, the money behind the order, and the seller and the exact terms. On November 18, 2026, the compiled Account programs accepted an order with a maximum of 100,000 units and all three present. They refused 23 attempts that each lacked one of the three: eight for permission, five for money and ten for the deal. After every refusal, each account the transaction could write, other than the fee payer, held the same bytes as before.

The same run follows the accepted order to payment. A key with no role in the order collected the seller's earned 60,000 from the lender's wallet to the seller, and eight altered versions of that collection were refused. The lender withdrew its free cash and was refused one unit more.

The programs ran on a local Solana virtual machine with a synthetic token. The run has 117 signed transactions, and all 213 recorded assertions passed. [case-index.json](case-index.json) names every transaction, and [commit-legs-trace.json.xz](commit-legs-trace.json.xz) holds each one's signed bytes, program log and account state.

## The scenario

One buyer, one agent, one seller, one credit issuer and one lender. Amounts are base units of the synthetic token. Orders carry no interest and no seller fee, so the seller collects exactly what it earned.

| | |
|---|---:|
| Order maximum, every order | 100,000 |
| Agent's per-order limit | 100,000 |
| Buyer's budget | 500,000 |
| Lender facility limit | 500,000 |
| Credit line limit | 500,000 |
| Lender cash at the start | 1,000,000 |
| Work the seller completes on order 1 | 60,000 |
| Agent's enrollment | 3,600 seconds |
| Agent's permission | 7,200 seconds |

In the tables below, "Row" is the zero-based position in the trace's `transactions` array and equals `index` in the case index. "Error" is the name and number in the program log, followed by the program that raised it. The case index labels permission `Q1`, money `Q2`, the deal `Q3` and collection `weld`.

## The commit

The instruction is ClaimsCore `accept_financed_tab`. ClaimsCore calls PrincipalRegistry and PoolIssuer inside it, so the three checks pass or fail together.

| Row | Attempt | Outcome |
|---:|---|---|
| 85 | Order 1, maximum 100,000. The agent is enrolled and inside its 100,000 limit and the 500,000 budget, the lender has 100,000 free, and the seller and the credit issuer's owner signed. | Accepted |

For row 85 the trace records one top-level instruction signed by the fee payer, the agent, the credit issuer's owner and the seller. The buyer's owner and the lender's owner did not sign. The hold on the buyer's budget rose from 0 to 100,000. The lender's promised total rose from 0 to 100,000 and its free cash fell from 100,000 to 0. No tokens left the lender's wallet.

### Permission

The agent must be enrolled by the current owner, hold its current key, and stay inside the owner's limits, budget and time.

| Row | Attempt | Outcome | Error |
|---:|---|---|---|
| 66 | A key the owner never enrolled signs order 1 as the agent | Refused | `CredentialKey` 6005, PrincipalRegistry |
| 67 | Order of 100,001 against the per-order limit of 100,000 | Refused | `PerOperationLimit` 6011, PrincipalRegistry |
| 68 | Order 1 would stay open one second longer than the agent's permission | Refused | `Expired` 6009, PrincipalRegistry |
| 70 | Order 1 (100,000) against a remaining budget of 99,999 | Refused | `BudgetExhausted` 6012, PrincipalRegistry |
| 107 | The agent's old key signs order 3 after the owner replaced it | Refused | `CredentialKey` 6005, PrincipalRegistry |
| 108 | Order 3 signed by the agent's replacement key | Accepted | |
| 110 | The revoked agent signs order 4 | Refused | `CredentialInactive` 6004, PrincipalRegistry |
| 112 | Order 4 at 3,599 seconds, the last second of the agent's enrollment | Accepted | |
| 113 | Order 5 at 3,600 seconds, when the agent's enrollment has ended | Refused | `Expired` 6009, PrincipalRegistry |
| 116 | Order 5 from an agent the previous owner enrolled, after the buyer's owner changed | Refused | `AuthorityEpoch` 6034, PrincipalRegistry |

### Money

The lender's facility, the credit line and the lender's free cash must each cover the order's maximum, under an approval from the lender's current owner.

| Row | Attempt | Outcome | Error |
|---:|---|---|---|
| 73 | Order 1 (100,000) against a lender facility limit of 99,999 | Refused | `FundingFacility` 6025, ClaimsCore |
| 76 | Order 1 while the lender's facility is closed to new orders | Refused | `FundingAdmission` 6040, ClaimsCore |
| 79 | Order 1 (100,000) drawn on a credit line whose limit is 99,999 | Refused | `FundingCreditLine` 6026, ClaimsCore |
| 81 | Order 1 (100,000) against lender free cash of 99,999 | Refused | `FundingCashShortfall` 6033, ClaimsCore |
| 103 | Order 2 after the lender's owner changed and before the new owner approved the facility | Refused | `FundingAuthorityGeneration` 6068, ClaimsCore |
| 105 | Order 2 after the lender's new owner approved the facility | Accepted | |

### The deal

The seller and the credit issuer's owner sign the exact terms, and the seller is the one the owner's permission names.

| Row | Attempt | Outcome | Error |
|---:|---|---|---|
| 55 | The seller does not sign order 1 | Refused | `OwnerAuthority` 6001, ClaimsCore |
| 56 | The credit issuer's owner does not sign order 1 | Refused | `OwnerAuthority` 6001, ClaimsCore |
| 57 | Another key signs order 1 in the seller's place | Refused | `FundingOwner` 6023, ClaimsCore |
| 58 | Another key signs order 1 in place of the credit issuer's owner | Refused | `FundingOwner` 6023, ClaimsCore |
| 59 | Order 1 names a payee other than the signing seller | Refused | `FundingTerms` 6031, ClaimsCore |
| 60 | Order 1 says 100,000 and the seller-side terms say 99,999 | Refused | `FundingTerms` 6031, ClaimsCore |
| 61 | Order 1 carries an expiry that has already arrived | Refused | `FundingTerms` 6031, ClaimsCore |
| 62 | Order 1 is sent at unit price 2 when the terms commit to unit price 1 | Refused | `FundingUsage` 6044, ClaimsCore |
| 63 | Order 1 carries assignment terms the credit line did not agree | Refused | `FundingAssignment` 6046, ClaimsCore |
| 65 | A different registered seller signs order 1, and the owner's permission names the first seller | Refused | `IntentMismatch` 6010, PrincipalRegistry |

`OwnerAuthority` in rows 55 and 56 is PrincipalRegistry's owner-authority check compiled into ClaimsCore, which is why ClaimsCore raises it. The case index files row 65 under both permission and the deal.

Row 64 is order 1 signed at 100,000 and then sent with the amount changed to 100,001 and the four signatures kept. The runtime's signature check rejected it with `SignatureFailure` before any program ran.

## Collection of the seller's 60,000

The seller reports 60,000 of completed work on order 1. One key signs and pays for every transaction in this table. It is not the buyer's owner, the agent, the lender's owner, the credit issuer's owner or the seller. The unchanged collection has two instructions: PrincipalRegistry `authorize_funding_wallet`, then the lender's Swig wallet signing ClaimsCore `collect_financier`.

| Row | Attempt | Outcome | Error |
|---:|---|---|---|
| 88 | Payee swapped to another principal's token account | Refused | `FundingTokenRouting` 6035, ClaimsCore |
| 89 | Payee swapped to the collector's own token account | Refused | `FundingTokenRouting` 6035, ClaimsCore |
| 90 | Collection of 60,001 when the seller earned 60,000 | Refused | `FundingTerms` 6031, ClaimsCore |
| 91 | A second transfer of 1 unit to the collector inside the wallet's signed envelope | Refused | `FundingAction` 6030, PrincipalRegistry |
| 92 | An extra instruction after the wallet envelope | Refused | `WalletEnvelope` 6023, PrincipalRegistry |
| 93 | The wallet instruction sent without the Registry's authorization | Refused | `PermissionDeniedProgramExecInvalidInstruction` 3033, Swig |
| 94 | Another party's token account named as the source | Refused | `FundingAction` 6030, PrincipalRegistry |
| 95 | Another party's cash record and token account named as the source | Refused | `FundingTerms` 6031, ClaimsCore |
| 97 | The collection with nothing changed | Paid | |

After row 97 the seller's token account held exactly 60,000 more and the lender's wallet exactly 60,000 less. The collector received nothing, and the transaction carried one signature. Row 93 is refused by the Swig wallet program itself. Swig logs only the number 3033, and the name comes from its source.

## Lender withdrawal

With 50,000 free and 40,000 promised to order 1, the lender's withdrawal of 50,001 was refused with ClaimsCore `FundingCashShortfall` 6033 (row 99). Its withdrawal of exactly 50,000 succeeded (row 100), and the promised 40,000 stayed in the wallet.

## The other rows

The receipt counts 36 refusals. The tables above hold 32 of them and row 64 is the 33rd. Rows 83, 84 and 96 start from account state the test harness wrote directly, and the case index labels each `harness-written state`. The remaining 75 transactions set up the scenario or are the owners' steps between cases, such as lowering a limit and restoring it.

## Terms the owner pins

A further record pins terms of the deal on the agent's permission. The owner pinned size = 10. The agent's order for size 11 was refused with PrincipalRegistry `TermsNotPermitted` and nothing was held or recorded, and the same order for size 10 was accepted. An agent whose permission was not pinned ordered size 11 and was accepted. This record ran commit `ef9ae97`, which is the commit the other records on this page ran plus this change, and it publishes the six source files that differ. The tables, the traces and the test module are in [pinned-terms](pinned-terms/README.md).

## Orders that share one budget and one wallet

A second record from the same day, on the same binaries, has two agents of one buyer ordering from two sellers. Both agents draw on one budget of 150,000, and one lender wallet of 150,000 backs both sellers' open orders at once. An order that would overdraw either is refused, each seller is paid from the shared wallet while the other's share stays untouched, and unused backing returns to the lender only after the seller releases it. The tables, traces and test module are in [shared-reserve](shared-reserve/README.md).

## An order that names who signs for delivery

A third record from the same day, on the same binaries, has six orders that each name a signer for delivery. The seller was paid 900 when the named signer signed for 900 against a claim of 1,400, and nothing when the named signer never signed. The seller's own report of delivery was refused on such an order, and one order named the buyer as the signer. The tables, the trace, the test module and the two source files that implement the rule are in [named-signer](named-signer/README.md).

## After the owner revokes an agent

A fourth record from the same day, on the same binaries, follows an agent through its revocation. Three of its orders were accepted, the buyer's owner revoked it, and its fourth order was refused. The seller was then paid 160,000 on the first two orders by a key with no role, and a new agent placed a fifth order. The tables, the trace and the test module are in [revocation](revocation/README.md).

## One account on both sides

A fifth record has one company buying on one order and selling on another. Its single book showed 100,000 on the buying side and 150,000 on the selling side at the same time, and each seller was paid straight from the wallet of the lender behind its order. The company changed its agent and then its owner key, and neither order changed. The tables, the trace and the test module are in [both-sides](both-sides/README.md).

## A refund the seller funds and a decider releases

A sixth record has a seller holding 200,000 of its own cash in its own wallet behind two refund promises, each naming a decider. The seller could not withdraw while the promises were open. The decider signed for 30,000 on one, which was paid to the buyer, and never signed on the other, which paid nothing. The tables, the trace and the test module are in [refund-promise](refund-promise/README.md).

## Where it ran

The compiled programs ran in LiteSVM from solders 0.29.0 under Python 3.12.3, with signature verification on and native v1 transactions. The token is one synthetic six-decimal SPL token and every key is a disposable test key. The run started on November 18, 2026 at 8:42 AM US Eastern. It sent no transaction to a public network.

The program source is commit `e55c0ad3a224a1c4e72de4c283161345d438caaf` of the Dexter Account repository. The trace records the SHA-256 of all 69 source inputs and of each binary:

| Program | Address | SHA-256 of the binary |
|---|---|---|
| ClaimsCore | `727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz` | `3ae2dc3c913c3ec696144c8e06104616f82a4d3305ff33e123c418eb446e0bb5` |
| PrincipalRegistry | `9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G` | `92a21620018609eb98f504f0bbfe8370eda6d10081ef1cc1d5c540903bf35214` |
| PoolIssuer | `3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm` | `6804757b94ce2da8854388a1f748b614296207682db27cde85958f868b726e65` |
| Swig | `swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB` | `bb744d9811c392e3f343952d1a99bef9c97aa1936248a23b9d7efc1adb8939c1` |

The source lines in the case index (`error_source`) refer to that commit. Its complete source, all 69 inputs, is in [source/](source/README.md). The [program source under `evidence/`](../../evidence/account-evidence/execution/foundation/onchain/programs) is the earlier pin of the November 1 records, `fb2a3a4`.

## Files

| File | Contents |
|---|---|
| [commit-legs-trace.json.xz](commit-legs-trace.json.xz) | All 117 transactions: signed bytes, result, program log, and the observed accounts before and after. About 15 MB uncompressed. |
| [case-index.json](case-index.json) | One entry per transaction: name, outcome, error name and number, the program that raised it and the source line its log names. |
| [commit-legs-receipt.json](commit-legs-receipt.json) | Counts, the trace hashes and the binary hashes. |
| [commit-legs.log](commit-legs.log) | The two lines the run printed. |
| [commit_legs.py](commit_legs.py) | The test module. It prepares order 1 once, then changes one fact for each case. |
| [provenance.json](provenance.json) | The source commit and the source and published hash of each file. |
| [SHA256SUMS](SHA256SUMS) | Hashes of the six files above. |

The trace, case index, receipt and test module are byte-identical to the source commit. In the log, one absolute workspace path is replaced with `<fixture-workspace>`. `commit_legs.py` imports the Account repository's shared test fixture, and the trace's `fixture_inputs` lists the hash of each fixture file it ran with.

The [verifier](verifier/README.md) folder is a separate item: the current verifier for the October 30 mainnet record.

## Check a row yourself

Download the trace, then print any row's name, result, state comparison and program log. This reads row 67:

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
" commit-legs-trace.json.xz 67
```

```text
Q1 refused: order of 100,001 against a per-order limit of 100,000
expected: reject | error: {'InstructionError': [0, {'Custom': 6011}]}
state unchanged: True
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz invoke [1]
Program log: Instruction: AcceptFinancedTab
...
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G invoke [2]
Program log: Instruction: Reserve
...
Program log: AnchorError thrown in programs/principal-registry/src/lib.rs:272. Error Code: PerOperationLimit. Error Number: 6011. Error Message: Operation exceeds the per-operation ceiling.
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G failed: custom program error: 0x177b
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz failed: custom program error: 0x177b
```

The elided lines are system program calls and compute-unit counts. `transaction_base64` in the same row is the signed transaction, and `observed_before` and `observed_after` hold each observed account's owner, lamports and data.

To check the files against their hashes, and the decompressed trace against `trace_sha256` in the receipt:

```sh
sha256sum -c SHA256SUMS
xz -dc commit-legs-trace.json.xz | sha256sum
```

The second command prints `8cabc7431098f7d1d9ab5610bb0e1091514468de68026280e6eaa3a3d8a8c676`.

The [claim guide](../../articles/the-account-inversion/README.md) maps the article to its records.
