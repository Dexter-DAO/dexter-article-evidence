# The November 18 devnet programs, read before the close

These three files are the on-chain bytes of the programs of the [November 18 devnet run](../README.md), read from Solana devnet (genesis `EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG`) on November 19, 2026 at 1:35 PM ET, at slot 524083120, with `solana program dump` from the Agave 4.4.0 CLI. The programs had not been closed when the files were read.

Each file's SHA-256 and size equal the on-chain hash and size in that run's [record](../devnet-record.json), and each program's last-deployed slot was unchanged since that record.

| Program | Address | Bytes | SHA-256 | Last deployed in slot | File |
|---|---|---:|---|---:|---|
| ClaimsCore | `727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz` | 3,121,456 | `a96472c1e26f35fcabe9a5af9122c2d807c1048ba8a7a511dc0b2ea5473b69f2` | 523,832,373 | [claims_core.727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz.devnet-dump.so](claims_core.727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz.devnet-dump.so) |
| PrincipalRegistry | `9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G` | 1,098,560 | `f6f4052ceb824561f98718176bb4d32d7851a603a949b750aecf2b9465587659` | 523,832,288 | [principal_registry.9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G.devnet-dump.so](principal_registry.9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G.devnet-dump.so) |
| PoolIssuer | `3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm` | 334,384 | `6804757b94ce2da8854388a1f748b614296207682db27cde85958f868b726e65` | 523,661,797 | [pool_issuer.3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm.devnet-dump.so](pool_issuer.3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm.devnet-dump.so) |

To check the files against their hashes, run `sha256sum -c SHA256SUMS` in this folder. The same three hashes are in `programs` in the run's record.
