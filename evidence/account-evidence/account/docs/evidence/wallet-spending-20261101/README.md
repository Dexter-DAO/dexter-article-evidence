# Governed wallet spending: local execution evidence

## Result and source

The current send and exact-input swap program paths passed focused signed native-v1
execution through actual compiled Registry, Core, Pool, retained Swig and SPL Token Swap.
The three new scenarios contain **300 transactions, 34 expected refusals, 767 assertions,
48 financial receipts and 12 committed wallet operations**. Counts include setup and
replay; committed operations are eight sends and four actual adapter swaps. Twelve affected
financing regression scenarios also passed on these binaries.

This evidence belongs to [PR #22](https://github.com/Dexter-DAO/dexter-account/pull/22),
starting from verified main `d7cfd401d9d5ab8cb1aa409f407572cd19746d31` after PR #21.
Program/shared-contract and adapter source is `fb2a3a4d2c2abec37468bd592ae72a75626d7b60`.
Final native fixtures, verifier and generated interfaces are
`5b205c6489efeb8d999d559434e251dfcaf92e2d`. Subsequent documentation and evidence packaging
do not change the exercised programs. The [manifest](verification.json) records source,
artifact and file hashes. Captured logs retain their original contents, including compiler
warnings and terminal newlines.

The [composition contract](../../wallet/SPENDING-CONTRACT.md) specifies exact independent
application inputs. The [current integration handoff](../../reviews/INTEGRATION-HANDOFF.md)
assigns remaining handwritten SDK, durable recovery and customer RPC work to Codex.

### Concurrent main integration

During closeout, Codex PR #23 merged to main at
`ddb40f73536a4391cb2782be8e77e2aee8185c56`. Its handwritten SDK and historical evidence are
preserved. The additive [integration verification](main-integration/verification.json)
records the current 261 SDK tests and two SDK-built governed repayments against this
branch's compiled programs, including fresh-process receipt-only recovery after a lost
response and zero rebroadcasts. The active program bridge's trace pins now select the
current qualified binaries; it does not fall back to historical hashes. Its selected
financial request and pre/post accounts agree byte-for-byte with the prior fixture.

The original 83-file execution package and its [inventory](evidence-files.json) remain
unchanged. An EOF-only blank-line cleanup in `wallet_spending.py` is documented with both
hashes and AST equivalence in the additive closeout; the original three traces still name
their exact `5b205c6` fixture source. Program source and generated interfaces are unchanged.
PR #23's separate localhost RPC evidence retains its own program pins and does not qualify
the new send/swap customer paths.

## What executed

| Suite group | Signed native-v1 transactions | Expected refusals | Assertions | Finance receipts | Wallet operations |
|---|---:|---:|---:|---:|---:|
| Send authority | 108 | 21 | 268 | 14 | 6 |
| Shared custody | 90 | 4 | 230 | 15 | 2 |
| Actual adapter swaps | 102 | 9 | 269 | 19 | 4 |
| New wallet subtotal | 300 | 34 | 767 | 48 | 12 |
| Financing regressions, 12 scenarios | 1,106 | 81 | 4,346 | 224 | 0 |
| All 15 scenarios | 1,406 | 115 | 5,113 | 272 | 12 |

[execution-report.json](execution-report.json) lists each scenario, raw/compressed trace
hashes and resource maxima. [case-index.json](case-index.json) maps each recorded transaction
label to its result and packet/resource measurements. Each `*-trace.json.xz` retains the
full signed packet, runtime result/logs, observed pre/post account bytes and assertions.
The smaller `*-receipt.json` summarizes native execution; `*-offline.json` records separate
offline verification. Historical PR #21 archives were not replaced by these new runs.

The new cases cover exact owner approval and bounded delegation, destination/asset/executor
binding, finite amount/count/time/interval limits, grant retry/revocation, owner-generation
changes, immutable replay and conflicting identity. They refuse cross-action/repayment
authority, foreign capital, unallocated surplus and spending accepted backing. Concurrent
activity is represented by competing prepared transactions and serialized shared-custody
interleavings, including fresh authorization after a stale-nonce refusal. Swaps also run
while two facilities share the input custody and in both pool orientations.

An independent sponsor collects an earned supplier entitlement after initiating-agent
revocation and cooperative financier-owner rotation. That transaction contains only the
sponsor's native signature. Actual/accounted custody cash falls from 300,000 to 200,000
token atoms, unused backing remains 200,000 and payable falls from 100,000 to zero; company
debt remains outstanding. No fresh financier-owner or initiating-agent approval is needed.
This proves the exercised program dependency, not a deployed-service outage experiment.

## Actual adapter and atomic failures

The adapter is official SPL Token Swap v3.0.0 at upstream revision
`264ca72de06b0c2b45c0b15d298000fe3f82db2e`. The live default head was verified before
inspection. [adapter-pin.json](adapter-pin.json) and
[adapter-build-receipt.json](adapter-build-receipt.json) retain the upstream archive and
lockfile hashes, exact unchanged source hashes, pinned build command and ELF identity.
Default upstream features are used; its optional production pool-creation fee/owner
configuration is not selected. No production rate or policy is chosen.

The standalone [adapter trace](adapter-direct-trace.json.xz) records nine signed native-v1
transactions, two expected refusals and ten assertions. It creates a real synthetic pool
through the actual program interfaces and exercises forward/reverse swaps and routing/
minimum-output refusals. It is separate from the four governed Core swaps. No test stand-in
is substituted for the adapter in either path.

The governed path reaches Swig at depth 1, Core at depth 2, Token Swap at depth 3 and legacy
Token at depth 4. Core authenticates the pool and checks actual user and pool-vault deltas.
[atomic-rollback-details.json](atomic-rollback-details.json) retains focused raw CPI evidence:

- In the real rounding case, approved input is 10,010 atoms but the actual Token input
  transfer is 10,009. The adapter also mints 2,424 LP fee tokens and transfers 9,481 output
  atoms, then returns success. Core rejects `WalletSwapOutcome`; the whole transaction
  restores the user accounts, pool vaults, LP mint/fee account, custody, permission and books.
- In the stale-projection case, the actual adapter succeeds and Registry consumes permission
  before the later accounting failure. The transaction restores every recorded protected
  account, including both mint-specific accounting paths and the adapter fee effects.

This qualifies that actual locally built implementation and its exercised interface. It
does not establish binary equivalence to a public deployment, its upgrade governance,
live-pool admission or qualification of Jupiter, Orca or an arbitrary router.

## Interfaces, source and checks

Actual IDLs are retained under [idl/](idl/), with their source receipt in
[idl-receipt.json](idl-receipt.json). The generated client manifest is
[generated-manifest.json](generated-manifest.json). Neither was hand-authored to resemble
the source. The current generated clients compile alongside the unchanged handwritten SDK.

[composition-vectors.json](composition-vectors.json) contains an exact approved send and
swap: request/permission Borsh bytes, digests, operation/leg IDs and PDAs, complete account
order/effective flags, signed native packets and raw result/permission/financial accounts.
[check-generated-vectors.mjs](check-generated-vectors.mjs) reconstructs the instruction
bytes and account order with the current generated TypeScript builders, and round-trips
the accounts, terms, identities and financial/result hashes. Its
[receipt](generated-vectors-receipt.json) confirms both vectors:

| Interface | Send | Exact-input swap |
|---|---:|---:|
| Core instruction bytes, including discriminator | 88 | 64 |
| Fixed Core accounts | 16 | 26 |
| Required Core/Pool book suffix | 2 | 4 |
| Economic request Borsh bytes | 441 | 441 |
| Permission terms Borsh bytes | 433 | 433 |
| Aggregate result account bytes | 715 | 715 |

Pinned host checks passed **33 workspace library tests** and **69 shared-contract tests**;
seven contract-library tests occur in both commands. TypeScript compilation and **245/245
existing SDK tests** passed. The vector checks establish actual generated-interface
agreement, not handwritten customer preparation, signer use or durable admission.

| Program artifact | Bytes | SHA-256 |
|---|---:|---|
| PrincipalRegistry | 782,728 | `e60391edb2a17f5b8398c83043d2cb97c5f55baf5c59f50bc9c795bc507cc604` |
| ClaimsCore | 1,934,008 | `a558c3f5140693c3c2b4e3b4c117f6c6cc256de5617fe9f1a52f5108c50b4e3f` |
| PoolIssuer | 334,384 | `524bc221b88035c911d8a5460e5e0526478838b621123b0e71d8ddbcc8a53a1b` |
| Retained Swig | See retained runtime artifact | `bb744d9811c392e3f343952d1a99bef9c97aa1936248a23b9d7efc1adb8939c1` |
| Actual SPL Token Swap | 169,080 | `545687ec1bee83f09638d5bf1d1a7dd8dc497b62a0788aa0532440c6fd095626` |

Build inputs and complete commands are in [build-receipt.json](build-receipt.json).
[doctor.json](doctor.json) records Node 24.22.1, Python 3.12.14/solders 0.29.0,
Anchor 1.2.2, cargo-build-sbf 4.5.0/platform-tools 1.57, host Rust 1.99.1 and the pinned
Agave 4.4.0 tools. SBPF v3 compilation is a separate choice from transaction version 1.

## Packet and resource measurements

| Recorded operation | Signed bytes | Account keys | Measured CU |
|---|---:|---:|---:|
| Exact owner-approved send | 1,080 | 22 | 299,759 |
| First delegated send | 1,080 | 22 | 296,776 |
| Independent accepted-right collection | 1,129 | 24 | 220,064 |
| Exact owner-approved swap | 1,464 | 34 | 517,574 |
| Reverse swap | 1,464 | 34 | 494,907 |
| Delegated swap | 1,464 | 34 | 496,530 |
| Real underdebit refusal and rollback | 1,464 | 34 | 241,786 |
| Late stale-projection refusal and rollback | 1,464 | 34 | 303,463 |

The maximum successful send consumption is 331,296 CU; swap is 517,574 CU. The largest
packet in the new three suites is 2,233 bytes, from supplier-commitment acceptance. The
regression maximum is 2,297 bytes and 701,340 CU. These native-v1 packets use the pinned
4,096-byte profile; the swap is not presented as a legacy/v0 1,232-byte packet.

Signed v1 settings request 1,400,000 CU and a 64 MiB loaded-account data limit, with zero
priority fee and no heap request. The pinned LiteSVM does not honor the inline resource
settings, so execution also uses an explicit 1,400,000-CU runtime override. This is an
identified qualification boundary, not proof of native-v1 RPC resource admission or fee
charging. Packet structure, signatures and actual execution are verified separately.

## Independent verification and reproduction

The [offline verifier](../../../foundation/onchain/tests/verify_wallet_spending.py) decodes
signed native packets and the compact SignV2/Core payload, reconstructs the complete
request and Registry bundle hash using effective privileges, and binds them to the result.
It checks receipt owners/addresses/hashes, both financial legs, actual token and custody
deltas, permission consumption and complete changed-book membership. Every refusal must
restore all recorded protected accounts; every retained assertion must agree. A second
reviewer reran all three final offline checks after reviewing the request-binding logic.
The verifier uses some shared receipt/quote helpers. It is not a separate monetary oracle,
an execution attestation or a substitute for retained native execution provenance.

From the repository root with the pinned workspace tools installed:

```bash
./dev doctor
./dev anchor-build
./dev anchor-idl
./dev exec python scripts/build-spl-token-swap.py
./dev client generate
./dev client build
./dev anchor-unit
./dev anchor-contracts
./dev anchor-smoke wallet-spending
./dev client test
./dev exec node docs/evidence/wallet-spending-20261101/check-generated-vectors.mjs
```

The adapter builder supports `--offline` after its pinned source/dependencies are cached.
To verify a retained trace without executing new transactions:

```bash
./dev exec python foundation/onchain/tests/verify_wallet_spending.py \
  docs/evidence/wallet-spending-20261101/wallet-spending-swap-trace.json.xz \
  --idl-directory docs/evidence/wallet-spending-20261101/idl
```

Repeat that offline command for send and shared traces. The affected native regressions
are `financing-wallet`, `financing-governed-repayment`, `financing-corrections` and
`financing-commercial`; their receipt/log files and separate offline verifications are
retained here. No additional validator run is needed for documentation-only edits.

## Remaining qualification

[runtime-availability.json](runtime-availability.json) records that Docker and its socket
were absent and the repository validator was stopped/not started. No customer RPC
transaction was sent in this pass. Complete signer/sponsor inspection, native resource
admission, priority fees, submission/finality, durable process recovery, real validator
contention and the deployed financier-backend-stopped journey remain Codex/release work.

The local use of disposable keys and synthetic pools/tokens creates no production
deployment, spending, price, fee commitment or package publication. Cooperative owner
rotation preserves accepted rights but does not qualify lost-owner recovery. The complete
account, financing, investor, identity and archival requirements remain in force.
