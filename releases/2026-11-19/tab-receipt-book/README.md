# The Tab receipt book

A settled Tab closes and returns every deposit to the account that paid it, and nothing the Tab signed can be used twice. The programs no longer store a receipt for each step. Each Tab keeps a step count and a running fingerprint of its events, and every payment of a chosen amount carries a number that can be used once. One order, from the commit to its close, created 3 accounts on 0.0169 SOL of deposits and left nothing locked. The same order on the earlier programs created 10 accounts and left 0.1733 SOL locked for good. A wallet send, a trade, a deposit and a withdrawal under a standing permission each created no account. Two orders were settled and closed, and then every step they had signed was sent again: of 124 transactions, 41 were refused, and no closed Tab reopened. Each kind of numbered payment was sent twice, and the second was refused every time, across 143 transactions with 26 refusals. The run used the compiled Account programs and a synthetic token.

The deposit figures were measured at the local test rate of 6,960 lamports per byte. The public networks charge 5,080, so real deposits are lower. [excess-rent-sizes.json](excess-rent-sizes.json) gives the local rate in `created_at_rate` and the size of each record: a Tab is 1,359 bytes, and with 128 bytes of account overhead its deposit in this run was 10,349,520 lamports. On Solana devnet on November 19 the same record took a deposit of 7,553,960 lamports, which is 5,080 per byte ([devnet run record](../devnet/devnet-record.json), `closes`).

The programs ran in LiteSVM, a local Solana virtual machine, on November 19, 2026, with a synthetic token and disposable keys. Nothing in this record was sent to a public network. "The earlier programs" are Dexter Account main at `8805fd3`, the binaries of the [November 18 permission-rules record](../../2026-11-18/permission-rules/README.md#where-it-ran), run in the same harness. In the files here that run is the side called `main`, and this record's build is the side called `branch`.

## What each flow creates and what it leaves locked

[flow-rent.md](flow-rent.md) lists every transaction and every account behind each row, and [flow-rent.json](flow-rent.json) holds the same data with each account's address. An account counts as created by a flow when one of the flow's transactions finds it absent and leaves it present. Its deposit is the lamports it held when created. "Locked after" is what those accounts still hold after the flow's last transaction. Amounts are SOL.

| Flow | Earlier programs: accounts, locked after | This build: accounts, deposit, locked after | Trace here |
|---|---|---|---|
| Order, commit to close: cash repayment settles it, closes appended to the settling repayment | 10, 0.173297040 | 3, 0.016891920, 0.000000000 | [main](traces/main/own-money-trace.json.xz), [branch](traces/branch/own-money-trace.json.xz) |
| Order, commit to close: Swig-governed final payment, keeper sends the closes separately (Tab records; its exact repayment approvals are the next rows) | 14, 0.320765520 | 5, 0.024506160, 0.000000000 | not published |
| Exact repayment approval: grant, then close_approval after expiry (the payment it authorizes is measured with its order) | 1, 0.005714160 | 1, 0.005547120, 0.000000000 | [branch](traces/branch/double-charge-trace.json.xz) |
| Evidence order, commit to close: no records, finalized at zero | 10, 0.146236560 | 3, 0.021847440, 0.000000000 | not published |
| Wallet send, standing grant | 2, 0.044133360 | 0, 0.000000000, 0.000000000 | [main](traces/main/wallet-spending-send-trace.json.xz), [branch](traces/branch/wallet-spending-send-trace.json.xz) |
| Wallet send, exact approval: grant, send, then (branch) close_approval after expiry | 3, 0.049743120 | 1, 0.005442720, 0.000000000 | [main](traces/main/wallet-spending-send-trace.json.xz) |
| Exact spending approval never used: grant, then close_approval after expiry | not measured | 1, 0.005442720, 0.000000000 | not published |
| SPL swap, standing grant | 3, 0.081000480 | 0, 0.000000000, 0.000000000 | not published |
| SPL swap, exact approval: grant, swap, then (branch) close_approval after expiry | 4, 0.086610240 | 1, 0.005442720, 0.000000000 | not published |
| Jupiter trade, standing grant | 3, 0.081000480 | 0, 0.000000000, 0.000000000 | not published |
| Jupiter trade, exact approval: grant, trade, then (branch) close_approval after expiry | 4, 0.086610240 | 1, 0.005442720, 0.000000000 | not published |
| Pump trade, standing grant | 4, 0.090862800 | 0, 0.000000000, 0.000000000 | not published |
| Pump trade, exact approval: grant, buy, then (branch) close_approval after expiry | 5, 0.096472560 | 1, 0.005442720, 0.000000000 | not published |
| Deposit: owner moves cash into the governed wallet's custody | 1, 0.036867120 | 0, 0.000000000, 0.000000000 | [main](traces/main/own-money-trace.json.xz), [branch](traces/branch/own-money-trace.json.xz) |
| Deposit: owner capital contribution into existing custody | 1, 0.036867120 | 0, 0.000000000, 0.000000000 | not published |
| Deposit: tokens into wallet trading custody | 2, 0.041064000 | 0, 0.000000000, 0.000000000 | not published |
| Withdrawal: owner withdraws free cash from custody | 1, 0.036867120 | 0, 0.000000000, 0.000000000 | [main](traces/main/own-money-trace.json.xz), [branch](traces/branch/own-money-trace.json.xz) |
| Withdrawal: tokens out of wallet trading custody | 2, 0.041064000 | 0, 0.000000000, 0.000000000 | not published |

Records created once per principal, wallet, facility, credit line, budget and permission are counted apart from the flows and stay open: 60 accounts holding 0.326041200 SOL on the earlier programs and 55 accounts holding 0.164339520 SOL on this build ([flow-rent.md](flow-rent.md), first two rows).

Where the last column names one build or none, the trace that row was measured from is not published with this record. Its figures are in flow-rent.md and flow-rent.json.

## One order from the commit to its close

The first row of the table is one order of 100,000 in the `own-money` run: the commit, the seller's report of completed work, the collection and the repayment that settles it. Both builds ran the same four transactions.

| Build | Account created | Deposit (lamports) | Held after the order (lamports) |
|---|---|---:|---:|
| Earlier programs | `principal_registry.OperationReceipt` | 2,143,680 | 2,143,680 |
| Earlier programs | `claims_core.FundedClaim` | 9,688,320 | 9,688,320 |
| Earlier programs | `claims_core.FinanceReceipt` | 36,867,120 | 36,867,120 |
| Earlier programs | `principal_registry.Reservation` | 3,807,120 | 3,807,120 |
| Earlier programs | `claims_core.FundedRun` | 5,964,720 | 5,964,720 |
| Earlier programs | `claims_core.FinanceReceipt` | 36,867,120 | 36,867,120 |
| Earlier programs | `claims_core.FundedPayment` | 2,081,040 | 2,081,040 |
| Earlier programs | `principal_registry.OperationReceipt` | 2,143,680 | 2,143,680 |
| Earlier programs | `claims_core.FinanceReceipt` | 36,867,120 | 36,867,120 |
| Earlier programs | `claims_core.FinanceReceipt` | 36,867,120 | 36,867,120 |
| Earlier programs | **10 accounts** | **173,297,040** | **173,297,040** |
| This build | `principal_registry.Reservation` | 2,136,720 | 0 |
| This build | `claims_core.FundedClaim` | 10,349,520 | 0 |
| This build | `claims_core.FundedRun` | 4,405,680 | 0 |
| This build | **3 accounts** | **16,891,920** | **0** |

On this build the Tab (`FundedClaim`), the budget hold (`Reservation`) and the run (`FundedRun`) close in the settling repayment, and each deposit returns to the account recorded as its payer when it was created. The repayment record is created and closed inside that same transaction, so it is not counted. 16,891,920 lamports is 0.0169 SOL and 173,297,040 lamports is 0.1733 SOL.

## Two settled orders, then every step sent again

[tab_book_replay.py](tab_book_replay.py) ran 124 transactions. 41 were refused and all 493 recorded assertions passed. Two orders ran to settlement: an ordinary order, number 5, for 10,000, and an order billed on a meter's report, number 20, for 2,000. Each closed in the transaction that settled it. Order 5's Tab closed after 8 steps and order 20's after 9, and [tab-book-replay-scenarios.json](tab-book-replay-scenarios.json) gives each Tab's final fingerprint and the deposit returned to each payer.

| Order | Records closed | Deposits returned (lamports) | Payers repaid |
|---|---|---:|---:|
| Ordinary order 5 | tab, hold, run 1, repayment record 1 | 20,699,040 | 3 |
| Evidence order 20 | tab, hold, run 1, repayment record 1, bill | 30,060,240 | 3 |

Then everything the two orders had signed was sent again. Rows are positions in the trace, and [case-index.json](case-index.json) names each one.

| What was sent again | Rows | Refusals | Error |
|---|---|---:|---|
| Each of the 13 steps, with its original signed bytes | 71, 73, 75, 77, 79, 81, 83, 85, 87, 89, 91, 93, 95 | 13 | `BlockhashNotFound`, before any program runs |
| The two commits, signed again by the same keys with a fresh blockhash | 72, 84 | 2 | `OrderUsed` 6061, PrincipalRegistry |
| The other 11 steps, signed again by the same keys with a fresh blockhash | 74, 76, 78, 80, 82, 86, 88, 90, 92, 94, 96 | 11 | `AccountNotInitialized` 3012, ClaimsCore: the hold, the Tab or the bill is gone |
| The commit signatures, the seller's capture and the buyer's numbered repayments, moved to another order | 97 to 99, 102, 105 to 106, 108 to 110, 119 | 10 | `SignatureFailure`, before any program runs |
| The meter's report and the seller's claim for order 20, presented to order 21's bill | 113 to 114 | 2 | `BillingSignature` 6063, ClaimsCore |
| A new commit, freshly signed, at closed order 5's number and at closed order 20's number | 121 to 122 | 2 | `OrderUsed` 6061, PrincipalRegistry |
| A freshly signed capture naming closed Tab 5 and its old run address | 123 | 1 | `AccountNotInitialized` 3012, ClaimsCore |

The 41 refusals in the table are all 41 of the run. Between them, 12 freshly signed records for new orders were accepted (rows 100 to 101, 103 to 104, 107, 111 to 112, 115 to 118, 120), so each refusal answers the copy and not the accounts it names. No Tab exists again at order 5's or order 20's address: the mandate keeps each order number it has used.

## Each numbered payment sent twice

[double_charge.py](double_charge.py) ran 143 transactions. 26 were refused and all 904 recorded assertions passed. A payment of a chosen amount carries a number taken from a window on the record it writes, and a number lands once. [double-charge-scenarios.json](double-charge-scenarios.json) lists the 17 cases below with the names of their transactions. In every case the file records that a refused transaction changed nothing the harness observes.

| Case | Refused | Error | Accepted in the same case |
|---|---:|---|---:|
| Lost response, ordinary repayment (route record window) | 1 | `NumberUsed` | 2 |
| Sibling attempts, ordinary repayment | 1 | `NumberUsed` | 1 |
| Sibling attempts, withdrawal | 1 | `NumberUsed` | 1 |
| Two intents sharing one number (why one allocator per record) | 1 | `NumberUsed` | 2 |
| Lost response, facility repayment (payer's book window) | 1 | `NumberUsed` | 2 |
| Lost response, withdrawal (custody window) | 1 | `NumberUsed` | 2 |
| Lost response, contribution (custody window) | 1 | `NumberUsed` | 2 |
| Lost response, seller-fee credit (beneficiary's book window) | 2 | `TabStale` (exact retry), `NumberUsed` (rebuild) | 2 |
| Lost response, correction cash leg (funder's book window) | 2 | `TabStale` (exact retry), `NumberUsed` (rebuild, after its token leg rolls back) | 2 |
| Third party repays the issuer's facility (one writer per window) | 0 | none: every payment lands | 3 |
| Rebuilt no-cash correction (company's book window) | 2 | `TabStale` (exact retry), `NumberUsed` (rebuild) | 2 |
| Rebuilt surrender release before expiry (supplier's book window) | 2 | `TabStale` (exact retry), `NumberUsed` (rebuild) | 2 |
| Exact-approval re-grant, governed repayment | 6 | `TabStale`, `RepaymentPermission`, `RepaymentPermission`, `RepaymentPermission`, `ApprovalNumber`, `RepaymentPermission` | 3 |
| Standing-grant governed repayment replay | 1 | `OrderUsed` | 2 |
| Settling repayment with appended closes retried after landing | 2 | `AccountNotInitialized` (both) | 1 |
| Close order: runs, then records, then the Tab | 0 | none: early closes are no-ops | 6 |
| Lost response, interest refund (lender's book window) | 2 | `TabStale` (exact retry), `NumberUsed` (rebuild, after its token leg rolls back) | 2 |

The 26 refusals by error: `NumberUsed` 6092 from ClaimsCore, 12; `TabStale` 6079 from ClaimsCore, 6; `RepaymentPermission` 6032 from PrincipalRegistry, 4; `AccountNotInitialized` 3012 from ClaimsCore, 2; `ApprovalNumber` 6065 from PrincipalRegistry, 1; `OrderUsed` 6061 from PrincipalRegistry, 1.

In the two cases with no refusal every payment lands. A third party repaying the credit issuer's facility uses a number from its own book, so the issuer's number stays free for the issuer. Closes sent out of order do nothing until the run has closed, then the record, then the Tab.

## Where it ran

The compiled programs ran in LiteSVM with native v1 transactions, a synthetic six-decimal SPL token and disposable test keys. Each trace states that it sent no transaction to a public network (`public_transactions_sent` is 0).

This record is pinned to Dexter Account main at `d204555ddf7f36a94aeebcfb83d05cc948c4ff95`, the merge of this change. Each of the four traces of this build records the SHA-256 of 76 build inputs, and all 76 equal the files at that commit. Sixty-seven of them are byte for byte the files under [../source/](../source/README.md), the source of the November 19 devnet build, which differs from this build only in the four program addresses and in the hashes its tests derive from them. The other nine are under [source/](source/NOTICE.md). The two traces of the earlier programs record the same 71 build inputs and the same four binary hashes as the [permission-rules record's](../../2026-11-18/permission-rules/README.md#where-it-ran) trace.

| Program | Bytes | SHA-256 of the binary |
|---|---:|---|
| ClaimsCore | 2,442,816 | `923e6b866d2560708c2303938f19ec4eecd506f48721ff2b7125d7f95d9a7fdd` |
| PrincipalRegistry | 1,051,464 | `d957129e317558324ec876224e58a5a9c5ba2613e6e9f6f8eab72b54e04c4243` |
| PoolIssuer | 197,432 | `2f84b587851464ba1e05785f07653a861e079f2d0d44bd9b34df65eb8c27b7be` |
| DeliveryRules | 276,224 | `7fd56bcae46c6f1d2587e1ff801daed8745887c15d0992f2af4272c83478f7d5` |

| Trace | Transactions | Refusals | Assertions | SHA-256 of the JSON inside the trace |
|---|---:|---:|---:|---|
| [traces/branch/tab-book-replay-trace.json.xz](traces/branch/tab-book-replay-trace.json.xz) | 124 | 41 | 493 | `709c65806cff08449e4d424435b15ca220fc21617c0eb6be5183da25f86996c7` |
| [traces/branch/double-charge-trace.json.xz](traces/branch/double-charge-trace.json.xz) | 143 | 26 | 904 | `c3733a7fe0f0c3c1ff970b7cf968dc0a97a137211fbeee94819e378eb55f7426` |
| [traces/branch/own-money-trace.json.xz](traces/branch/own-money-trace.json.xz) | 65 | 2 | 160 | `f3894263a3df65f0b9f570cfeb8798715215033756549c81f5f4db4379b00d73` |
| [traces/branch/wallet-spending-send-trace.json.xz](traces/branch/wallet-spending-send-trace.json.xz) | 108 | 23 | 280 | `4fd8d4cc892349952f655ef65c5f5e37d8b94e97c5f880c539c91e11fd7f2421` |
| [traces/main/own-money-trace.json.xz](traces/main/own-money-trace.json.xz) | 65 | 2 | 146 | `4f5f8ddd3afb059cbe2c4eef995c40f728139d5f9f22e95d138c9f2c95f75761` |
| [traces/main/wallet-spending-send-trace.json.xz](traces/main/wallet-spending-send-trace.json.xz) | 108 | 21 | 278 | `7f47c231b8f7900d366a1122cea8fc0ef4169db87ef71f390ad5bd75169fc288` |

The earlier programs ran with one change to the test harness and none to the programs: the harness also records each account's lamports before and after every transaction, which the flow table reads. [main-harness.patch](main-harness.patch) is that change.

## Files

| File | Contents |
|---|---|
| [traces/branch/](traces/branch) | Four traces of this build: the replay run, the numbered-payment run, and the two other runs the published flow rows were measured from. Each holds every transaction's signed bytes, result, program log and observed accounts. The replay trace is about 12 MB uncompressed and the numbered-payment trace about 17 MB. |
| [traces/main/](traces/main) | Two traces of the earlier programs, for the same flow rows. |
| [case-index.json](case-index.json) | One entry per transaction of the replay and numbered-payment traces, 267 in all: name, outcome, error name and number, the program that raised it and the source line its log names. |
| [make_case_index.py](make_case_index.py) | The script that wrote the case index from the two traces. It was written for this release. |
| [tab-book-replay-scenarios.json](tab-book-replay-scenarios.json) | The replay run's own result file: its counts, each order's steps, close, fingerprint and deposits, and each replayed step with its error. |
| [double-charge-scenarios.json](double-charge-scenarios.json) | The numbered-payment run's own result file: the 17 cases with their refused and accepted transactions. |
| [flow-rent.md](flow-rent.md), [flow-rent.json](flow-rent.json) | Every flow on both builds, with each transaction and each account it created. |
| [flow-rent.py](flow-rent.py), [flow-rent-spec.json](flow-rent-spec.json) | The script that measured the flows and the file that names each flow's transactions. |
| [excess-rent-sizes.json](excess-rent-sizes.json) | The size of every record type, measured from live accounts, with the rate the runs used. It is the result file of a separate run whose traces are not published here. |
| [tab_book_replay.py](tab_book_replay.py), [double_charge.py](double_charge.py) | The two test modules. They build on the Account repository's test fixtures. |
| [main-harness.patch](main-harness.patch) | The harness change the earlier programs ran with. |
| [source/](source) | The nine build inputs of this build that differ from [../source/](../source/README.md), with their [notice](source/NOTICE.md). |
| [provenance.json](provenance.json) | The source commit and the source and published hash of each file. |
| [SHA256SUMS](SHA256SUMS) | Hashes of every file above. |

The traces, flow-rent files, script, spec, patch and test modules are byte-identical to commit `d204555`. Each test module's hash equals its entry in its trace's `fixture_inputs`. In the two scenario files and in excess-rent-sizes.json the absolute workspace path is replaced with `<fixture-workspace>`, in one, one and five places. The source files keep their existing copyright, and this release adds no license grant for them.

The source record holds more than is published here: the other traces of both builds, the comparison of every earlier test module on both builds, the client tests, the local-validator runs and the build, unit-test and interface logs. No run log exists for the two suites, so the counts above come from the traces and the replay run's result file.

## Check it yourself

Download a trace, then print any row's name, result, state comparison and program log. This reads row 121 of the replay run, a freshly signed commit at closed order 5's number:

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
" traces/branch/tab-book-replay-trace.json.xz 121
```

```text
ordinary order 5: a new commit, signed afresh with a new request, recreates order 5's address
expected: reject | error: {'InstructionError': [0, {'Custom': 6061}]}
state unchanged: True
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz invoke [1]
Program log: Instruction: AcceptFinancedTab
...
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G invoke [2]
Program log: Instruction: Reserve
...
Program log: AnchorError thrown in programs/principal-registry/src/lib.rs:475. Error Code: OrderUsed. Error Number: 6061. Error Message: Order number was already committed under this mandate.
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G failed: custom program error: 0x17ad
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz failed: custom program error: 0x17ad
```

The elided lines are system program calls and compute-unit counts. The program addresses in these traces are the ones the local runs used. With `traces/branch/double-charge-trace.json.xz` and row 67, the same command prints a repayment built again with a number that had landed, refused with `NumberUsed`, error 6092, at line 54 of `programs/claims-core/src/funding_state.rs`.

To count what one order created and what it left locked, on either build:

```sh
python3 -c "
import json, lzma, sys
rows = json.loads(lzma.open(sys.argv[1]).read())['transactions']
start = next(i for i, r in enumerate(rows) if r['name'].startswith('ACCEPTED: the buyer\'s agent orders 100,000'))
tab = next(k for k, (b, a) in rows[start]['account_lamports'].items() if b is None and a and a[2] in (1264, 1359))
made, held = {}, {}
for r in rows[start:]:
    if r['result'].get('err') is None and tab in r['account_keys']:
        for k, (b, a) in r['account_lamports'].items():
            if b is None and a and k != r['account_keys'][0]: made.setdefault(k, a[0])
            if k in made: held[k] = a[0] if a else 0
print(len(made), 'accounts,', sum(made.values()), 'lamports deposited,', sum(held.values()), 'held after')
" traces/branch/own-money-trace.json.xz
```

It prints `3 accounts, 16891920 lamports deposited, 0 held after`. With `traces/main/own-money-trace.json.xz` it prints `10 accounts, 173297040 lamports deposited, 173297040 held after`. The Tab is the account of 1,359 bytes the commit creates, or 1,264 bytes on the earlier programs.

To write the case index again from the traces, and to check the files against their hashes and a decompressed trace against the table above:

```sh
python3 make_case_index.py
sha256sum -c SHA256SUMS
xz -dc traces/branch/tab-book-replay-trace.json.xz | sha256sum
```

The first command prints `tab-book-replay: 124 transactions, 41 refused, 493 assertions` and `double-charge: 143 transactions, 26 refused, 904 assertions`. The last prints `709c65806cff08449e4d424435b15ca220fc21617c0eb6be5183da25f86996c7`.

The [November 19 devnet run](../devnet/README.md) sent three orders through these programs on a public network, each settled and closed. The [source](../source/README.md) of that build is published with it.
