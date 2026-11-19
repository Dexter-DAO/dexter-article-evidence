# November 19, 2026: the Tab receipt book

This release holds three things: a record of the Account programs with the Tab receipt book on a local Solana virtual machine, a run of those programs on Solana devnet at four new addresses, and the source of the devnet build.

## The local record

[tab-receipt-book/](tab-receipt-book/README.md) measures what an order, a wallet send, a trade, a deposit and a withdrawal create and what they leave locked, on this build and on the programs before it. One order from the commit to its close created 3 accounts and left nothing locked, where the earlier programs created 10 and left all of them. Two orders were settled and closed and every step they had signed was sent again: 124 transactions, 41 refused. Each kind of numbered payment was sent twice: 143 transactions, 26 refused. The folder has six traces, a case index, the runs' result files, the flow table and the test modules.

## The devnet run

[devnet/](devnet/README.md) is the run of November 19 on Solana devnet: the four programs' on-chain hashes and sizes, and the commit, the collections, the repayments, the closes and the delivery rule with an explorer link for each. [devnet-record.json](devnet/devnet-record.json) lists all 74 transactions, 62 accepted and 12 refused, and the 447 checks.

The programs of the [November 18 devnet run](../2026-11-18/devnet/README.md) were read from the chain once more before this build replaced them, and those files are in [programs-at-close/](../2026-11-18/devnet/programs-at-close/README.md) in that record.

## The source

[source/](source/README.md) is the complete source of ClaimsCore, PrincipalRegistry, PoolIssuer, DeliveryRules and their shared types at the commit of the devnet build: 76 files. One hash over all of them equals the build-input hash in the devnet record.

## Files

| Folder | Contents |
|---|---|
| [tab-receipt-book/](tab-receipt-book/README.md) | The local record, pinned to Dexter Account `d204555`. |
| [devnet/](devnet/README.md) | The devnet page, the run record, the log and the driver, at Dexter Account `cd2b2bb`. |
| [source/](source/README.md) | The 76 build inputs at Dexter Account `c1e1d15`, which holds the same program source as `cd2b2bb`. |

Each folder has its own `provenance.json` and `SHA256SUMS`. To check all three from this folder:

```sh
for d in tab-receipt-book devnet source; do (cd "$d" && sha256sum -c SHA256SUMS); done
```

The [front page](../../README.md#what-the-records-show) lists these records beside the earlier ones.
