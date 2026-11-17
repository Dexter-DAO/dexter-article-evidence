# Money that keeps earning while orders are open

On July 18, 2026, on Solana mainnet, Dexter's Vault program moved an owner's dollars into a yield-bearing token and back, twice. The token stayed in the owner's wallet the whole time. Each trade carried the owner's passkey signature, and the program checked the wallet's balances before and after the trade inside the same transaction.

The same day's test run covers a wallet with obligations outstanding. The program allowed a purchase that left enough stablecoin in the wallet to cover them, and it refused the purchase that did not.

## The mainnet run

All nine transactions are final and none failed. The earning token is Maple's syrupUSDC, whose yield accrues to whoever holds it. Here the holder was the owner's wallet.

| Time (UTC) | Step | Owner's wallet after the step | Transaction |
|---|---|---|---|
| 10:58:47 | Funded with 2.000000 USDC | 2.000000 USDC | [5it5rQgo](https://explorer.solana.com/tx/5it5rQgoZ5rGeZ2eGeesM8g9ABNV72E9WShhmspFURsPkwqyKPV9qM9szSYpdiLz2PEXkP23rqMBcuDq7oug8keR) |
| 10:58:48 | Vault set up under the owner's passkey | 2.000000 USDC | [5ejecxVF](https://explorer.solana.com/tx/5ejecxVFsPU5m3Cvuz8fSaTxQLYAT7bTiHpsuwh18hJd1NtFxa2fufQY8wx8S2YM8SJDVdPWKozf7okhm1nA5Hiv) |
| 11:06:43 | Bought the earning token with 1.000000 USDC | 1.000000 USDC and 0.851800 syrupUSDC | [3a91Esk3](https://explorer.solana.com/tx/3a91Esk3yyC2uictro8in3wyT89tBevZAD75v5CG9aWNHKKDnzUz9LrvPp1mEYRBmdRg9tLw86oNJ75S6ZsUMSJa) |
| 11:06:57 | Sold the whole position | 1.999798 USDC | [5q7CEBBu](https://explorer.solana.com/tx/5q7CEBButxXi4gsZ5e9vVHxPdqQJB3o2Yf17Byo79JrqodvoThLk3R657UvczFwGWv3fNcWkUQ8ATHuUqJxXqbrc) |
| 11:07:28 | Funded with 2.000000 USDC more | 3.999798 USDC | [4TSmiVUx](https://explorer.solana.com/tx/4TSmiVUx1EfmB87hcyWyDdSKA849CjLe6nLjpcQQ6yQpDp9Ya29iWj5b9xf1vQTfpaDX7NzbCwgbVR8wfxV26rgy) |
| 11:07:32 | Bought the earning token with 1.000000 USDC | 2.999798 USDC and 0.851800 syrupUSDC | [3fnTzwP3](https://explorer.solana.com/tx/3fnTzwP3wPoJCNysVaHYFCm3RjRKBu7N5PL3CE5PGdNV2kbvr8cCZuc8Q3tZk6uaq6TLGhHckEUmktkAjeYBFZmX) |
| 11:07:46 | Sold the whole position | 3.999596 USDC | [28puEGSV](https://explorer.solana.com/tx/28puEGSVDXWukxdHDATkAK4drdkcYqqTKybDMAzApjnexS1YUmfjyx2JevBTrhfB1CRbc27yBUsb7rWzRPkSUrED) |
| 11:07:49 | Owner requested a withdrawal | 3.999596 USDC | [3AJzQqwt](https://explorer.solana.com/tx/3AJzQqwtz3Y2dewLzm89wQCzucCC5Aa2Fnm2qgje5Gup8Xt3efiXn76ds1qo34JJuxB3Nhnr2ZUAJeBmrDLvdJwW) |
| 11:07:50 | Withdrawal paid out | 0 | [5QGAzPYP](https://explorer.solana.com/tx/5QGAzPYPbAT3C1JuVXJfqJSy21X9jJ3fRfw4V6gTmG2X2y4PP2t5Wm7JpS6PAkwQT7Q4bmKFdeuats1NzZZFj8zm) |

Each round trip returned 0.999798 of the 1.000000 USDC it started with. The 0.000202 USDC difference is the cost of trading through the pool.

[mainnet-transactions.json](mainnet-transactions.json) holds the slot, time, programs and token balances of every transaction, read from mainnet on November 17, 2026.

## The rule the program enforces

A purchase of the earning token goes through only if the stablecoin left in the wallet still covers what the wallet owes. That is the amount locked for sellers and, where a lender financed the owner, the amount drawn plus interest. These lines are from `finish_swap.rs` in the Vault program at commit `1d6e752`, the version [deployed to mainnet](https://explorer.solana.com/tx/3yUCMti9aS48LKGca7toqxC7QnBR6v37FZWmZXZ6J1ZDHPgs4HZGj2KQnY8cEVc6jcUDrzFtf9ViBXwaiLtwDAsH) at 07:50 UTC on July 18:

```rust
// Reservation invariant (finalize_withdrawal's, verbatim logic):
// post-buy USDC must still cover crystallized locks + the node's
// full credit liability + pro-forma accrued interest.
let mut reserved = vault.outstanding_locked_amount;
if vault.node != Pubkey::default() {
    let node = ctx
        .accounts
        .node
        .as_ref()
        .ok_or(error!(VaultError::CreditNodeRequired))?;
    require_keys_eq!(node.key(), vault.node, VaultError::WalletNodeMismatch);
    reserved = reserved
        .checked_add(node.subtree_draw)
        .ok_or(error!(VaultError::SwapWouldViolateReservation))?;
    reserved = reserved
        .checked_add(crate::accrual::pro_forma_accrued(node, clock.unix_timestamp)?)
        .ok_or(error!(VaultError::SwapWouldViolateReservation))?;
}
require!(
    usdc_after >= reserved,
    VaultError::SwapWouldViolateReservation
);
```

The same check guards withdrawals, so the owner can invest or spend the surplus down to what the wallet owes and no further.

## The test run with obligations outstanding

The July 18 test suite ran this program on a local validator loaded with a copy of the live syrupUSDC pool, so every trade priced against real liquidity.

| Test | Result |
|---|---|
| A. Buy the earning token with 25.000000 USDC | Passed. The wallet received 21.296038 syrupUSDC. |
| B. Sell the full position | Passed. The wallet received 24.994999 USDC. |
| c8. Buy that would leave less USDC than the amount locked for sellers | Refused with `SwapWouldViolateReservation` |
| c8b-0. Buy that leaves the lender's reserved amount in place | Passed |
| c8b-1. Buy that dips below the lender's reserved amount | Refused with `SwapWouldViolateReservation` |
| D. Wallet balances after all eleven refused attempts | Unchanged |

The other refused attempts were a trade without its closing check, a closing check aimed at a different vault, a second transfer hidden inside the trade, a trade routed to another program, a minimum changed after signing, a replayed approval, an expired approval, a disabled asset, an approval signed for a different vault and an overstated spend.

[test-run-2026-07-18.txt](test-run-2026-07-18.txt) is the 14-test log and [test-run-lender-reservation-2026-07-18.txt](test-run-lender-reservation-2026-07-18.txt) is the 3-test log. Both were committed to the Vault repository on July 18, at `1d6e752` and `bf2f232`. Terminal color codes and a Node.js startup warning have been removed. Nothing else is changed.

## Check it yourself

Open any transaction above in a Solana explorer. Each trade shows the Vault program, the passkey signature check and the owner's two token balances before and after. To read one without an explorer, put its signature into this request:

```sh
curl -s https://api.mainnet-beta.solana.com -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"getTransaction","params":["SIGNATURE",{"encoding":"jsonParsed","maxSupportedTransactionVersion":0}]}'
```

```text
Vault program   Hg3wRaydFtJhYrdvYrKECacpJYDsC9Px7yKmpncj2fhc
Owner's wallet  6KqP3LHpyGwHL1Ni8u7Hc7cTFdcU9qCDiXNtA5UmSZFk
Vault account   7biWWeoDtniC8djiyStts8VzWdXHXPhTyeHztoGimEet
USDC            EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v
syrupUSDC       AvZZF1YaZDziPY2RCK4oJrRVrbN3mTD9NL24hPeaZeUj
```

[SHA256SUMS](SHA256SUMS) lists the hashes of the three record files.
