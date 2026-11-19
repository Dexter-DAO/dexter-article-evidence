# The Account programs on Solana devnet, November 19, 2026

On November 19, 2026 the four Account programs were deployed to Solana devnet (genesis `EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG`) and a sequence of 74 transactions was sent to them over public JSON-RPC between 4:02 and 4:10 PM Eastern. The programs accepted 62 and refused 12. Each refusal is the error the sequence named before sending. 447 checks on the resulting state passed. The same sequence on a local validator with the same build gave the same counts.

## The programs

Each program's bytes were read back from devnet after the deploy and equal the build.

- ClaimsCore, `DEXcor9xYbiaSyvdEjKprMZuJFwe3MdgwW5kVzUiHdtu`, 2,442,816 bytes, sha256 `6144105bd973f843080e9eb15013cc8ab20069608e35ea46627e9ccdfcb98e0a`, deployed in slot 524119684.
- PrincipalRegistry, `DEXregFGwJM4LsVAPeWf7pQm4JBNbZxijTRQzrxk1SQS`, 1,051,464 bytes, sha256 `64bbaea177e9a982f483e78b28877bb03ee94daaac1dc374c899e4b78105505e`, deployed in slot 524119556.
- PoolIssuer, `DEXissgoPzcY4bDsGfyKjy1bMxh3VFZ8yDChjjLrLg6x`, 197,432 bytes, sha256 `97d95b58df5c6fc60f92adfb1ca6a0e31be119f50d3a5b2564b50d6a05473119`, deployed in slot 524119584.
- DeliveryRules, `DEXdeLmbdmRi1V8XHUGzNCFYcTEZQdkpy9jywRRu8CVt`, 276,224 bytes, sha256 `55a6792033e1e29f4d898b0a12e3a66b46eae6b8990b2eaca2853e51456c61c2`, deployed in slot 524119613.

These are devnet deployments. Nothing is deployed on mainnet. On devnet all four programs are upgradeable by one key, and ClaimsCore accepts an upgradeable rules program only when it shares ClaimsCore's own upgrade authority.

The source is Dexter Account main at `cd2b2bb`. It is the November 19 Tab receipt book build with the four program addresses changed and nothing else: the binaries have the same sizes, and every test of that build gave the same counts.

Each program's interface (IDL) is published on devnet as a program-metadata account and reads back identical to the build: ClaimsCore 55 instructions, PrincipalRegistry 35, DeliveryRules 8, PoolIssuer 4.

## The commit

Three attempts were refused, one for each missing check, and then the order was accepted.

- Refused, money: an order of 100,000 when the lender's free cash was 99,999. ClaimsCore `FundingCashShortfall`, error 6033. [transaction](https://explorer.solana.com/tx/5EZYfStyPL3a7Nc35LfVBXj9gZibKFWVeFC1M6dffqjmBDFTBicnkK3wJCfSCfFf4QhEcmz513KHNSfnKsayZknp?cluster=devnet)
- Refused, permission: an order of 100,001 when the agent's per-order limit was 100,000. PrincipalRegistry `PerOperationLimit`, error 6011. [transaction](https://explorer.solana.com/tx/b6dtgdVJw9xRUmGn7udeXWgTZ7p19YpjH5hLZYAcDm3KYawfdcZQDyPo76FTZ65z5yWBvAYwmXe8bNWgXMSzDsL?cluster=devnet)
- Refused, deal: the order sent at a unit price of 2 when the agreed terms said 1. ClaimsCore `FundingUsage`, error 6044. [transaction](https://explorer.solana.com/tx/Eqz88rMhJLE9JwFEEeSD2xVxj4VgvCbkwfxUveKF73YuCg6vbBsJAfKujjZAxZTedkhgZCizmLzdwX3CvVtvhMZ?cluster=devnet)
- Accepted: order 1 of 100,000, inside the agent's 100,000 limit and the 500,000 budget, with 150,000 free in the lender's wallet and the seller and the credit issuer's owner signing. [transaction](https://explorer.solana.com/tx/uVzLohr8BeWnzK9VQFmNCdjoutkwPg8TKgExdgEQY4HEHuzY1JF6ofiGD88p3By1RCkoXHeTuB9ZjsR3HV5Gcgv?cluster=devnet)

## Collection, release and repayment

- The seller reported 60,000 of completed work.
- Refused: a collection with the payee swapped to the collecting key's own token account. ClaimsCore `FundingTokenRouting`, error 6035. [transaction](https://explorer.solana.com/tx/4BbbdrrggTK6ngjPNo3vYdVmb7svGDicGfayDsz4XCdWYfkaNbHWsPNGigg1EzLRWiRFRc3Ld9b4ps4pck7udQuj?cluster=devnet)
- Refused: a collection with an extra instruction after the wallet's signature. PrincipalRegistry `WalletEnvelope`, error 6023. [transaction](https://explorer.solana.com/tx/a3f6tmURYTznntaYhvEK9h9viL6vbaVQu48B2K5YuqX7dkvjJfJYBDzU59FB6EfhxCTLu2y9j74dmLBwuZECfEo?cluster=devnet)
- Accepted: a key with no role in the order paid the seller's earned 60,000 from the lender's wallet. [transaction](https://explorer.solana.com/tx/qKtPBvZmpM4btPNPZLkNJ2MGGP6E1Y4bhn9STUqViN8tP4dqPVHNCHUbmkfCBNnH3fAfwxt1mAi4tJ1QeDGsUKg?cluster=devnet)
- Refused: the lender asked to withdraw 50,001 when 50,000 was free and 40,000 was promised. ClaimsCore `FundingCashShortfall`, error 6033. [transaction](https://explorer.solana.com/tx/3GJGDQPaYJuy28Q9LcLhzG9h3taxUQypJJLBUUiDZyqCU3uPTqa9utLYaBnLNuDtV6SxkDA7UFPvsKMu1zkMxweL?cluster=devnet)
- Accepted: the seller released the order's unused 40,000. [transaction](https://explorer.solana.com/tx/3eZNX7aazSVZf5wMWvXhUnqKUM6m6eiDZPYovvWDVRMzdaVno46uw2omd1pphRkBHo1cXWxUzg5yLULY9aHJ4BkU?cluster=devnet)
- The buyer repaid 30,000 with route number 1. [transaction](https://explorer.solana.com/tx/3ejwL2oVZ7jSbFiRLGmZGGx3QwZ5jRTvTmXoKdpiNzrnwEcgLNUJu77srqaB8994n4xM8XbdaRTLXV3WQ3xSeTau?cluster=devnet)
- Refused: the same 30,000 built again against the order's new step count, with route number 1. ClaimsCore `NumberUsed`, error 6092. [transaction](https://explorer.solana.com/tx/5BLCGGtPyfyegcPyGYXAyfFgmU4HojGksLYDd1LwYYkNx1hMSjX6N8ZdEYLrfb6uR3aNp9y7SmYhPP2DKMEFJoSy?cluster=devnet)
- Settled and closed: the buyer repaid its last 30,000 with route number 2, and the run, its repayment record and the Tab with its budget hold closed in that transaction. [transaction](https://explorer.solana.com/tx/4qiULZmQ8p3x6FMAPEV8TegPsmFbj6LWu8DsLDKsaLAwiVeSDAMoQxr2mSpkC5TWxiDoTxPnHXQcY97EqnGTzdu5?cluster=devnet)

## Terms and price the owner pins

The buyer's owner created a permission for a second agent already pinned to size 10 and 1 per unit, in one instruction.

- Refused: an order of 50,000 for size 11. PrincipalRegistry `TermsNotPermitted`, error 6053. [transaction](https://explorer.solana.com/tx/2WY4d7cjw5u7GPa1Mh4Rxjt85ktZBNq9F2uYEY4hb9TjRy49sh8XPnoVprKUp7Fmn8rezqYUTe1axsJLxzygtvwt?cluster=devnet)
- Refused: the same order sent without showing its terms. PrincipalRegistry `TermsRequired`, error 6051. [transaction](https://explorer.solana.com/tx/5DLToGyCvQUhmcYjEYrWTp7Uq4om7rdRMfKfGhanEp7UnNtMxdv4cMs478eneApqcYQVQpgz2KpTjURHvNM7XPtM?cluster=devnet)
- Refused: size 10 at 2 per unit. PrincipalRegistry `TermsNotPermitted`, error 6053. [transaction](https://explorer.solana.com/tx/5HXmA2vUZnFtzAHBK2vp5PhCC6NDCuYmBDVc9KzaBMunsJuufdkDPGczurxcrpmqrYutKsqw3Rj2oX4memd3NxNg?cluster=devnet)
- Accepted: size 10 at 1 per unit, an order of 50,000. [transaction](https://explorer.solana.com/tx/1fT2a7SRvrfeGyAQTh6UU6puvpLnJXnB5SLe6tPS62oR5LrPBWQ1qChta3dS6d1nJ3SdZ8mXUJKPFWazEQsTQRG?cluster=devnet)
- The seller reported all 50,000, a key with no role collected it for the seller, and the buyer repaid 50,000.
- Closed by a keeper: the collecting key, which has no role in the order, sent the order's closes alone. Every deposit went to the account that paid it and the keeper paid only the fee. [transaction](https://explorer.solana.com/tx/Nq3umT2Z4QihBK7kT4NrvbHQ36AVy6UYovUD3DHW6T87wWNxFG56f5HgCJdPmxQKLD92W3maf1JJuLuDTiaqiCV?cluster=devnet)

## An order under a delivery rule

A rule was created under which the carrier alone signs for delivery and nobody may object. The owner pinned a permission to that rule's delivery-source address, and order 3, for 2,000 at 1 per unit, named it as its delivery signer.

- Accepted: order 3. [transaction](https://explorer.solana.com/tx/3g1KCvBuWsM6Vpzcdb2Z1NAVwSAyVCrwS9y2C4hECBWSihbavy79bsTPxhv2n7pSYfx5sTfcaBRHbvJvQ6iy41fN?cluster=devnet)
- Refused: the seller signed for 1,400 units on the order's case. DeliveryRules `NotASigner`, error 6005. [transaction](https://explorer.solana.com/tx/5Pr5im3oh29FAL4DbBSR2QYycCDdaPbF7SGoSPHpP17mmKjfoxhwtZsFGyKGbSgYqZRh2iT6xJKiNENeiTqQNBko?cluster=devnet)
- Accepted: the carrier signed for 900 units. [transaction](https://explorer.solana.com/tx/65oEzZamryxF722Q1Pg6dBwSDuBJutP4ELTKw9xBfrALARPsUa7QFd2UfVCMq9AWaGMm7NNX7FVYZHaGAEKv14QQ?cluster=devnet)
- Refused: a key with no role tried to settle the case while the objection window was open. DeliveryRules `ObjectionsOpen`, error 6016. [transaction](https://explorer.solana.com/tx/4U7Qn1aoqHxPzTPs3navVwEF9WgNzjtW5epdxnfb13HwqWXzL1A9g2Gdu7RcpYQHhTfhU6zEDRW6oD2ZGF8x2aG8?cluster=devnet)
- Settled: once objections closed, the same key settled the case, and the rules program reported the carrier's 900 units into ClaimsCore. [transaction](https://explorer.solana.com/tx/337MwM2sJ8d5gcbdGUaVU33i1Lnjo8jZPdGBJdxrUDVWiyS6czLbdPRBxECNLq5VTLgZoHvKiwh91tLdswdG4sW3?cluster=devnet)
- The seller claimed 1,400. The bill was finalized at 900 and the unused 1,100 was released. [transaction](https://explorer.solana.com/tx/3G3HQ3mkZg8BE2CQpdX5qxxNjtKjXaxRrEmY5jFgG8dpLFPrbZ8BXzjMgf4ZCJF8mnxyeoc7y65PtWZKGckgY6XR?cluster=devnet)
- A key with no role collected the seller's 900, the case closed and its deposit returned to the payer that opened it, and the buyer's repayment of 900 closed the run, the repayment record, the Tab, its budget hold and the bill in one transaction. [transaction](https://explorer.solana.com/tx/5QZ1aydUk5TKtYy39fVwn4aaCFSAG1aCxAunGhtmGJvNK2K4wHLNt3aEbdna98fCxE2oziZMEkArtbzTSdSTpetu?cluster=devnet)

## Where it ended

The credit issuer repaid the lender's facility 110,900. The lender's wallet held 150,000 again with nothing promised, the seller held 110,900, and the buyer's budget hold was zero.

## The record

`devnet-record.json` in this folder lists all 74 transactions with their signatures, slots, compute units and results, the programs' on-chain hashes, and the 447 checks. The largest transaction was 3,239 bytes and the most accounts in one transaction was 40.

## Files

| File | Contents |
|---|---|
| [devnet-record.json](devnet-record.json) | The run record: the cluster, the five programs the run uses and their on-chain hashes, the scenario, and every transaction with its signature, slot, time, result, compute units, fee, signers, program log, explorer link and the figures read before and after, with the 447 checks, the three closes and the deposit each returned, the payer's costs and the final figures. |
| [devnet.log](devnet.log) | The 68 lines the driver printed: its check of the programs before sending, and the run's last transactions, per-transaction table and summary. |
| [public_rpc_sequence.py](public_rpc_sequence.py) | The driver that sent the run, byte-identical at commit `cd2b2bb`. It imports the Account repository's transport and test fixture files, and the record gives the SHA-256 of each. |
| [provenance.json](provenance.json) | The source of each file, its source and published hash, and what was omitted. |
| [SHA256SUMS](SHA256SUMS) | Hashes of the four files above. |

The run's salt file, from which every key the run created derives, is not published, nor is the journal the driver keeps beside it. From the record's `run` section three fields are omitted: the local paths of the salt file and the journal, and the salt file's SHA-256. In the record `build.artifact_root` reads `<fixture-workspace>`, and in the log the record's path reads `<run-directory>/devnet-record.json`. Nothing else in the record changed. The log leaves out the lines the deploy script printed before the driver started. The published record holds no secret key, seed or salt: every 32-byte value in it was checked as an Ed25519 seed against the public keys it names, and every 64-byte value as a keypair.

The record's `cases` array lists the transactions in the order they were sent. Beside the four Account programs, `programs` lists the Swig wallet program the lender's wallet uses. The [source](../source/README.md) of the four Account programs is published with this record, and one hash in the record, `build.build_inputs_sha256`, covers all 76 of its files.

## Check it yourself

Read any transaction from the public devnet endpoint. This reads the accepted commit:

```sh
curl -s https://api.devnet.solana.com -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"getTransaction","params":["uVzLohr8BeWnzK9VQFmNCdjoutkwPg8TKgExdgEQY4HEHuzY1JF6ofiGD88p3By1RCkoXHeTuB9ZjsR3HV5Gcgv",{"encoding":"json","maxSupportedTransactionVersion":1}]}' \
  | python3 -c "import json,sys; r=json.load(sys.stdin)['result']; print(r['slot'], r['meta']['err']); print('\n'.join(r['meta']['logMessages'][:2]))"
```

The record gives this transaction slot 524120595, no error, and these first two log lines:

```text
Program DEXcor9xYbiaSyvdEjKprMZuJFwe3MdgwW5kVzUiHdtu invoke [1]
Program log: Instruction: AcceptFinancedTab
```

For the repayment sent again with a used number, `5BLCGGtPyfyegcPyGYXAyfFgmU4HojGksLYDd1LwYYkNx1hMSjX6N8ZdEYLrfb6uR3aNp9y7SmYhPP2DKMEFJoSy`, the record gives slot 524120780 and the error `{'InstructionError': [0, {'Custom': 6092}]}`. To check a program against its hash, dump it with the Solana CLI and hash the file:

```sh
solana program dump DEXcor9xYbiaSyvdEjKprMZuJFwe3MdgwW5kVzUiHdtu claims_core.so --url https://api.devnet.solana.com
sha256sum claims_core.so
```

The record's hash for ClaimsCore is `6144105bd973f843080e9eb15013cc8ab20069608e35ea46627e9ccdfcb98e0a`. To check the files against their hashes, run `sha256sum -c SHA256SUMS` in this folder.

The [Tab receipt book record](../tab-receipt-book/README.md) holds the local runs of the closes and of the numbered payments, with every refusal by row. The [November 18 devnet run](../../2026-11-18/devnet/README.md) is the earlier build at its earlier addresses.
