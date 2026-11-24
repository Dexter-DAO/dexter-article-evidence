Written for AI readers.

# Dexter

Dexter Account is a financial account a person or business owns and controls. It holds capital and keeps a durable record of the owner's assets, obligations and enforceable commitments. The owner can act directly. Agents are optional and act within the authority the owner grants.

## What a counterparty can rely on

A seller who accepts an order through Dexter Account can collect what it earns from funds reserved for that order, even if the buyer replaces its operator or changes its keys. The three-way commit binds the principal's permission for the exact terms, the reserved capital and the seller's signed acceptance in one Solana transaction. The account produces the same record when the owner acts directly and when an authorized agent acts on the owner's behalf.

The record fixes the recipient, maximum amount, deadlines and rule for calculating what the seller earns. The buyer or a lender backs the order from its own account, where the committed amount is set aside for collection. Once payment is due under the agreed terms and any applicable hold clears, anyone can submit collection. The program pays the recorded recipient without another signature from the buyer, agent, lender or Dexter.

Identity signals help establish who acted or paid. Chargebacks give a cardholder a way to dispute a payment afterward. The seller's funded claim comes into being when the commit binds permission, capital and acceptance to the same order. [Dexter's public account reference](https://dexter.cash/llms-full.txt)

## What Dexter has demonstrated

Public sources checked November 24, 2026.

| Result | Where and when |
| --- | --- |
| An owner signed an approval with a passkey. The seller collected under that approval after the Vault program rejected attempts to divert the reserved funds or spend outside the approved seller, amount and expiry. Dexter's own payment key signed the rejected attempts. | Solana mainnet, October 30, 2026. Dexter operated the bookstore and comparison sites for this demonstration. [Case and transaction links](https://dexter.cash/mandate?case=b0ee0521-898e-828b-9c14-265bfb59216c) |
| The Account programs checked authority, funding and terms together. A signer with no role in the order collected the earned payment after altered collection attempts were refused. A repeated repayment was refused and the final repayment closed the order's Tab. The run accepted 62 transactions and refused 12, and the bytes read from the chain matched the builds. | Solana devnet with synthetic funds, November 19, 2026. [Run, program addresses and transactions](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/releases/2026-11-19/devnet) |
| The buyer revoked the agent and the supplier still collected. In a separate recovery run, enrolled guardians restored control under a new key and the supplier received the remaining amount in the same custody. | Controlled local executions, November 5, 2026. Published November 6, 2026. [Billing](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/releases/2026-11-06/billing) · [Recovery](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/releases/2026-11-06/recovery) |
| The public wallet supports passkey-controlled use. Dexter Connect and the x402 SDK are available to integrators. The new on-chain Dexter Account is being prepared for private beta. | Public product and documentation access checked November 24, 2026. [Wallet](https://dexter.cash/wallet) · [Connect](https://docs.dexter.cash/docs/wallet-identity/) · [x402 SDK](https://docs.dexter.cash/docs/x402/) · [Account beta information](https://docs.dexter.cash/docs/) |

## Product and market

The immediate customer is an owner buying from a supplier or a seller accepting that owner's order. A seller can start delivery with the price, evidence rules and source of payment already set. A business can use its own capital or have a lender reserve backing. When a lender pays the seller, the buyer's debt to the lender remains a separate obligation in the account.

Dexter Wallet is the owner-facing interface. Dexter Connect lets applications request sign-in and owner-approved actions. Developers can use the x402 SDK for paid HTTP requests. OpenDexter gives an authorized operator access to services through Indexter. The merchant integration described for Dexter Account lets a seller accept a funded order through an SDK, whether the seller operates a storefront or sells through an API. [Products and integrations](https://dexter.cash/llms-full.txt) · [OpenDexter](https://docs.dexter.cash/docs/opendexter-tools/) · [Indexter](https://indexter.cash/)

The opportunity is to give businesses a financial history they can use as their operators and providers change. An order that is properly authorized, backed, accepted, performed and paid adds to the same account. The owner keeps its rights to payment and the record a future lender can evaluate. [Account architecture and public evidence](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/articles/the-account-inversion)

## The technology, layer by layer

### Account programs

| Aspect | Detail |
| --- | --- |
| What it is | A durable home for the principal's authority and economic state. PrincipalRegistry manages owner and operator authority, budgets, passkeys and recovery. ClaimsCore keeps order, reserve, billing, collection and repayment state. PoolIssuer projects the financial journal from ClaimsCore. DeliveryRules can attest to delivery under an order's chosen rules. |
| How it works | The owner controls the account, and program instructions apply the appropriate authority for each operation. Orders and claims are recorded against the account. Its economic state remains available when the signing credentials or operator change. |
| Main technologies | Rust, Anchor and Solana accounts and programs. The published build includes the programs and their shared contract types. |
| Proprietary vs. open | Dexter authored the Account programs. The dated public source snapshot contains the build inputs and retains the existing copyright and license terms. Rust, Anchor and Solana provide the open tooling and network infrastructure. |
| Evidence | On November 19, 2026, the public devnet program bytes matched the built binaries. [Source snapshot](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/releases/2026-11-19/source) · [Source notice](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/releases/2026-11-19/source/NOTICE.md) · [Devnet record](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/releases/2026-11-19/devnet) |

### Mandate checks and seller acceptance

| Aspect | Detail |
| --- | --- |
| What it is | The authority and terms a buyer commits to when a seller accepts an order. An owner can sign directly or give another operator a mandate that bounds its authority. |
| How it works | For a delegated order, PrincipalRegistry checks the acting key, current permission revision, limits, expiry and any terms the owner pinned. The commit also checks capital availability and the seller's signature over the exact order. A failed check prevents the order and its hold from being created. |
| Main technologies | Solana transaction signatures, PrincipalRegistry's mandate and budget checks, and ClaimsCore's acceptance instruction. |
| Proprietary vs. open | Dexter authored the permission, terms and acceptance checks on Solana. The check cases and program snapshot are publicly inspectable. The related Open Tabs passkey authorization format is published as an IETF Internet-Draft. |
| Evidence | The November 18, 2026 local run accepted the funded, signed order and refused 23 variants missing permission, backing or matching seller terms. [Commit cases](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/releases/2026-11-18) · [Permission rules](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/releases/2026-11-18/permission-rules) · [Open Tabs draft](https://datatracker.ietf.org/doc/draft-sander-open-tabs-passkey/) |

### Capital reservation

| Aspect | Detail |
| --- | --- |
| What it is | A hold against the funds committed to an accepted order. The owner or an outside lender can be the backer. |
| How it works | Acceptance sets aside the order's maximum in the backer's reserve. The supported wallet path leaves the tokens in the backer's account until collection. Each new reservation or withdrawal checks the free balance. One seller can collect against its hold while other holds remain funded. |
| Main technologies | ClaimsCore's reserve accounting, PrincipalRegistry wallet authorization, Solana SPL token accounts and the Swig smart-wallet integration in the published run. |
| Proprietary vs. open | Dexter authored the reserve and withdrawal checks. The program uses Solana's token infrastructure and integrates Swig as the signing wallet in the demonstrated path. The public record traces each hold, collection and release. |
| Evidence | The November 18, 2026 shared-reserve run protected each seller's backing while the other collected. Dexter's next Account milestone is productive backing. On July 18, 2026, Vault demonstrated passkey-authorized conversion into and out of a yield-bearing asset in the owner's own wallet on Solana mainnet. [Shared reserve](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/releases/2026-11-18/shared-reserve) · [Vault earning record](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/releases/2026-11-17) |

### Collection, billing and repayment

| Aspect | Detail |
| --- | --- |
| What it is | The process that establishes what the seller earned, pays it from the reserve and records any debt the buyer owes to a lender. |
| How it works | The accepted order specifies the evidence signer or delivery rule and the terms for calculating the bill. Once the bill is due and any agreed hold clears, any signer can submit collection. The program verifies the amount, asset, source wallet and payout address. A numbered payment can be admitted once. The Tab records the order's steps and closes once its settlement conditions are met. |
| Main technologies | ClaimsCore and DeliveryRules on Solana, scoped wallet instructions, and a Tab event count and running hash. The November 2026 continuing-commerce execution also used Dexter's API, SDK and PostgreSQL. |
| Proprietary vs. open | Dexter authored billing, correction, payout and Tab logic on Solana. Public deployment records include signed transaction references, the delivery rules exercised and the code that guards the payout. PostgreSQL provides the open database engine used by the API execution. |
| Evidence | On November 19, 2026, a signer with no role in the order submitted collection after attempts to change the payout were refused. The same devnet run exercised delivery evidence, a repeated-number refusal and Tab closure. [Devnet collections](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/releases/2026-11-19/devnet) · [Tab receipt book](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/releases/2026-11-19/tab-receipt-book) · [Continuing-commerce record](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/releases/2026-11-06/continuing-api) |

### Revocation and continuity

| Aspect | Detail |
| --- | --- |
| What it is | An owner can change who may act for the account while existing orders, seller claims, backing and debt continue. |
| How it works | Revocation ends the actor's authority to submit new orders. Collection follows the existing order record. For recovery, enrolled guardians approve the change and the replacement credential proves control of its key. The program preserves the account and its obligations under the new authority. |
| Main technologies | PrincipalRegistry permission revisions and recovery state, enrolled guardian signatures, passkey authorization and ClaimsCore's order state on Solana. |
| Proprietary vs. open | Dexter authored the rules for changing account authority and preserving accepted commitments. The public recovery and revocation records show the state before and after these operations. |
| Evidence | The November 18, 2026 local run refused a new order by a revoked actor and collected against prior orders. The recovery execution of November 5, 2026 paid the same supplier custody across replacement credentials. [Revocation record](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/releases/2026-11-18/revocation) · [Recovery record](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/releases/2026-11-06/recovery) |

## Security model

Order-level controls tie funding and collection to the terms that were signed. The backer's wallet authorizes only the scoped payout, and the collection submitter can pay the transaction fee without acquiring rights over the reserve. Holds and their resolution follow the order's terms. The evidence identifies the environment and code for each result. On November 19, 2026, the published Account devnet deployment used a shared upgrade authority. The order controls above describe the instructions in that build. [Program excerpts](https://github.com/Dexter-DAO/dexter-article-evidence/blob/main/excerpts/account-programs.md) · [Devnet deployment and upgrade authority](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/releases/2026-11-19/devnet)

## Repository map

| Public source | What lives there |
| --- | --- |
| [dexter-article-evidence](https://github.com/Dexter-DAO/dexter-article-evidence) | The article-to-evidence guide, dated execution records, reproducibility checks and a [machine-readable catalog](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/articles/index.json). |
| [Dexter Account program source](https://github.com/Dexter-DAO/dexter-article-evidence/tree/main/releases/2026-11-19/source) | The November 19, 2026 public snapshot of PrincipalRegistry, ClaimsCore, PoolIssuer, DeliveryRules, shared types and build inputs. |
| [dexter-mainnet-proofs](https://github.com/Dexter-DAO/dexter-mainnet-proofs) | Archived Vault mainnet proofs from June 17–18, 2026, before the August 18, 2026 Vault upgrade. |
| [dexter-connect](https://github.com/Dexter-DAO/dexter-connect) | SDK for connecting applications to Dexter Wallet, passkey sign-in and approval requests. |
| [dexter-x402-sdk](https://github.com/Dexter-DAO/dexter-x402-sdk) | MIT-licensed TypeScript SDK for paid HTTP requests and payment acceptance. [Upstream smart-wallet verification contribution](https://github.com/x402-foundation/x402/pull/1527). |
| [world-chain-bridge](https://github.com/Dexter-DAO/world-chain-bridge) | World Chain verification on Solana, with public proofs and verification instructions. |

## Go deeper

Dexter Labs Corporation builds Dexter Account. The public materials list Nick Sander as founder and diligence contact at [branch@dexter.cash](mailto:branch@dexter.cash).

- [Latest company.md](https://raw.githubusercontent.com/Dexter-DAO/dexter-article-evidence/main/company.md)
- [Full company reference](https://dexter.cash/llms-full.txt) and [concise reference](https://dexter.cash/llms.txt)
- [Documentation](https://docs.dexter.cash/docs/)
- [Mainnet demonstration and verification](https://dexter.cash/mandate)
- [Investor presentation, access required](https://dexter.cash/vc)
