## 3. The Account is the principal's estate

An Account belongs to a continuing person, company or other recognized principal. It has stable identity and history, controlling authorities, custody bindings, assets, accepted commitments, debts, receivables and permissions. A wallet address is one custody binding. An agent is an actor. A conversation or job is an execution context. A key is a credential. None of those is the Account itself.

Use a stable `PrincipalId`, scoped by network and Registry domain. The principal kind and governing agreement reference are explicit. A company may have several officers and controlled wallets; a person may replace devices; an agent may change model, runtime and keys. Those changes update subordinate bindings and generations. They do not move the principal's identity, restart budget counters or orphan obligations.

The fresh Registry must implement the following durable relationships:

| Record | Required meaning |
|---|---|
| Principal | Stable identity, kind, governing-policy reference, current authority generation and custody bindings |
| Owner/officer authority | Who can approve which principal action, with exact scope and threshold where applicable |
| Actor | Continuing operator identity, independent of its current process and key |
| Credential | Key type, binding, validity, generation and permitted authentication role |
| Mandate | Owner-approved actions, assets, counterparties, payment modes, per-operation and aggregate ceilings, expiry and escalation rules |
| Budget bucket | Shared consumption and reservations; all relevant jobs point to it |
| Job/session binding | Context and narrower permissions within an existing mandate; no independent replenished allowance |
| Operation permit/result | Exact intent, unique operation identity, consumed origin, accepted consequences and durable reconciliation reference |
| Wallet binding | Swig configuration, wallet address, program/deployment profile, complete allowed management graph and generation |
| Migration receipt | Consume-once provenance and the precise legal/economic continuity accepted into this principal |

These are contract concepts, not a requirement for one account per row. Final packed layouts should follow actual concurrency, rent and proof requirements. Credential and policy generation are monotonic; an opening slot is never an incarnation nonce. Closing storage cannot make an old signed operation valid again.

### 3.1 Authority, cash and credit are different quantities

For a shared expenditure budget, the default invariant is:

`consumed expenditure + accepted but unconsumed authorization <= approved ceiling`.

Delivery/capture changes the relevant reserved authorization into consumed expenditure. A canceled, genuinely unaccepted preparation may release its reservation. Revocation prevents new accepted commitments; it does not erase previously accepted supplier rights. Repayment releases financial credit exposure, not a lifetime agent expenditure allowance. Recurring budget windows and outstanding-credit limits are separate counters.

A $1,000 mandate does not imply $1,000 in escrow. It permits eligible expenditures up to $1,000 under its other constraints. A particular accepted $3 delivery may consume $3 of authority and commit $3 of a selected funding source. The available amount returned by the SDK must identify which constraint is limiting it: authority, cash, accepted source capacity, credit admission, issuer liquidity or accounting freshness.

Default recurring expenditure windows use an explicit UTC timestamp policy, with the admission window recorded on a finite accepted authorization. Pending commitments must not disappear at a window rollover. A new window does not discharge old debt. Native Swig slot-based recurring limits are useful additional controls, but they must not be displayed as a wall-clock daily account budget without an exact agreed conversion and enforcement contract.

For concurrency, begin with atomic consumption of the shared budget account. Independent jobs may have distinct Tabs and operation IDs while sharing that budget. If measured traffic requires partitioning, allocate finite child quotas from the parent: allocation removes capacity from the parent, child use cannot exceed its allocation, and reclaim cannot take capacity backing accepted work. Creating another conversation never creates another quota by default.

## 4. Program structure and permanent interfaces

| Boundary | Permanent responsibility | Implementation flexibility |
|---|---|---|
| PrincipalRegistry | Principal, actor and credential bindings; exact intent; grants, aggregate budgets and management policy; replay-safe operation identity | Swig deployment profile, owner ceremony, hosted/self-held signer, controlled wallet generation |
| ClaimsCore | Accepted terms and funding certificate; origin, debt/payable conservation, title, settlement, correction, offset, default and accepted-right service | Cash, supplier credit and independently admitted B07/B08 source/issuer profiles |
| PoolIssuer | Treasury and beneficial escrow accounting; tranches, positions, pricing, income, redemption, losses, recoveries and manager powers | Epoch and continuous investor products, eligible transfers and external capital sponsors |
| IdentityGuard | Principal consent, custody-domain commitment, default/release cutoff, exact creditor/recipient entitlement and replay protection | Qualified Arcium profile or another admitted conditional-release implementation |
| Vendor adapter | Queue/computation definitions, cryptographic verification and authenticated recipient delivery for its declared capability | Exact vendor SDK, deployed cluster, preprocessing and recovery profile |
| Journal/indexing components | Complete program effect commitments, readable records, archive/proof availability and reconciliation | Storage provider, independent indexer, browser/server reader and statement UI |

PrincipalRegistry is the account/authority registry. ClaimsCore owns the separate financial admission directory for issuer and funding profiles, including which program/version may issue an accepted B07/B08 certificate and which service route that certificate preserves. An issuer references its continuing principal and Registry authority; PoolIssuer owns that issuer's investor engine. These distinct records should not share the ambiguous name "Registry" in generated clients or operations logs.

The small shared contract library owns identifiers, version negotiation, amount encodings, effect types, typed errors and generated codecs. Program ownership does not leak into the ordinary Account SDK interface. The modules still expose low-level builders for specialist integrations.

The stable cross-module contracts are:

- **Account/Authority context:** principal, actor, mandate, policy revision, credential generation, operation ID, network/program domain and exact action terms.
- **Funding certificate (B07):** source/profile/partition, asset, finite amount, beneficiary, accepted terms, unused-capacity release rule, earned-right service descriptor and admitted trust/deployment profile.
- **Issuer admission (B08):** issuer principal, controlling program/version, eligibility and risk decision, pricing/terms hash, accepted credit amount and funding-certificate reference. An advertised credit limit alone is not a certificate.
- **Economic origin and lot:** delivery origin, debtor, original supplier payable, current debt creditor, asset, residual amounts, basis/provenance and relevant terms. Creditor assignment does not rewrite original supplier backing.
- **Book effect:** all affected economic book IDs and partitions, operation/lot identity, typed conservation delta, exact money-routing effect, predecessor/sequence/hash and required consumer set.
- **Release entitlement:** principal, identity/custody domain, eligible finalized default, claim and creditor at cutoff, authenticated recipient key, permitted fields/purpose, accepted policy revision and nonce.
- **Durable operation result:** exact intent digest, attempts, acceptance/delivery/settlement states, origin/claim IDs, current or historical evidence basis, journal checkpoint and proof availability.

Freeze these meanings and version-negotiation rules before program ABIs. Generate serializers, layouts and Rust/TypeScript vectors from the chosen schemas. Do not freeze vendor-specific ciphertext or queue encodings before their qualification. Adding a pool product or an Arcium profile must not replace principal IDs or already accepted claims.

### 4.1 What remains atomic

An economic origin is consumed once. Its debt/payable changes, authority consumption, money movement and complete affected-book effects commit in the same Solana transaction. Capture creates debt once at delivery. Funding an existing payable reduces that payable and its backing; it creates no new buyer debt. Refund, correction, offset and title transfer have explicit conservation transitions. A journal worker cannot decide those facts after the transaction.

The authoritative equations and transitions are [the monetary model](money/model/pool_model.py), with its revision and equation provenance in [model authority](money/evidence/model-authority.json). The acquisition discount remains a deduction from gross debt valuation. The previous discrepancy is not reopened by this migration. Repeated tables and implementation assertions must derive from the model/schema rather than independent prose formulas.

Immutability is a deployment and governance fact. Whole economic books sharing capital must pin their engine and accepted governance profile consistently. A registry metadata code hash does not freeze an upgradeable executable. Registry governance cannot retroactively rewrite the terms or remove the service route of an accepted obligation. Prospective module versions use explicit version admission and rights-preserving migration.

The production planner must preserve a directed call graph. Existing local evidence covers
exact Registry/Swig repayment, funding-wallet collection and withdrawal profiles. The
implemented send, admitted SPL/Jupiter exact-input swap and token withdrawal profiles use
that directed route with a published composition contract and separate execution
qualification. Governed run/facility repayment and PR #25 sends have recorded localhost
SDK/RPC verification under their retained source/artifact pins. Current schema-2 spending,
token lifecycle and admitted Pump/stock paths have local SDK/RPC qualification at their
recorded pins. Additional wallet profiles, public execution and complete financing
onboarding/provider journeys still need qualification.
For a governed-wallet operation, use top-level native credential verification when required,
top-level Registry authorization and top-level Swig SignV2 invoking Core; Core performs its
admitted downstream calls. Core-escrow servicing can begin directly at Core. Pool projection
reads Core-owned records/effects without calling back into Core during a Core dispatch.
Keep Core-to-Swig signing and Core-to-Pool-to-Core cycles outside these admitted profiles.
Measure each complete transaction against the qualified runtime's depth/account limits.
