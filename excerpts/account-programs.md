# Account programs: the source lines and recorded rows behind Dexter's references

This page quotes what [dexter.cash/llms.txt](https://dexter.cash/llms.txt) and the [full reference](https://dexter.cash/llms-full.txt) cite on three-way commit, the account and its funding arrangements, the network fee and corrections. Source lines come from the [program source](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/evidence/account-evidence/execution/foundation/onchain/programs), at the execution pin of the Nov 1, 2026 records (fb2a3a4). Row numbers are zero-based transactions in the retained traces linked above their tables, which the compiled programs produced on a local SVM with synthetic assets. Amounts are token atoms.

## Three-way commit: three checks in one transaction

The `accept` instruction in [funding.rs](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/execution/foundation/onchain/programs/claims-core/src/funding.rs), lines 391 to 476, calls `reserve` in the Registry's [lib.rs](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/execution/foundation/onchain/programs/principal-registry/src/lib.rs) inside the same transaction. Any failed check fails the whole transaction.

Did the owner say yes? The agent's current key signs, under an active mandate at its exact revision, inside the per-operation ceiling and the shared budget.

```rust
// principal-registry/src/lib.rs
213: require_keys_eq!(ctx.accounts.credential.key(), actor.current_key, RegistryError::CredentialKey);
215: require!(mandate.active, RegistryError::MandateRevoked);
216: require!(mandate.revision == terms.mandate_revision, RegistryError::MandateRevision);
227: require!(terms.cap > 0 && terms.cap <= mandate.per_operation_limit, RegistryError::PerOperationLimit);
229: ctx.accounts.budget.reserve(terms.cap)?;
```

Is the money there, and will it stay? The order must fit the credit line, the facility and the cash floor of the wallet that backs it.

```rust
// claims-core/src/funding.rs
418: require!(plus(credit_total,plus(terms.cap,maximum_interest)?)?<=a.credit_line.limit,CoreError::FundingCreditLine);
420: require!(plus(plus(a.facility.reserved,a.facility.drawn)?,terms.cap)?<=a.facility.commitment_limit,CoreError::FundingFacility);
421: require!(plus(a.custody.protected()?,terms.cap)?<=a.custody.accounted_cash,CoreError::FundingCashShortfall);
```

Who is the deal with, and on what terms? The supplier and the issuer's owner sign the exact terms, and those terms must match the mandate's supplier, asset and funding source.

```rust
// claims-core/src/funding.rs
114: pub issuer_owner:Signer<'info>,
116: pub supplier:Signer<'info>,
401: require!(a.issuer.owner==a.issuer_owner.key() && a.supplier_principal.owner==a.supplier.key(),CoreError::FundingOwner);
403: require!(terms.domain==a.principal.domain && terms.principal==a.principal.key() && terms.actor==a.actor.key()
404:     && terms.mandate==a.mandate.key() && terms.asset==a.mint.key() && terms.supplier==a.supplier.key()
405:     && terms.source==a.facility.key() && terms.cap>0 && terms.expires_at>Clock::get()?.unix_timestamp,CoreError::FundingTerms);
// principal-registry/src/lib.rs
224: require_keys_eq!(terms.asset, ctx.accounts.budget.asset, RegistryError::IntentMismatch);
225: require_keys_eq!(terms.supplier, mandate.supplier, RegistryError::IntentMismatch);
226: require_keys_eq!(terms.source, mandate.source, RegistryError::IntentMismatch);
```

If all three pass, the order's maximum is held in the reserve, and the seller can start work.

```rust
// claims-core/src/funding.rs
460: a.custody.unused=plus(a.custody.unused,terms.cap)?;a.custody.revision=plus(a.custody.revision,1)?;
```

Rows in [financing-wallet-trace](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/account/docs/evidence/wallet-spending-20261101/financing-wallet-trace.json.xz):

| Row | Name | Result |
|---|---|---|
| 64 | "first facility accepts work against the financier wallet" | Accepted. No token program call, and every token balance is unchanged |
| 65 | "second facility accepts work against the same financier wallet" | Accepted. No token program call, and every token balance is unchanged |
| 74 | "third otherwise eligible Tab cannot exceed the standing seller fee aggregate" | Refused, error 6050. All 97 observed accounts are unchanged |
| 87 | "revoked initiating agent cannot create a new commitment" | Refused, error 6004. All 111 observed accounts are unchanged |

The seller later collects what it earned. [funding_collection.rs](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/execution/foundation/onchain/programs/claims-core/src/funding_collection.rs) opens with its rule:

```rust
// claims-core/src/funding_collection.rs
1: //! Independent servicing of already-earned supplier rights. No initiating
2: //! agent, financier owner, issuer backend or hosted subscription authorizes it.
```

## The account

From section 3 of the Account specification in the Nov 4 package, `evidence/account-evidence/spec/account-and-interfaces.md`:

> An Account belongs to a continuing person, company or other recognized principal. It has stable identity and history, controlling authorities, custody bindings, assets, accepted commitments, debts, receivables and permissions. A wallet address is one custody binding. An agent is an actor. A conversation or job is an execution context. A key is a credential. None of those is the Account itself.

> A company may have several officers and controlled wallets; a person may replace devices; an agent may change model, runtime and keys. Those changes update subordinate bindings and generations. They do not move the principal's identity, restart budget counters or orphan obligations.

## Funding arrangements

From section 7 of the specification, `evidence/account-evidence/spec/tab-and-backing.md`:

> Keep spending permission, accepted guarantee capacity and earned payable as separate state.

| Arrangement | Whose resources become committed? | When cash moves | Result after delivery | Reuse/release and supplier service |
|---|---|---|---|---|
| Cash at delivery | Buyer's finite accepted payment authorization | Buyer wallet to supplier as the accepted purchase executes | Paid receipt; no residual issuer debt/payable | Remaining free cash remains usable; authorized refund is a distinct later right |
| Cash prefunded for a finite order | Buyer's exact order reserve | Buyer wallet to a protected source before the protected promise is accepted | Supplier's funded right is paid against the reserved amount | Only provably unused capacity returns; full mandate ceiling is not funded |
| Supplier-carried credit | Supplier carries the buyer's credit risk | No funding transfer is required at acceptance/capture | One buyer-to-supplier debt | Current creditor collects/assigns; no fictitious issuer guarantee or self-payable |
| Wallet-backed source | Buyer's or issuer's exact wallet reserve under a complete control graph | Cash can remain in the managed wallet until payout | Accepted capacity becomes the earned supplier right defined by that profile | Every spending and management path must preserve the floor; settlement must survive agent rotation |
| Pool cash guarantee | Pool investors' committed source cash | Pool treasury to Core escrow before the guarantee becomes accepted | Buyer debt to issuer plus issuer payable to supplier, created once | Unused reserve can release; payable backing survives agent/issuer outage; supplier can use Core payout |

The [reserve state](../evidence/account-evidence/execution/foundation/onchain/programs/claims-core/src/funding_state.rs) records committed and drawn capacity. The [signed shared-custody sequence](records.md#the-decoded-account-sequence) shows the protected floor through revocation, owner rotation and collection. The [financing contract types](../evidence/account-evidence/execution/foundation/onchain/crates/contracts/src/financing.rs) define the fields used by those programs.

## The network fee

The fee is a supplier-approved term of each accepted order: a rate, an aggregate cap and a fixed destination.

```rust
// crates/contracts/src/commercial.rs
38: pub struct SellerFeeTerms {
44:     pub rate_bps: u16,
47:     pub aggregate_fee_cap: u64,
51:     pub beneficiary: Pubkey,
53:     pub destination: Pubkey,
```

[Collection](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/execution/foundation/onchain/programs/claims-core/src/funding_collection.rs) pays the supplier's net and the fee in one transaction, from lines 116 to 124:

```rust
// claims-core/src/funding_collection.rs
118:     &ctx.accounts.destination.to_account_info(), &ctx.accounts.mint, ctx.accounts.token_program.key(),
119:     settlement.supplier_net)?;
124:     &fee_destination.to_account_info(), &ctx.accounts.mint, ctx.accounts.token_program.key(), settlement.fee)?;
```

The run's [fixture fee terms](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/execution/foundation/onchain/tests/financing_fixture.py) are rate_bps 137 plus fixed_fee 7, so 60,000 atoms carry a fee of 829.

Rows in [financing-commercial-trace](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/account/docs/evidence/wallet-spending-20261101/financing-commercial-trace.json.xz):

| Row | Name | Result |
|---|---|---|
| 55 | "supplier approves immutable aggregate fee agreement main" | Agreement recorded |
| 68 | "second finite Tab shares the supplier-approved parent fee cap" | Accepted |
| 69 | "third otherwise eligible Tab cannot exceed the standing seller fee aggregate" | Refused, error 6050 |
| 85 | "successful wallet supplier collection reimburses approved token cost and margin" | Receipt: amount 60,000, supplier_net 59,171, seller_fee 829. From those proceeds the supplier reimbursed its approved sponsor 550 |
| 97 | "ordinary authorized third-party cash services buyer interest without company approval" | Receipt: protocol_interest 370 |
| 98 | "company repays remaining principal and interest to current creditor" | Receipt: protocol_interest 1,850 |
| 103 | "issuer separately repays external financier principal and accrued funding interest" | Receipt: protocol_interest 629 |
| 107 | "fee beneficiary returns a first actual portion to original supplier" | Fee credit 400 |
| 109 | "fee beneficiary returns the remaining actual collected fee" | Fee credit 429 |
| 110 | "returned seller fees cannot be returned a second time under a different ID" | Refused, error 6030 |

## A wrong or partial delivery

The `correct` instruction in [corrections.rs](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/execution/foundation/onchain/programs/claims-core/src/corrections.rs) takes a signed correction of one delivered run. The buyer's owner signs every correction. The supplier's owner signs any correction that changes the bill, and the current creditor's owner signs where it loses principal or waives interest.

```rust
// claims-core/src/corrections.rs
23: pub struct CorrectFinancedRunArgs {
26:     pub unpaid_reduction: u64,
27:     pub collected_return: u64,
28:     pub creditor_compensation: u64,
29:     pub interest_waiver: u64,
```

Returned cash goes to the current creditor's account, line 343 of the same file. A refund of an earlier repayment goes back to the account the original receipt debited, line 43 of [historical_refunds.rs](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/execution/foundation/onchain/programs/claims-core/src/historical_refunds.rs).

Rows in [financing-corrections-lifecycle-trace](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/account/docs/evidence/wallet-spending-20261101/financing-corrections-lifecycle-trace.json.xz):

| Row | Name | Result |
|---|---|---|
| 61 | "supplier and company negotiate an unpaid reduction with creditor consent" | Bill reduced by 10,000 |
| 62 | "same correction ID returns its original result without another credit" | Original result returned |
| 63 | "same correction ID cannot change its accepted amount" | Refused, error 6038 |
| 64 | "current creditor cannot lose principal without consent" | Refused, error 6052 |
| 69 | "fee beneficiary explicitly credits its collected fee to the frozen supplier route" | Fee credit 281 |
| 76 | "independent other-domain owner funds the approved cash return without fresh creditor signature" | 5,000 returned |
| 87 | "different correction IDs cannot over-reduce the remaining supplier bill" | Refused, error 6031 |
| 88 | "full correction closes the original bill and preserves the later run" | 25,000 corrected |
| 95 | "original collection retry after full refund recovers its old receipt without paying again" | Old receipt returned |

Seven of the [fifteen retained scenarios](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/checks/offline-check-20261104.json) cover corrections and refunds: financing-corrections-lifecycle, -ownership, -ownership-escrow, -own-capital, -repaid-uncollected and -interest-alias, and financing-governed-repayment-refunds.

## Read a row yourself

Download a trace and run this with its path and a row number:

```sh
python3 -c "import json,lzma,sys; t=json.loads(lzma.open(sys.argv[1]).read()); x=t['transactions'][int(sys.argv[2])]; print(x['name'],x['expected'],x['result'].get('err')); print([r['data'] for r in t['receipts'] if r['address'] in x['account_keys']])" financing-commercial-trace.json.xz 85
```

For wallet spending, the [case index](../evidence/account-evidence/account/docs/evidence/wallet-spending-20261101/case-index.json) identifies each transaction by suite and row. The [execution report](../evidence/account-evidence/account/docs/evidence/wallet-spending-20261101/execution-report.json) lists the scenario traces. The [wallet verifier source](../evidence/account-evidence/execution/foundation/onchain/tests/verify_wallet_spending.py) shows how the fixture checks those results.

## Check the Account records

From `evidence/account-evidence/` in the [dated ZIP](https://github.com/Dexter-DAO/dexter-article-evidence/releases/tag/2026-11-06b), with Python 3.10 or newer:

```sh
python3 -m venv .venv
.venv/bin/python -m pip install -r requirements.txt
.venv/bin/python verify_files.py
.venv/bin/python verify_recorded_evidence.py
.venv/bin/python inspect_article_claim.py
```

`verify_files.py` checks the package hashes. `verify_recorded_evidence.py` authenticates the signed packets and checks recorded account continuity, receipt hashes, financial effects, token movements, reserves and rollback. `inspect_article_claim.py` decodes the sequence used in The Account Inversion. The [Nov 4 results](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/evidence/account-evidence/checks/offline-check-20261104.json) record 1,406 signed packets, 272 financial receipts, 115 refusals with rollback and 5,113 recorded-state assertions.
