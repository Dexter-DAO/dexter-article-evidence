# Re-executing the retained Account scenarios

Offline verification is complete with the files in this package. Fresh SVM execution requires compiled artifacts and setup that are separate from those checks.

## Exact original artifacts

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| `principal_registry.so` | 782728 | `e60391edb2a17f5b8398c83043d2cb97c5f55baf5c59f50bc9c795bc507cc604` |
| `claims_core.so` | 1934008 | `a558c3f5140693c3c2b4e3b4c117f6c6cc256de5617fe9f1a52f5108c50b4e3f` |
| `pool_issuer.so` | 334384 | `524bc221b88035c911d8a5460e5e0526478838b621123b0e71d8ddbcc8a53a1b` |
| `swig.so` | 296888 | `bb744d9811c392e3f343952d1a99bef9c97aa1936248a23b9d7efc1adb8939c1` |
| SPL Token Swap | 169080 | `545687ec1bee83f09638d5bf1d1a7dd8dc497b62a0788aa0532440c6fd095626` |

The original [build receipt](account/docs/evidence/wallet-spending-20261101/build-receipt.json) records the three Account artifacts under `/workspace/scratch/e85e65001bc6/dexter-account/.local/anchor/deploy/`. The exact Registry/Core/Pool ELF files are not included in this candidate. The source and retained hashes are included; a rebuilt binary must be compared with these hashes before it is described as the original artifact.

## Build inputs and execution setup

The complete 42-file program/build input set is under `execution/foundation/onchain/`. The receipt records `cargo-build-sbf 4.5.0`, platform tools `v1.57`, SBPF `v3`, `--locked`, Anchor `1.2.2` and the original commands. Use the relative manifest for each of `programs/principal-registry`, `programs/claims-core` and `programs/pool-issuer` when adapting those commands to a new checkout.

The selected fixture sources are under `execution/foundation/onchain/tests/`. Native execution also needs the matching IDLs copied into the fixture's `.local/anchor/idl` directory, a build receipt pointing to the actual ELF paths, and retained Swig at `foundation/swig/runtime/artifacts/swig-retained.so`. The swap scenario additionally needs the pinned SPL Token Swap artifact and its adapter configuration/build receipt. Those setup paths are used by the original fixture; the portable offline verifier accepts the included IDLs directly.

The [runtime record](account/docs/evidence/wallet-spending-20261101/doctor.json) and [adapter pin](account/docs/evidence/wallet-spending-20261101/adapter-pin.json) retain the original versions and hashes. Local native-v1 execution used a 4096-byte packet profile and an explicit 1,400,000-CU LiteSVM override. This is not a claim about public RPC resource admission.

No original fixture or program source was modified to make the offline check pass. No fresh SVM execution, rebuild or public transaction submission was performed while preparing this candidate.
