# Account program source at the November 18 commit

This folder holds the complete source of the three Account programs, ClaimsCore, PrincipalRegistry and PoolIssuer, and the types they share, at commit `e55c0ad3a224a1c4e72de4c283161345d438caaf` of the Dexter Account repository. It is the source the binaries were built from for the [commit](../README.md), [shared-reserve](../shared-reserve/README.md), [named-signer](../named-signer/README.md), [revocation](../revocation/README.md), [both-sides](../both-sides/README.md) and [refund-promise](../refund-promise/README.md) records. The [pinned-terms record](../pinned-terms/README.md) ran this commit plus one change and publishes the six files that differ. The [pinned-delivery record](../pinned-delivery/README.md) ran that source plus a further change and publishes the seven files that differ from those already published. The [pinned-price record](../pinned-price/README.md) ran the pinned-delivery source plus a further change and publishes the four files that differ from those already published. The [early-settlement record](../early-settlement/README.md) ran the pinned-price source plus a further change and publishes the three files that differ from those already published. The [delivery-rules record](../delivery-rules/README.md) ran the early-settlement source plus a further change and publishes the seven build inputs that differ from those already published, with the delivery-rules program's README and retained binary. The [devnet run](../devnet/README.md) put the early-settlement record's binaries on Solana devnet.

The 69 files are the `build_inputs` list each of those traces records. They are copied byte for byte at their repository paths, and each file's SHA-256 equals the trace's entry for it. The files total 1,177,459 bytes, and the Rust files hold 19,454 lines.

## What is here

| Path under `foundation/onchain/` | Files | Contents |
|---|---:|---|
| `programs/claims-core/src/` | 26 | ClaimsCore: orders, the reserve, collection, bills, corrections and wallet operations |
| `programs/principal-registry/src/` | 11 | PrincipalRegistry: owners, agents, permissions, budgets, passkeys, recovery and the wallet authorization |
| `programs/pool-issuer/src/` | 2 | PoolIssuer: pools and the ordered projection of ClaimsCore's financial journals |
| `crates/contracts/src/` | 13 | The types and calculations the three programs share |
| `crates/contracts/tests/`, `crates/contracts/examples/` | 10 | The shared crate's nine test files and one encoding example |
| `Cargo.toml` in the workspace and each of the four crates, `Cargo.lock`, `Anchor.toml` | 7 | Build configuration: the manifests, the dependency lockfile and the Anchor workspace file that names the three program addresses |

The seven build files and the ten test and example files are build inputs, not program logic. They are included because the trace lists them. The Swig wallet program is not among these inputs, and each trace records its binary hash separately.

The [named-signer folder](../named-signer/source) holds two of these files, pinned by that record's own hashes. This folder is the complete set. The [program source under `evidence/`](../../../evidence/account-evidence/execution/foundation/onchain/programs) is the earlier pin of the November 1 records, `fb2a3a4`.

## Where the article's mechanisms are

Paths are under `foundation/onchain/`. Line numbers are where each function starts.

| Mechanism | File | Function |
|---|---|---|
| The commit: one transaction checks permission, money and the deal | [programs/claims-core/src/funding.rs](foundation/onchain/programs/claims-core/src/funding.rs) | `accept`, line 467, the handler of `accept_financed_tab`. Its checks are in `accept_bound_payee`, line 492: the signers and terms, then the credit line, the facility and the lender's cash at lines 531 to 534, then the call into the Registry at line 562 |
| | [programs/principal-registry/src/lib.rs](foundation/onchain/programs/principal-registry/src/lib.rs) | `reserve`, line 235: the agent's key, the permission, the per-order limit and the seller the permission names |
| | [programs/principal-registry/src/state.rs](foundation/onchain/programs/principal-registry/src/state.rs) | `Budget::reserve`, line 88: the hold against the buyer's budget |
| Collection of what the seller earned | [programs/claims-core/src/funding_collection.rs](foundation/onchain/programs/claims-core/src/funding_collection.rs) | `collect`, line 15, the handler of `collect_financier` |
| | [programs/principal-registry/src/wallet_funding.rs](foundation/onchain/programs/principal-registry/src/wallet_funding.rs) | `authorize_funding`, line 156, the handler of `authorize_funding_wallet`, and `validate_funding_instruction`, line 232 |
| | [programs/principal-registry/src/swig_framing.rs](foundation/onchain/programs/principal-registry/src/swig_framing.rs) | `CompactFrame::parse`, line 21: the one instruction the wallet may sign |
| The reserve floor | [programs/claims-core/src/funding_state.rs](foundation/onchain/programs/claims-core/src/funding_state.rs) | `FinancierCustody::protected`, line 74, and `debit_uncommitted`, line 93 |
| The named delivery signer | [programs/claims-core/src/evidence_billing.rs](foundation/onchain/programs/claims-core/src/evidence_billing.rs) | `accept_evidence_policy`, line 127, `verify_signature`, line 216, `submit_evidence`, line 234, and `finalize`, line 276 |
| | [crates/contracts/src/billing.rs](foundation/onchain/crates/contracts/src/billing.rs) | `deterministic_bill_amount`, line 224 |
| Guardian recovery | [programs/principal-registry/src/recovery.rs](foundation/onchain/programs/principal-registry/src/recovery.rs) | `configure_policy`, line 266, and `recover_owner`, line 316 |
| | [programs/claims-core/src/recoverable_billing.rs](foundation/onchain/programs/claims-core/src/recoverable_billing.rs) | `accept`, line 65, `submit_evidence`, line 139, and `submit_claim`, line 166 |
| | [programs/claims-core/src/receiving.rs](foundation/onchain/programs/claims-core/src/receiving.rs) | `open`, line 217, and `withdraw`, line 256 |
| Passkey permission | [programs/principal-registry/src/passkey.rs](foundation/onchain/programs/principal-registry/src/passkey.rs) | `authorize_operation`, line 272, the handler of `authorize_passkey_operation` |
| | [crates/contracts/src/webauthn.rs](foundation/onchain/crates/contracts/src/webauthn.rs) | `verify_webauthn_assertion`, line 66 |
| Corrections and refunds | [programs/claims-core/src/corrections.rs](foundation/onchain/programs/claims-core/src/corrections.rs) | `correct`, line 278, the handler of `correct_financed_run` |
| | [programs/claims-core/src/interest_refunds.rs](foundation/onchain/programs/claims-core/src/interest_refunds.rs) | `refund`, line 163, the handler of `refund_funded_interest` |
| Key rotation | [programs/principal-registry/src/ownership.rs](foundation/onchain/programs/principal-registry/src/ownership.rs) | `rotate`, line 53, the handler of `rotate_principal_owner` |
| | [programs/principal-registry/src/lib.rs](foundation/onchain/programs/principal-registry/src/lib.rs) | `rotate_actor`, line 189, and `revoke_actor`, line 209, for an agent's key |

The instruction names are the entry points in each program's `lib.rs`. The source lines those six records' case indexes cite in `error_source` are lines of these files.

## Match a file to a trace

From this folder, with any file's path as the second argument:

```sh
python3 -c "import json,lzma,hashlib,sys; b=json.loads(lzma.open(sys.argv[1]).read())['build_inputs']; f=sys.argv[2]; print(hashlib.sha256(open(f,'rb').read()).hexdigest()==b[f], f)" ../commit-legs-trace.json.xz foundation/onchain/programs/claims-core/src/funding.rs
```

It prints `True` and the path when the file's hash equals the trace's build input. To check all 69 files against the trace, and every file against this folder's own hashes:

```sh
python3 -c "import json,lzma,hashlib,sys; b=json.loads(lzma.open(sys.argv[1]).read())['build_inputs']; print(all(hashlib.sha256(open(f,'rb').read()).hexdigest()==h for f,h in b.items()), len(b))" ../commit-legs-trace.json.xz
sha256sum -c SHA256SUMS
```

The first command prints `True 69`.

## Files

| File | Contents |
|---|---|
| [foundation/](foundation) | The 69 source files at their repository paths. |
| [provenance.json](provenance.json) | The commit, and each file's path, size, SHA-256 and Git blob hash, with the trace's build-input hash beside it. |
| [NOTICE.md](NOTICE.md) | The files retain their existing copyright, and this release adds no license grant for them. |
| [SHA256SUMS](SHA256SUMS) | Hashes of the 69 source files, the notice and the provenance file. |
