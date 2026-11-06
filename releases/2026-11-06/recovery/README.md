# Recover control while accepted obligations continue

The compiled-program recovery journey, on a local runtime, restores control without the lost or hostile owner's signature. It recovers the buyer, financier, supplier and assignee after the fixed-external bill's first report and claim and before the main bill's, while retaining their continuing identities, custody and accepted financial terms.

The run then establishes a 900-unit bill under the original 2,000-unit cap, in fixture units. An independent caller collects 400 to the accepted supplier custody, and its owner withdraws 200. The supplier recovers again, the remaining 500 is collected into the same custody, and the new owner withdraws 700. Buyer debt remains 900. Old grants and stale owner proofs fail, and retries do not pay twice. The separate fixed-external bill still pays its original destination.

[Scenario results](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/releases/2026-11-06/recovery/recovery-scenarios.json) and the [execution receipt](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/releases/2026-11-06/recovery/recovery-receipt.json) identify the result. The [complete compressed native trace](recovery-trace.json.xz) retains 168 transactions, 36 refusals, 674 assertions and 22 financial receipts, and expands to roughly 46 MB.

## Recovery authority

The exercised policy requires an explicitly enrolled two-of-three guardian quorum and proof of possession of the new key. Enrollment happens before key loss. Changing a guardian set requires the current owner and the required old and new quorums. Accepted effects stay in place through recovery.

Source and supplier continuity are accepted under the continuing profile. A fixed external destination keeps paying its original destination after a recovered owner adopts a new wallet.

## Passkey authority alongside recovery

The separately retained passkey RPC receipt, [quoted here](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/excerpts/records.md#passkey-authority-alongside-recovery), records same-Principal adoption, company authority and a financed admission with two P-256 approvals, on a local run with software-generated P-256 keys and eleven finalized transactions. Native and passkey authority share the continuing Principal.

Run the release's standard-library checker to verify the file hashes and selected recorded recovery outcomes.
