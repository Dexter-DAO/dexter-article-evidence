# Current verifier for the October 30 mainnet record

This folder holds the current verifier for the [October 30, 2026 mainnet record](../../../evidence/historical-evidence/record.json). It rereads the record's transactions from a Solana mainnet RPC and checks the Internet Archive's copy of the seller's store file against the hash the passkey approval committed to. The record is unchanged.

[verify-case-three/](verify-case-three) is the `examples/verify-case-three` folder of [Dexter-DAO/dexter-x402-sdk](https://github.com/Dexter-DAO/dexter-x402-sdk/tree/ed789c3725c0059614a69d8413f64ecaf198701b/examples/verify-case-three) at commit `ed789c3725c0059614a69d8413f64ecaf198701b`, copied byte for byte. The [copy inside the November 4 package](../../../evidence/historical-evidence/verifier) is the same folder at commit `43980c8daaa5e3067681b9e3738b7bbb968ea200`. That package is kept as approved, so its copy stays as it was.

## What changed

Four of the nine files differ between the two commits: `verify.mjs`, `archive.mjs`, `verify.test.mjs` and `README.md`. All four changes concern one check, the archive check.

The record links to an Internet Archive capture of `https://dexter.cash/.well-known/did.json` timestamped 02:55:33 UTC on October 30, 2026. The Internet Archive answers that link with a redirect to its capture from 06:28:57 UTC the same day, 80 seconds after the passkey approval at 06:27:37 UTC. On November 18, 2026, its index listed that capture and no other for this file.

The November 4 verifier refused every redirect, so its archive line reads:

```text
FAIL archive: The archive URL is not an exact capture
```

This verifier follows one redirect, and only to another raw capture of the same store file at `web.archive.org`. It then applies the earlier verifier's two tests to the capture the archive served:

- Its timestamp must fall from 24 hours before the approval through 30 minutes after it.
- Its bytes must hash to the value in the seller binding that the passkey approval committed to.

When the served capture is not the one the link names, the line prints both times. A second redirect, a redirect to any other address, a capture outside the window and a capture with different bytes each fail the check.

## The run

On November 18, 2026 at 9:17 AM US Eastern, this verifier checked the unchanged record (SHA-256 `9c2052a9e07706d1596d819521615c187b76fc215087cc37cd13b1c17cbd152a`) against Solana mainnet under Node v24.22.1. It printed 54 PASS lines and no FAIL line, and exited with status 0. [mainnet-run.txt](mainnet-run.txt) holds the 55 lines it printed, and [mainnet-run.json](mainnet-run.json) records the command, the time and the counts. The RPC address carried an access key and is not retained.

The archive line:

```text
PASS archive: https://web.archive.org/web/20261030062857id_/https://dexter.cash/.well-known/did.json sha256 eaa77c5e24fb7356749890577a60887cd5464c2b8d2f0750e592cea3c4e83c03, captured 1 min after the approval; the record links to 2026-10-30T02:55:33.000Z and the archive answers that link with this capture from 2026-10-30T06:28:57.000Z
```

The line rounds the 80 seconds to `1 min`. The November 4 copy, run the same morning against the same record, printed the same 53 other PASS lines and the FAIL line above, and exited with status 1. The 55th line of both runs, `LIVE store_file: still_listed`, is a read of the store file as served during the run. It does not affect the exit status.

The test suite passed the same morning: 82 tests, 0 failures.

## Run it

Use Node 24 or newer, from a clone of this repository:

```sh
cd releases/2026-11-18/verifier/verify-case-three
npm ci --ignore-scripts --no-audit --no-fund
npm test
node verify.mjs ../../../../evidence/historical-evidence/record.json --rpc https://YOUR-ARCHIVAL-SOLANA-RPC
```

`npm test` runs locally after the install. The last command is read-only and submits no transaction. It needs an RPC that retains the October 30 transactions, and it reads the capture from `web.archive.org` and a screenshot and the store file from `dexter.cash`. Each line prints `PASS` or `FAIL` with its evidence, and the exit status is zero only when no line fails. The SDK's own [README](verify-case-three/README.md) describes each check.

## Files

| File | Contents |
|---|---|
| [verify-case-three/](verify-case-three) | The nine files of the SDK folder: the verifier, its archive helper, its tests and fixtures, the package files and the SDK's README. |
| [provenance.json](provenance.json) | The SDK commit and its tree, and the Git blob hash, size and SHA-256 of each of the nine files. |
| [mainnet-run.txt](mainnet-run.txt) | The 55 lines the November 18 run printed. |
| [mainnet-run.json](mainnet-run.json) | The command, time, Node version, record hash, counts and exit status of that run. |
| [SHA256SUMS](SHA256SUMS) | Hashes of the twelve files above. |

To check the files against their hashes, and one file against the SDK commit:

```sh
sha256sum -c SHA256SUMS
git hash-object verify-case-three/verify.mjs
```

The second command prints `54b821d6fa51268b6a3d8b4869085f3e82055791`, the blob hash of `verify.mjs` in the SDK commit.

The [record excerpts](../../../excerpts/records.md#check-the-oct-30-record) give the offline checks of the same record.
