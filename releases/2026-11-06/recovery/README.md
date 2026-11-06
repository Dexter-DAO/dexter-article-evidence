# Recover control while accepted obligations continue

The compiled-program recovery journey restores control without the lost or hostile owner's signature. It recovers the buyer, financier, supplier and source before the first source report or supplier claim, while retaining their continuing identities, custody and accepted financial terms.

The run then establishes a 900-unit bill under the original 2,000-unit cap. An independent caller collects 400 to the accepted supplier custody; its owner withdraws 200. The supplier recovers again, the remaining 500 is collected into the same custody, and the new owner withdraws 700. Buyer debt remains 900. Old grants and stale owner proofs fail; retries do not pay twice. The separate fixed-external bill still pays its original destination.

[Scenario results](recovery-scenarios.json), the [execution receipt](recovery-receipt.json) and [qualification record](recovery-qualification.json) identify the result. The [complete compressed native trace](recovery-trace.json.xz) retains 168 transactions, 36 intended refusals, 674 assertions and 22 financial receipts. It expands to roughly 46 MB. Workspace-path redactions change its exported hash; [provenance](../provenance.json) preserves both source and export identities. Financial values and recorded outcomes are unchanged.

## Recovery authority

The exercised policy requires an explicitly enrolled two-of-three guardian quorum and proof of possession of the replacement key. The guardians can replace the operational owner without that owner's consent. Enrollment must happen before key loss. Replacing a guardian set requires the current owner and the required old and new quorums; losing the old quorum can therefore prevent a policy change. Recovery cannot undo already accepted effects.

Source and supplier continuity must be accepted under the continuing profile. A fixed external destination is not silently redirected to a recovered owner's new wallet. These are properties of the selected mechanism, not a claim that every customer has enrolled in it.

## Passkey authority alongside recovery

The separately retained [passkey RPC receipt](passkey-rpc-receipt.json) records same-Principal adoption, company authority and a financed admission with two P-256 approvals. Its completed local run had eleven finalized transactions. Native and passkey authority share the continuing Principal; this result does not turn the native guardian-recovery run into a physical-device recovery demonstration.

The passkey run used software keys and synthetic WebAuthn flags, Agave 4.4.0 with SIMD-0268 disabled, generated admission targets and an internal compiled financial-claim join. Its freshly built ELF bytes differ from the Pro build; their recorded identities and limits are retained. The [accepted source/evidence review](pr85-source-and-evidence-review.json) and [merge receipt](account82-84-85-merge-receipt.json) establish the reviewed source milestone. Later customer integration does not change what these completed runs established.

Run the release's standard-library checker to verify the file hashes and selected recorded recovery outcomes. This checks retained observations; it does not rerun native execution or certify all program behavior.
