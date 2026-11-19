# Account program source at the November 19 devnet build

This folder holds the complete source of the four Account programs, ClaimsCore, PrincipalRegistry, PoolIssuer and DeliveryRules, and the types they share, at commit `c1e1d156f449cb832647cfd2f1c76896616eb1ce` of the Dexter Account repository. It is the source the binaries of the [November 19 devnet run](../devnet/README.md) were built from. The run's page names the commit `cd2b2bb`. The two commits hold the same program source: every file here has the same bytes at both.

The [Tab receipt book record](../tab-receipt-book/README.md) ran this source with four other program addresses. Nine of its build inputs differ from the files here, in the addresses and in the hashes the tests derive from them, and that record publishes those nine.

The 76 files are the build inputs of the four programs. They are copied byte for byte at their repository paths. The files total 1,259,290 bytes, and the Rust files hold 21,125 lines.

## What is here

| Path under `foundation/onchain/` | Files | Contents |
|---|---:|---|
| `programs/claims-core/src/` | 30 | ClaimsCore: orders, the reserve, collection, bills, corrections, wallet operations, the events each Tab step emits, the closes and the settlement hold |
| `programs/principal-registry/src/` | 11 | PrincipalRegistry: owners, agents, permissions, budgets, passkeys, recovery and the wallet authorization |
| `programs/pool-issuer/src/` | 2 | PoolIssuer: the ordered projection of ClaimsCore's financial journals |
| `programs/delivery-rules/src/` | 1 | DeliveryRules: rules under which a program signs a delivery report |
| `crates/contracts/src/` | 14 | The types and calculations the four programs share, among them the number window |
| `crates/contracts/tests/`, `crates/contracts/examples/` | 10 | The shared crate's nine test files and one encoding example |
| `Cargo.toml` in the workspace and each of the five crates, `Cargo.lock`, `Anchor.toml` | 8 | Build configuration: the manifests, the dependency lockfile and the Anchor workspace file that names the four program addresses |

The eight build files and the ten test and example files are build inputs, not program logic. They are included because the build reads them. The Swig wallet program is not among these inputs, and the devnet record gives its on-chain hash separately.

## Where the Tab receipt book is

Paths are under `foundation/onchain/`. Line numbers are where each function starts.

| Mechanism | File | Function |
|---|---|---|
| A number that can be used once | [crates/contracts/src/window.rs](foundation/onchain/crates/contracts/src/window.rs) | `admit`, line 92: a number above the window's highest moves the window up, a number inside it is admitted once, and a number below it is never admitted |
| The step count and the running fingerprint | [programs/claims-core/src/tab_events.rs](foundation/onchain/programs/claims-core/src/tab_events.rs) | `record_tab_step`, line 399: each Tab step emits one event and extends the Tab's fingerprint with the event's bytes |
| Closing a settled order | [programs/claims-core/src/tab_close.rs](foundation/onchain/programs/claims-core/src/tab_close.rs) | `handle_close_run`, line 148, `handle_close_repayment_record`, line 130, and `handle_close_tab`, line 184: each deposit returns to the payer recorded when the record was created |
| Closing an approval after it expires | [programs/claims-core/src/tab_close.rs](foundation/onchain/programs/claims-core/src/tab_close.rs) | `handle_close_approval`, line 255 |
| A rules program and its upgrade authority | [programs/claims-core/src/evidence_billing.rs](foundation/onchain/programs/claims-core/src/evidence_billing.rs) | `check_rule_program`, line 207: a rules program is accepted when it has no upgrade authority, or when its upgrade authority is ClaimsCore's own |

The four program addresses are in each program's `lib.rs` (`declare_id!`) and in `Anchor.toml`. They are the addresses the programs have on devnet.

## Match the files to the devnet record

The devnet record carries one hash over all 76 build inputs, `build.build_inputs_sha256`. From this folder:

```sh
python3 -c "
import hashlib, json, sys
paths = [l.split('  ', 1)[1].strip() for l in open('SHA256SUMS') if 'foundation/' in l]
inputs = {p: hashlib.sha256(open(p, 'rb').read()).hexdigest() for p in paths}
print(len(inputs), hashlib.sha256(json.dumps(inputs, sort_keys=True).encode()).hexdigest())
print(json.load(open(sys.argv[1]))['build']['build_inputs_sha256'])
" ../devnet/devnet-record.json
sha256sum -c SHA256SUMS
```

The first command prints `76 7a4d8397e09d77241021fc27e897594161c7c5e62d1f7e05595f59cbc9583de9` and then the same hash from the record.

## Files

| File | Contents |
|---|---|
| [foundation/](foundation) | The 76 source files at their repository paths. |
| [provenance.json](provenance.json) | The commit, and each file's path, size, SHA-256 and Git blob hash. |
| [NOTICE.md](NOTICE.md) | The files retain their existing copyright, and this release adds no license grant for them. |
| [SHA256SUMS](SHA256SUMS) | Hashes of the 76 source files, the notice and the provenance file. |

The [November 18 source](../../2026-11-18/source/README.md) is the earlier pin, `e55c0ad`.
