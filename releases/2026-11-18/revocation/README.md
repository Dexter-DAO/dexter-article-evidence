# Revoking an agent stops its next order and nothing else

Revoking an agent stops its next order and nothing else. On November 18, 2026, on the compiled Account programs, an agent placed three orders of 100,000 each and all three were accepted. The buyer's owner then revoked the agent, and the agent's fourth order on the same terms was refused while the budget, the facility, the credit line and the lender's cash each still covered it. After the revocation the seller reported 60,000 and 100,000 of work on the first two orders, and a key with no role in the orders collected exactly 160,000 to the seller. The owner then enrolled a new agent, which placed a fifth order.

After an order is accepted, the agent's key appears in no check on earning, collection or release. The trace asserts it, and the source shows it: the account lists of `EarnFinancedRun`, `CollectFinancier` and `ReleaseFinancierUnused` in [funding.rs](../source/foundation/onchain/programs/claims-core/src/funding.rs) hold no agent, agent-key or permission account, and neither do the two Registry calls those instructions make.

The programs ran on a local Solana virtual machine with a synthetic token. The run has 72 signed transactions, and all 271 recorded assertions passed. After each of the 3 refusals, every account the transaction could write, other than the fee payer, held the same bytes as before. [case-index.json](case-index.json) names every transaction, and [revocation-trace.json.xz](revocation-trace.json.xz) holds each one's signed bytes, program log and account state.

## The scenario

One buyer, one agent, one seller, one credit issuer and one lender. Every order has a maximum of 100,000, with no interest and no seller fee. The agent's per-order limit is 100,000, the buyer's budget is 500,000, the facility and the credit line are 500,000 each, and the lender's wallet starts with 1,000,000. Amounts are base units of the synthetic token.

In the tables below, "Row" is the zero-based position in the trace's `transactions` array and equals `index` in the case index. Rows 0 to 54 are setup. "Error" is the name and number in the program log, followed by the program that raised it.

## In order

| Row | What happened | Outcome | Error |
|---:|---|---|---|
| 55 | The agent places order 1, maximum 100,000, with the seller | Accepted | |
| 56 | The agent places order 2, maximum 100,000, with the seller | Accepted | |
| 57 | The agent places order 3, maximum 100,000, with the seller | Accepted | |
| 58 | The buyer's owner revokes the agent | Accepted | |
| 59 | The revoked agent signs a fourth order of 100,000 with the seller on the same terms | Refused | `CredentialInactive` 6004, PrincipalRegistry |
| 60 | The revoked agent sends accepted order 1 again | No effect | |
| 62 | The seller reports 60,000 of completed work on order 1 | Accepted | |
| 63 | The seller reports 100,000 of completed work on order 2 | Accepted | |
| 64 | A key with no role collects order 1's 60,000 from the lender's wallet to the seller | Paid | |
| 65 | A key with no role collects order 2's 100,000 from the lender's wallet to the seller | Paid | |
| 68 | The buyer's owner enrolls a new agent with a new key | Accepted | |
| 69 | The buyer's owner gives the new agent the same permission: this seller, this lender's facility, 100,000 per order | Accepted | |
| 70 | The new agent places a fifth order, maximum 100,000, with the seller | Accepted | |

Row 60 is not a refusal. The program recognises the order it already recorded, returns without calling the Registry and changes no account. It creates no new hold and no new promise.

What the trace records around these rows:

- With three orders accepted, the hold on the buyer's budget is 300,000 and the lender's promised total is 300,000. No tokens have left the lender's wallet.
- The revocation in row 58 leaves the three order records, the budget hold and the lender's promise byte-identical. The agent's record shows it inactive at generation 2.
- Before row 59 the remaining budget, facility limit, credit line and lender's free cash each cover one more order of 100,000. After the refusal the hold and the promise are still 300,000.
- Each work report in rows 62 and 63 is signed by the seller and the fee payer. Each collection in rows 64 and 65 carries one signature, the collecting key's. The revoked agent's key is not among the accounts of any of the four.
- The seller received exactly 160,000 and the lender's wallet paid exactly 160,000.
- At the end the buyer's debt on the three orders is 160,000, owed to the credit issuer. Each order record still carries the terms the revoked agent signed, and each acceptance receipt is byte-identical to when it was accepted. The fifth order's record names the new agent.

## Releasing unused backing

The other two refusals are about who may release an order's unused backing. They do not depend on the revocation.

| Row | Release | Sent by | Outcome | Error |
|---:|---|---|---|---|
| 61 | Order 3's unused 100,000, before the order expires | The revoked agent, signing in the seller's place | Refused | `FundingReleaseAuthority` 6043, ClaimsCore |
| 66 | Order 3's unused 100,000, before the order expires | A key with no role, without the seller's signature | Refused | `FundingReleaseAuthority` 6043, ClaimsCore |
| 67 | Order 1's unused 40,000, before the order expires | The seller | Accepted | |
| 71 | Order 3's unused 100,000, one second after the order expired | A key with no role, with no other signature | Accepted | |

Before an order expires only the seller can give up its unused amount, and after expiry anyone can send the release. The check is that the signer is the seller, so rows 61 and 66 would be refused the same way for an agent that had not been revoked.

## Keys the trace names

| Role | Key |
|---|---|
| The revoked agent's key | `Cdkrk8tujFY6mTyGwFgKpnbiGc1hqtXCog1qvUdKAe6D` |
| Seller | `4MfyR4G3NWfVRDWo6iNAHDBZqWMgwZX6FNtMqEW3a9JT` |
| Fee payer of setup, order and work-report transactions | `AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9` |
| Key that collects, and releases in rows 66 and 71 | `6jQ7iw7szEgN6hp49SAAMagYSYdDAZTQRDF3b8vTLNqM` |

## The rule in the source

The fourth order is refused in the Registry's `reserve`, at line 257 of [lib.rs](../source/foundation/onchain/programs/principal-registry/src/lib.rs): `require!(actor.active, RegistryError::CredentialInactive);`. The owner's `revoke_actor` is at line 209 of the same file. The account lists that hold no agent account are `EarnFinancedRun`, line 171, `CollectFinancier`, line 205, and `ReleaseFinancierUnused`, line 294, of [funding.rs](../source/foundation/onchain/programs/claims-core/src/funding.rs). The release check is at lines 210 to 212 of [funding_servicing.rs](../source/foundation/onchain/programs/claims-core/src/funding_servicing.rs).

## Where it ran

The compiled programs ran in LiteSVM from solders 0.29.0 under Python 3.12.3, with signature verification on and native v1 transactions. The token is one synthetic six-decimal SPL token and every key is a disposable test key. The run started on November 18, 2026 at 10:38 AM US Eastern, and the test set the clock once, to 7,201 seconds after the start, before row 71. It sent no transaction to a public network.

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
| [revocation-trace.json.xz](revocation-trace.json.xz) | All 72 transactions: signed bytes, result, program log, and the observed accounts before and after. About 7 MB uncompressed. |
| [case-index.json](case-index.json) | One entry per transaction: name, outcome, error name and number, the program that raised it and the source line its log names. |
| [revocation-receipt.json](revocation-receipt.json) | Counts, the trace hashes and the binary hashes. |
| [revocation.log](revocation.log) | The 2 lines the run printed. |
| [revocation.py](revocation.py) | The test module. It reuses the scenario and the refusal checks in the commit record's [commit_legs.py](../commit_legs.py). |
| [provenance.json](provenance.json) | The source commit and the source and published hash of each file. |
| [SHA256SUMS](SHA256SUMS) | Hashes of the six files above. |

The trace, case index, receipt and test module are byte-identical to the source commit. In the log, one absolute workspace path is replaced with `<fixture-workspace>`. The trace's `fixture_inputs` lists the hash of every test file the run used.

## Check a row yourself

Download the trace, then print any row's name, result, state comparison and program log. This reads row 59, the revoked agent's fourth order:

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
" revocation-trace.json.xz 59
```

```text
refused: the revoked agent signs a fourth order of 100,000 with the seller on the same terms
expected: reject | error: {'InstructionError': [0, {'Custom': 6004}]}
state unchanged: True
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz invoke [1]
Program log: Instruction: AcceptFinancedTab
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G invoke [2]
Program log: Instruction: Reserve
Program log: AnchorError thrown in programs/principal-registry/src/lib.rs:257. Error Code: CredentialInactive. Error Number: 6004. Error Message: Credential is inactive.
Program 9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G failed: custom program error: 0x1774
Program 727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz failed: custom program error: 0x1774
```

The lines left out here are system program calls and compute-unit counts. `transaction_base64` in the same row is the signed transaction, and `observed_before` and `observed_after` hold each observed account's owner, lamports and data.

To check the files against their hashes, and the decompressed trace against `trace_sha256` in the receipt:

```sh
sha256sum -c SHA256SUMS
xz -dc revocation-trace.json.xz | sha256sum
```

The second command prints `e67b8495b36eccd072d0b91c4f27c197466cb41e40407d32b0fbae7fade18975`.

The [commit record](../README.md) covers the three checks on a single order. The [claim guide](../../../articles/the-account-inversion/README.md) maps the article to its records.
