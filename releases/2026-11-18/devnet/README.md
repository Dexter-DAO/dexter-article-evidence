# The Account programs on Solana devnet

November 19, 2026: the programs at these devnet addresses were closed and replaced by the next build at [four new addresses](../../2026-11-19/devnet/README.md). The transactions below remain on devnet. Before the close, each program's on-chain bytes were read once more, at slot 524083120, and matched the hashes in this record exactly. Those files are published with this record.

On November 18, 2026, the Account programs of Dexter Account main ran on Solana devnet, a public Solana network. At 8:55 PM ET PrincipalRegistry was upgraded in place and ClaimsCore was deployed, and PoolIssuer was already there. Read from the chain, all four programs the run uses matched their builds byte for byte: ClaimsCore, PrincipalRegistry and PoolIssuer are the binaries of the [early-settlement](../early-settlement/README.md#where-it-ran) and [delivery-rules](../delivery-rules/README.md#where-it-ran) records, and the Swig wallet program matched its pinned hash.

From 8:55 to 8:59 PM ET the run sent 52 transactions: 43 were accepted and 9 were refused on chain by the programs. The commit accepted an order of 100,000 after refusing one attempt for each of its three checks, the money, the permission and the deal. A key with no role in the order collected the seller's earned 60,000 from the lender's wallet after two altered collections were refused, the lender's withdrawal of 50,001, one more than its free cash, was refused, and the seller released the unused 40,000. The buyer's owner created a permission already pinned to size 10 and a price of 1 per unit. Orders for size 11 and at 2 per unit were refused, and the order at the pinned values was accepted and paid 50,000. After each refusal every figure the run reads was unchanged, and every transaction read back identical once finalized. The payer spent 0.3679872 SOL.

| Program | On devnet | Transaction | SHA-256 on chain | Bytes | Build |
|---|---|---|---|---:|---|
| [PrincipalRegistry](https://explorer.solana.com/address/9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G?cluster=devnet) | Upgraded in place at 8:55:02 PM ET, slot 523,832,288 | [4M7F3WYa](https://explorer.solana.com/tx/4M7F3WYaY44PPjPTc1p2hHKfFqVUfYpGkXsmNmh2DNdJTZH5ExjXsLyBWVyXv5yGgVaTMkvvmUHzhHgtSm93Xmeh?cluster=devnet) | `f6f4052ceb824561f98718176bb4d32d7851a603a949b750aecf2b9465587659` | 1,098,560 | exact match |
| [ClaimsCore](https://explorer.solana.com/address/727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz?cluster=devnet) | Deployed at 8:55:23 PM ET, slot 523,832,373 | [3EMBiSGj](https://explorer.solana.com/tx/3EMBiSGjbkPUi81SdreKTgC3G9A9sCwAXZsDLyKor9nw6TfdsxYE9t1pjpwvZMoJN4HhSr5DpfinyjCX2GFrknXW?cluster=devnet) | `a96472c1e26f35fcabe9a5af9122c2d807c1048ba8a7a511dc0b2ea5473b69f2` | 3,121,456 | exact match |
| [PoolIssuer](https://explorer.solana.com/address/3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm?cluster=devnet) | Already on devnet, last deployed in slot 523,661,797 | | `6804757b94ce2da8854388a1f748b614296207682db27cde85958f868b726e65` | 334,384 | exact match |
| [Swig](https://explorer.solana.com/address/swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB?cluster=devnet) | Swig's own program on devnet, last deployed in slot 522,994,140 | | `0418f037fe1c9f6c0cd9d3ee57d684f69aca2ed9b3388f5d0c45020b01cce1d9` | 443,904 | exact match with the pinned hash |

The run's driver read each program's ProgramData before sending anything and hashed the program bytes. On devnet the Swig program is a different build from the retained Swig binary the LiteSVM records load (`bb744d98…`), so the driver pins its devnet hash.

## The scenario

Every key is a devnet test key, and the token is a synthetic six-decimal SPL token the run created. A buyer, a credit issuer, a lender and a seller are registered as principals. The buyer has a budget of 500,000 and agent 1 may order at most 100,000 per order from this seller against this lender's facility. The lender's money sits in a Swig wallet bound to its principal, with 99,999 at first and 50,001 added before the order. The facility and the buyer's credit line are 500,000 each. Order 1 is 100,000. The seller earns 60,000 of it, a key with no role collects that, and the seller releases the unused 40,000. Agent 2's permission pins size 10 and a unit price of 1, and its order 2 of 50,000 is earned and collected in full. Amounts are base units of the synthetic token.

The tables list every transaction of the run in the order it was sent. "#" is the position in the record's `cases` array. Each signature links to Solana Explorer on devnet, where the transaction shows its accounts, instructions, program log and result.

## Setup

| # | Time (ET) | What happened | Outcome | Error | Transaction |
|---:|---|---|---|---|---|
| 0 | 8:55:39 PM | Create the synthetic six-decimal token and mint 1,000,000 to the payer | Accepted |  | [56exRZgE](https://explorer.solana.com/tx/56exRZgEHVRTykgrCStN9tuhV5A3ezZYDdCSKRmPRm5ypeXJhZPEqn8WUuSCw7EFdhREjSmCnotw1Mqj9Awh14qw?cluster=devnet) |
| 1 | 8:55:43 PM | Register the buyer as a principal | Accepted |  | [2QrCdwTC](https://explorer.solana.com/tx/2QrCdwTCfbKZWbXEZ1FtRFT4Y3mYi3SMr2QQVC6oxteheoJSUrEYu1SwGQErQCSRsWq4A3cBuxGaXydpEQVXh5aV?cluster=devnet) |
| 2 | 8:55:46 PM | Open the buyer's financial book | Accepted |  | [4rAaSCNk](https://explorer.solana.com/tx/4rAaSCNkRxG1Sq887wD5RGPYvfz5xZDY9QGEoqdjFKwqfttLCsA688eDfsu88xh3HMfdefYWSfpBprE5Vf7FRoxw?cluster=devnet) |
| 3 | 8:55:50 PM | Open the buyer's Pool projection | Accepted |  | [5LjypSfy](https://explorer.solana.com/tx/5LjypSfydAfdggFSucv8xbth5FQgEWrWwbFVCzgywGTRQA3SZnhTfbQbU8BABhGDTEVCr8JTTqqoQppYkCo3wRAB?cluster=devnet) |
| 4 | 8:55:53 PM | Register the credit issuer as a principal | Accepted |  | [2Tr3xiEm](https://explorer.solana.com/tx/2Tr3xiEmE4xfLyudfLp5H8pUCdDyyeZPW9cACXDdN3bpfo1vtAa8zncACvjVw7c6uu6GB6wr5GmN8XmLP21oGLJE?cluster=devnet) |
| 5 | 8:55:56 PM | Open the credit issuer's financial book | Accepted |  | [rvgwCrGG](https://explorer.solana.com/tx/rvgwCrGGuQUzrMv14GhCkCCUPkFSkN6CsJg5uVHpMhwXPQyHk7anRmPUdNzKARo6eUVQMPfUs2x6ysLxuRpFP2L?cluster=devnet) |
| 6 | 8:55:59 PM | Open the credit issuer's Pool projection | Accepted |  | [3fNXQofU](https://explorer.solana.com/tx/3fNXQofUJihFtBPt1gx6ofhwhcS5WTo7YhXpvXNreu9cdYC9jvp4azsSuB1fmQWkLcZwdDPu8ZaTsbzdn62JHbFh?cluster=devnet) |
| 7 | 8:56:02 PM | Register the lender as a principal | Accepted |  | [2wwMfajB](https://explorer.solana.com/tx/2wwMfajB1xQJReAkDqvhut7YjZi8Jez18TDYTs6TAXM3SdvXVWFXhHSbeDPZuoKqsXaArz94KbPii8uLB3nqWx1h?cluster=devnet) |
| 8 | 8:56:05 PM | Open the lender's financial book | Accepted |  | [4Hx2DAeS](https://explorer.solana.com/tx/4Hx2DAeSW5SpVXi6QBYaJajTzaTrYAouUqCBF9pittz7SvTxHiDR9comPz43pjNvoxXH1PG4AHkgq1p6iVwkqh9k?cluster=devnet) |
| 9 | 8:56:08 PM | Open the lender's Pool projection | Accepted |  | [DcK1hg4w](https://explorer.solana.com/tx/DcK1hg4wVdruJwykrEpW7GeFBYvJXYK767FuQtcC5qsQuDL86KpgeKXwZVicELriD8kHJkf2Nzkc5zbbnYw4gEU?cluster=devnet) |
| 10 | 8:56:12 PM | Register the seller as a principal | Accepted |  | [3x3rndrx](https://explorer.solana.com/tx/3x3rndrx4mX1vDqTEc6FtNGexnz16ckTkaJxUZ5Sa5HbGy9uKJfz8WEZCgMkTz2THxthTVKdcRLWXNHKCNHhVzE?cluster=devnet) |
| 11 | 8:56:15 PM | Open the seller's financial book | Accepted |  | [4YoqPV2M](https://explorer.solana.com/tx/4YoqPV2M3kCQMRdS8YsBWGLMpTNaYgQ6pCddtUbRMXkgStL15cCxGKSxBpkb4qo8awinAEUzwMdoxjTF7AzYH7q6?cluster=devnet) |
| 12 | 8:56:19 PM | Open the seller's Pool projection | Accepted |  | [cuWc2tqF](https://explorer.solana.com/tx/cuWc2tqFEYDLExGytCWQ2Heo4jstAPvwhvyLGxWPsmGgNNJxtE4PHeAbJ59HhWKfUYGjsFt1kbC33uYCJftX8as?cluster=devnet) |
| 13 | 8:56:22 PM | Create the seller owner's token account | Accepted |  | [2bhrVBSt](https://explorer.solana.com/tx/2bhrVBSt1wq4uN9Sy3UMfAy1RWomgrqSh59eMp8NvRxTj3TvtuCgnaa8GhZWfQsc7nWhVYqJ6MbMcRBuHMMadFpu?cluster=devnet) |
| 14 | 8:56:25 PM | Create the lender owner's token account | Accepted |  | [4okzuCef](https://explorer.solana.com/tx/4okzuCefRNPJFW4LhzzMVySBQfhn6ZAN6EcqJGW9PjEEMiYEDxqgWsFCVrWsSCftnYxJriKbyYAMQhwVRUMJDRit?cluster=devnet) |
| 15 | 8:56:28 PM | The buyer's owner sets the budget to 500,000 | Accepted |  | [2PyxV6Pg](https://explorer.solana.com/tx/2PyxV6Pgy89DpEeAU48g4NBQFy8tYRJAJRdJX7vTU1twRPrME91NLQqWDCoGUktfWMFFwY6nvjuzeMwLxGWXmihG?cluster=devnet) |
| 16 | 8:56:31 PM | The buyer's owner enrolls agent 1 | Accepted |  | [4KBh9AMJ](https://explorer.solana.com/tx/4KBh9AMJm6PqX8qU5hKJtcBo4knZFgm7QGkiabq6muTm4nNR1BP5CJCNv5AWpPgDa38EYbAUz36Q5i28ikCuKWWx?cluster=devnet) |
| 17 | 8:56:34 PM | Create the credit issuer wallet's token account | Accepted |  | [3UnSs2Fj](https://explorer.solana.com/tx/3UnSs2FjVU9UMybL7n7hTDy8TtCgkPmfryn5FReSfojcgeqTQFjbtz4E8JxMVwCiviqCt6cba7pPin4U2bjpBjiU?cluster=devnet) |
| 18 | 8:56:38 PM | The credit issuer's owner opens the cash record for its wallet, with 0 allocated | Accepted |  | [5r78AZY1](https://explorer.solana.com/tx/5r78AZY1UZxPdsUwWVp8bYQiMURTXBs2wcE6AJt7qPkfHAsBvCKucB9Hzidq3JSmmzEUGPQeb5eMiQvz6p8Vc8FE?cluster=devnet) |
| 19 | 8:56:41 PM | Create the lender's Swig wallet and bind it to the lender's principal | Accepted |  | [5gCWnweQ](https://explorer.solana.com/tx/5gCWnweQyy7WGce9M7z9aTEZQfauN2bVrmL5PJwvXciNocvCDU4HHPMuGqqDeZ4HGJATG1hjauwxSZC83DSxEanX?cluster=devnet) |
| 20 | 8:56:44 PM | Create the lender wallet's token account | Accepted |  | [5Z9nSUdT](https://explorer.solana.com/tx/5Z9nSUdTcctt9ks1UzhZJ2ydo22MjBLaPyK2NtKJWk3uRWWVrtpk8jkrp8jDdCKhx9nHp71GAGmHWmkdzdDnG3ep?cluster=devnet) |
| 21 | 8:56:48 PM | Put 99,999 tokens in the lender's wallet | Accepted |  | [5Ua9eGxt](https://explorer.solana.com/tx/5Ua9eGxt8YHSDk79y2WEhzt6afJxkM1ypiMHUj3mfcxTpXcaCdLtR2gHHZHbnuHxQK38iu7an1g2XFUdrLkRF5fF?cluster=devnet) |
| 22 | 8:56:52 PM | The lender's owner opens the cash record for its wallet, with 99,999 allocated | Accepted |  | [2hPWcvnv](https://explorer.solana.com/tx/2hPWcvnv6vfeCU8K5TggFywiGLmkSB6fY9SLiQ7uUS2b1LZGj77B7zRCEwnwkkm3KNSGMcod3HjHyqN73T151DCW?cluster=devnet) |
| 23 | 8:56:55 PM | The lender's owner and the credit issuer's owner open the lender's facility of 500,000 | Accepted |  | [eHmZbxUH](https://explorer.solana.com/tx/eHmZbxUH3UoStki2p1WcKoxToFx6TByoLWS7pdqXsCHi5gJUj82U5PprK5gdmpRMUacASQ6v2g5y8qmpU1wYa9Z?cluster=devnet) |
| 24 | 8:56:58 PM | The buyer's owner and the credit issuer's owner open the buyer's credit line of 500,000 | Accepted |  | [EUYXWTea](https://explorer.solana.com/tx/EUYXWTeaZZiuHen3e7tyWNso56BxPEQUHENjsGGXzsqq1JKbVks7aThDX1dhrpf2dHi8nUxNfWFkBnde5VN2EfR?cluster=devnet) |
| 25 | 8:57:01 PM | The buyer's owner permits agent 1: this seller, this lender facility, at most 100,000 per order | Accepted |  | [KXYx9mZh](https://explorer.solana.com/tx/KXYx9mZh6NnAwjn3qeP5TeF7igxpHL7wr88oVyMgZNpESELgiFDYYSt5kWDfsX2fhpJiNDjE427fnZAyQWDgpfX?cluster=devnet) |
| 26 | 8:57:05 PM | Give the lender's owner 50,001 tokens to add later | Accepted |  | [3DheQu6E](https://explorer.solana.com/tx/3DheQu6EVp1RCzJLCvfZdkUriL83JrxfwDc7RpAXa2zsXFRQNEn5NnDMvo4KM66wFBLHKxgMvvGBwCbFAu2pHPVG?cluster=devnet) |

## The commit

| # | Time (ET) | What happened | Outcome | Error | Transaction |
|---:|---|---|---|---|---|
| 27 | 8:57:10 PM | Order of 100,000 when the lender's free cash is 99,999 | Refused | `FundingCashShortfall` 6033, ClaimsCore | [3xEZF2Z6](https://explorer.solana.com/tx/3xEZF2Z6fDKoxW3Z1795k9GiEt9eMqFiUCEPJQQEQDSzXfXzggNcrKWLAYDXGoQVwkh7UFWMrf6cVGLi9RHVvMmw?cluster=devnet) |
| 28 | 8:57:13 PM | The lender adds 50,001 to its wallet, and its free cash becomes 150,000 | Accepted |  | [3pKFHNmY](https://explorer.solana.com/tx/3pKFHNmY4uHnyP2TQw959Su8HEpW5EEVKPG4yNsAfxWQXbSMkx6XenTfcxxRsoKCwbRWKGUGkGvzhXn2qXy1RPCq?cluster=devnet) |
| 29 | 8:57:16 PM | Order of 100,001 when agent 1 may order at most 100,000 | Refused | `PerOperationLimit` 6011, PrincipalRegistry | [2oYJhw2i](https://explorer.solana.com/tx/2oYJhw2iq8GLuJdN5115j3xL28KNqoAdgandTHp7WPw5aom3LE44HNzsCkcFuUnYoZZiaFyTiwoh7jCGLz6jqidY?cluster=devnet) |
| 30 | 8:57:20 PM | Order sent at a unit price of 2 when the agreed terms commit to a unit price of 1 | Refused | `FundingUsage` 6044, ClaimsCore | [5ckLUXgn](https://explorer.solana.com/tx/5ckLUXgndW5zcyPVpRAMgGMUSqyRKSSmVSw8etbyyWRdqjqzsSMkRTjaajEB58vYbBUHcnTU9Q9jSBgNbnEeJqyy?cluster=devnet) |
| 31 | 8:57:22 PM | The commit accepts the order of 100,000: within agent 1's 100,000 per order and the 500,000 budget, with the lender's free cash at 150,000, signed by the seller and the credit issuer's owner | Accepted |  | [93jWsEiX](https://explorer.solana.com/tx/93jWsEiXuWzjwdKz3v8jhFFKD3w5EtMynSYYgcHcT25WXBdciU1xuoSFzG1zjUFKJp3k1ZWtqVSX5uocxrTNkKU?cluster=devnet) |

In row 29 PrincipalRegistry refuses the order inside ClaimsCore's commit, so the Registry fails first and ClaimsCore fails with the same code. Rows 27, 29 and 30 change one fact each from the order row 31 accepts: the lender's free cash, the amount and the unit price.

## Earning and collection

| # | Time (ET) | What happened | Outcome | Error | Transaction |
|---:|---|---|---|---|---|
| 32 | 8:57:29 PM | The seller reports 60,000 of completed work on the order | Accepted |  | [2jx2B4Jx](https://explorer.solana.com/tx/2jx2B4JxEUrSVtaP95e17LNGvojKmASN6K9HPvA3Sec87FRXV5ujrXNKggu7WNgqgACFSGfzkxpbEDbH48PFXyW3?cluster=devnet) |
| 33 | 8:57:35 PM | The payer gives the collecting key lamports for one receipt and its fees | Accepted |  | [2W6BTyG5](https://explorer.solana.com/tx/2W6BTyG5y1BVJdG63wikhVAuE6ZKsTPnauK4kh5s8ZVhEijRHoHXswABQgCcEch66yDQw4BJzkjLoqTG2JZTAajW?cluster=devnet) |
| 34 | 8:57:38 PM | Create the collecting key's own token account | Accepted |  | [371M9Eyv](https://explorer.solana.com/tx/371M9Eyvuztg2RmsiEauwdgzjCbuoiq8B81muswTsSjN4rrUBRWiK9kZL1AgAt99wAwuuq9dTYX62JtGgRWxjZq?cluster=devnet) |
| 35 | 8:57:43 PM | Collection with the payee swapped to the collecting key's own token account | Refused | `FundingTokenRouting` 6035, ClaimsCore | [Yfxm4hw4](https://explorer.solana.com/tx/Yfxm4hw4EKmauBVpn4TTFxSKnosa8FzQY9bgQ6JrFM1qZk5eKLEbLHt33HnJrDk4LUxqM9f9ATmnBqmX63czPBD?cluster=devnet) |
| 36 | 8:57:47 PM | Collection with an extra instruction after the wallet's SignV2 | Refused | `WalletEnvelope` 6023, PrincipalRegistry | [z3PH6TuW](https://explorer.solana.com/tx/z3PH6TuWwCwRuYb4CZBtYpy5Pmb1Gw9YZkBd9JRcTL2XmNB7UaNMe6WP4CbAAyRtu6JorGoK4NAywLeS7GGRvaC?cluster=devnet) |
| 37 | 8:57:50 PM | A key with no role in the order collects the seller's earned 60,000 from the lender's wallet | Paid |  | [3s1xwqy5](https://explorer.solana.com/tx/3s1xwqy5CxH3teV3pASTkNqLyC4d4g4uSSJsrmmRGBvjbvGKDE2RFrTaJpZBPEwJcnBPnGG8hDEXACgQjTNTDd9G?cluster=devnet) |

Rows 35 to 37 are signed by the collecting key alone, `FN9dHbLQzZJWSG1VfEZHa5Xn2Aofwxtjd8DcvcBU7jS2`, which holds no role in the order and pays its own fee. In row 35 ClaimsCore refuses inside the Swig wallet's instruction, so ClaimsCore fails first and Swig fails with the same code. In row 37 the seller received exactly 60,000 and the lender's wallet paid exactly 60,000.

## Withdrawal and release

| # | Time (ET) | What happened | Outcome | Error | Transaction |
|---:|---|---|---|---|---|
| 38 | 8:57:53 PM | The payer gives the lender's owner lamports for one receipt | Accepted |  | [3koba6nd](https://explorer.solana.com/tx/3koba6ndXroUCwrpHeDgF7ULvoHGTeXNG13Rg5XnGXnHArhyH1wFT1ihBytmCKhdTfs6NdTFahasdxE8BnAYEpaY?cluster=devnet) |
| 39 | 8:57:58 PM | The lender asks to withdraw 50,001 when 50,000 is free and 40,000 is promised | Refused | `FundingCashShortfall` 6033, ClaimsCore | [5BAeD8XH](https://explorer.solana.com/tx/5BAeD8XHArCFWsTJbJDRDTzKQj8wmDHr6DY7dM3RrKBGBVY8HR2ZCh1C6LAMPWLPpxQpBPeE8HnLMqZqb3yqtyjm?cluster=devnet) |
| 40 | 8:58:00 PM | The seller releases the order's unused 40,000 | Accepted |  | [3ruDFp45](https://explorer.solana.com/tx/3ruDFp45Ldh4q5eyQ4Kj56RV13kAmuBkRhCu4TDAYVnwtSnbrLCVyAoLBujumVzWJh9U4A99kWVFn4AkF5Bua5pq?cluster=devnet) |

In row 39 ClaimsCore refuses inside the Swig wallet's instruction. After row 40 the lender's free cash is 90,000 and nothing is promised.

## Terms and price the owner pins

| # | Time (ET) | What happened | Outcome | Error | Transaction |
|---:|---|---|---|---|---|
| 41 | 8:58:06 PM | The buyer's owner enrolls agent 2 | Accepted |  | [5hwStrA6](https://explorer.solana.com/tx/5hwStrA6rqGnd7SiNaz4iQXcD6ooqTwH8Ley1egcjyAiNSFs3no8Z7JmTijFoER3mhnSiFeVuWK5pp5xa3eLZdH7?cluster=devnet) |
| 42 | 8:58:09 PM | The buyer's owner creates agent 2's permission already pinned to size 10 and a unit price of 1, in one instruction (`create_pinned_mandate`): this seller, this lender facility, at most 100,000 per order | Accepted |  | [3J2bS4zd](https://explorer.solana.com/tx/3J2bS4zdhmuz8XBuVTZEEcEE3qSByjW8qgvsQs3qRjiJL7TSkaXCWvbW95gkqj62KybJWBrNseboJomt4cXHrsUy?cluster=devnet) |
| 43 | 8:58:13 PM | Agent 2's order of 50,000 says size 11 where the owner pinned size 10, sent through `accept_pinned_financed_tab` | Refused | `TermsNotPermitted` 6053, PrincipalRegistry | [4BeAsGWm](https://explorer.solana.com/tx/4BeAsGWmCvnRih2UfrGNU7qSNyTXgy5R4J2tmV6JvdEbcRgatwDD2Xu6QGznjZaSxMMdXnpyJ7y3tQppxHRkPx8P?cluster=devnet) |
| 44 | 8:58:17 PM | The same size 11 order sent through plain `accept_financed_tab`, which shows no terms | Refused | `TermsRequired` 6051, PrincipalRegistry | [3SCDCKAX](https://explorer.solana.com/tx/3SCDCKAXfPSErrsv2y7r39ZQQNFEj6FKLcWn8Jk7aWqKeuAiPoeKBnierWozRV3iWeeNy4CsScocWx5fpuPDegfk?cluster=devnet) |
| 45 | 8:58:20 PM | Agent 2's order of 50,000 for size 10 at 2 per unit, where the owner pinned 1 per unit | Refused | `TermsNotPermitted` 6053, PrincipalRegistry | [HiqPXnMS](https://explorer.solana.com/tx/HiqPXnMSVRHc9K3Q8sk5J1Y7C3aQfdJWEQFoBck2qo7CjQictc9vP9fCC3tF5kw8TJMHHBeC2Q1qcAY4eeFR4AL?cluster=devnet) |
| 46 | 8:58:23 PM | Agent 2's order of 50,000 for size 10 at 1 per unit, the pinned values, sent through `accept_pinned_financed_tab` | Accepted |  | [5RdMRwxN](https://explorer.solana.com/tx/5RdMRwxNPXVJ1U5T2V9R3G2s7k6UgEguPRpd6DS8Mcnan11JrXSYw1j2kuWkgYihQWTswf1pdTyc67yaTV4kn4Xx?cluster=devnet) |
| 47 | 8:58:27 PM | The seller reports all 50,000 of order 2 as completed work | Accepted |  | [34VuZuvX](https://explorer.solana.com/tx/34VuZuvXhq36HBEyJYUnwrN6pJtCRH6QsQWeUBw8JVKRdVBSbwCQRZLMQijghVeEVwTq92ZMfYM9ZxwKE5kdcQfQ?cluster=devnet) |
| 48 | 8:58:31 PM | The payer gives the collecting key lamports for order 2's collection receipt and its fee | Accepted |  | [5DHKA2er](https://explorer.solana.com/tx/5DHKA2eroj7rvQWx6myRwepvoUYJ1SKS2JmDQusb81XuwSZ3DNa2DRfSA5VTAgEeBXDy4tPgjidtV91Qyrf8cMwJ?cluster=devnet) |
| 49 | 8:58:35 PM | A key with no role in the order collects the seller's earned 50,000 on order 2 from the lender's wallet | Paid |  | [5TYa7mNd](https://explorer.solana.com/tx/5TYa7mNdmSr7mXNCvpzDXWDCMH5XKsBcqmBtx3XeYxiQDAdSJaornnk5wZ1Hi6q2sgBambfxnXF9ChsRYAQBG3MW?cluster=devnet) |

In rows 43 to 45 PrincipalRegistry refuses the order inside ClaimsCore's commit, so the Registry fails first and ClaimsCore fails with the same code. Row 46 is the order the owner's pins allow. Row 49 is signed by the collecting key alone, and the seller received exactly 50,000.

## Returning lamports

| # | Time (ET) | What happened | Outcome | Error | Transaction |
|---:|---|---|---|---|---|
| 50 | 8:58:39 PM | The collecting key returns its remaining lamports to the payer | Accepted |  | [4MvBPsCf](https://explorer.solana.com/tx/4MvBPsCfsRKggYDxKxjTo9vWj7Z46KrHKRdaDAMo4MKAaqAx7XUW65t4JCJaRiZ1Wi7Xeu17YdL89WM1kv6ofkUv?cluster=devnet) |
| 51 | 8:58:42 PM | The lender's owner returns its lamports to the payer | Accepted |  | [4LsYY1n4](https://explorer.solana.com/tx/4LsYY1n4Y85u7qXcHpSd3Tqu2JQ7wuWaiEUL2Aw7jbP1qUBtcJhTXKCtkgKtGVCo3JqqoXacpkDj1QfvzyPdk9pC?cluster=devnet) |

## After the run

| Figure | Value |
|---|---:|
| Seller's tokens | 110,000 |
| Lender's wallet and cash on record | 40,000 |
| Lender's promised cash | 0 |
| Order 1: paid, buyer's debt, released | 60,000, 60,000, 40,000 |
| Order 2: paid, buyer's debt | 50,000, 50,000 |
| Budget used | 110,000 |

The record's 255 assertions, these figures among them, compare what the driver expected with the accounts it read from the chain, and all passed.

## Where it ran

The cluster is Solana devnet, genesis hash `EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG`, node version 4.4.1. The run sent native v1 transactions through a devnet JSON-RPC endpoint, with a compute-unit cap of 1,400,000 and no priority fee. It waited for each transaction to be confirmed, then read every signature back once finalized, and each read matched. It started at 8:55:37 PM ET and ended at 8:59:16 PM ET, 224.4 seconds over slots 523,832,432 to 523,833,351, with 395 HTTP requests and no rate-limited response.

The payer, [`FpD1YKCetiW6JamQNQRNRbASCFdR2iZX4BPAmea3MmKB`](https://explorer.solana.com/address/FpD1YKCetiW6JamQNQRNRbASCFdR2iZX4BPAmea3MmKB?cluster=devnet), spent 367,987,200 lamports: 500,000 in transaction fees and 367,487,200 in account deposits. It is also the upgrade authority of the three Account programs on devnet. The Swig program's upgrade authority is `yRtJa2jbNRv5KR7qKTi4ZFQ1GGjbkyPHra5C7G4NtPb`. The largest transaction is 2,623 bytes, rows 43, 45 and 46, and the most accounts in one transaction is 37.

The programs were built from Dexter Account main at `5df2ff19efaaaad2936a769138f54afcd05b8cde`, whose program source is the [early-settlement record's](../early-settlement/README.md#where-it-ran). The driver is `foundation/onchain/tests/public_rpc_sequence.py` at commit `7de2fcebaadf3e2e341ddff833f8b5d8c1de5aad`, the merge of its pull request into Dexter Account main. The record gives the SHA-256 of the driver and of the five test files it imports.

When this record was published, all 54 signatures, the two deploy transactions and the 52 of the run, read back from `https://api.devnet.solana.com` with no key. Each of the 52 run transactions had the slot, time, result, fee payer, fee and compute units the record gives, and both deploy transactions succeeded at the slots each program reports as last deployed. The four programs, dumped from the same endpoint, hash to the values above.

## Public keys

| Role | Key |
|---|---|
| Payer | `FpD1YKCetiW6JamQNQRNRbASCFdR2iZX4BPAmea3MmKB` |
| Buyer's owner | `HEnFPytxFj5mDCg8XPeKVNcAXTJ5z9WgFNHd5JcLNY7S` |
| Agent 1 | `391Y4KNfikbPstF8XnVffLdpZd6uuCK29okHN9mz964f` |
| Agent 2 | `HWPYi7gp6KgSHngTNfThsfGNGGiqXXvwWsrgBzhSpDWu` |
| Seller's owner | `6AN4Wjiy1xVBqF8Gg27H2CJjfz7bURwVMTbb1bLjmk5C` |
| Credit issuer's owner | `HtFwAScCmogKyKrCkNqh4KyNjwHnoNVpLcNxYiEHrWNy` |
| Lender's owner | `14mq5fuUQzgDtgc4camhJXQRFLYnRie2gjp7EbDb2D9` |
| Collecting key | `FN9dHbLQzZJWSG1VfEZHa5Xn2Aofwxtjd8DcvcBU7jS2` |
| Token mint | `G4DRXGeqi7wvsABGHSm24HeVXUpGM7zcr2ckZCyXLGGY` |

The record's `run.addresses` names 23 accounts of the run, from the buyer's budget to the order records.

## Files

| File | Contents |
|---|---|
| [devnet-record.json](devnet-record.json) | The run record: the cluster, the four programs and their on-chain hashes, the scenario, and every transaction with its signature, slot, time, result, compute units, fee, signers, program log, explorer link and the figures read before and after, with the 255 assertions, the payer's costs and the final figures. |
| [devnet.log](devnet.log) | What the deploy and the run printed: both deploy signatures, the on-chain checks and the run's summary. |
| [public_rpc_sequence.py](public_rpc_sequence.py) | The driver that sent the run, byte-identical at commit `7de2fce`. It imports the Account repository's `public_rpc_transport.py`, `financing_fixture.py`, `financing_support.py` and `pinned_terms.py`, and the pinned-price record's [pinned_price.py](../pinned-price/pinned_price.py), which is byte-identical. |
| [provenance.json](provenance.json) | The source of each file, its source and published hash, and what was omitted. |
| [SHA256SUMS](SHA256SUMS) | Hashes of the four files above. |

The run's salt file, from which every key the run created derives, is not published, nor is the journal the driver keeps beside it. From the record's `run` section three fields are omitted: the local paths of the salt file and the journal, and the salt's SHA-256. In the record `build.artifact_root` reads `<fixture-workspace>`, and in the log the record's path reads `<run-directory>/devnet-record.json`. Nothing else changed. The published record holds no secret key, seed or salt: every 32-byte value in it was checked as an Ed25519 seed against the public keys it names, and every 64-byte value as a keypair.

## Check it yourself

Read any transaction from the public devnet endpoint. This reads row 31, the commit:

```sh
curl -s https://api.devnet.solana.com -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"getTransaction","params":["93jWsEiXuWzjwdKz3v8jhFFKD3w5EtMynSYYgcHcT25WXBdciU1xuoSFzG1zjUFKJp3k1ZWtqVSX5uocxrTNkKU",{"encoding":"json","maxSupportedTransactionVersion":1}]}' \
  | python3 -c "import json,sys; r=json.load(sys.stdin)['result']; print(r['slot'], r['meta']['err']); print('\n'.join(r['meta']['logMessages'][:2]))"
```

```text
523832873 None
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz invoke [1]
Program log: Instruction: AcceptFinancedTab
```

With row 27's signature, `3xEZF2Z6fDKoxW3Z1795k9GiEt9eMqFiUCEPJQQEQDSzXfXzggNcrKWLAYDXGoQVwkh7UFWMrf6cVGLi9RHVvMmw`, it prints slot 523832821 and `{'InstructionError': [0, {'Custom': 6033}]}`. To check a program against its hash, dump it with the Solana CLI and hash the file:

```sh
solana program dump 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz claims_core.so --url https://api.devnet.solana.com
sha256sum claims_core.so
```

The second command prints `a96472c1e26f35fcabe9a5af9122c2d807c1048ba8a7a511dc0b2ea5473b69f2`. To check the files against their hashes, run `sha256sum -c SHA256SUMS` in this folder.

The [November 18 commit record](../README.md), the [pinned-terms record](../pinned-terms/README.md) and the [pinned-price record](../pinned-price/README.md) hold the LiteSVM runs of the same checks, with every refusal by row. The [claim guide](../../../articles/the-account-inversion/README.md) maps the article to its records.
