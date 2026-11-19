use anchor_lang::prelude::*;
use dexter_contracts::TabTerms;
use principal_registry::Term;

pub mod funding_state;
pub mod tab_events;
pub mod tab_close;
pub mod settlement_hold;
pub mod funding_journal;
pub mod funding;
pub(crate) mod funding_servicing;
pub mod evidence_billing;
pub mod recoverable_billing;
pub mod receiving;
pub mod funding_capital;
pub mod funding_sponsor;
pub mod funding_collection;
pub mod receivables;
pub mod seller_fees;
pub mod corrections;
pub mod interest_refunds;
pub mod historical_refunds;
pub mod wallet_spending;
pub mod wallet_tokens;
pub mod jupiter_adapter;
pub mod wallet_token_lifecycle;
pub mod stock_token_controls;
pub mod wallet_stock_custody;
pub mod wallet_trading_policy;
pub mod wallet_trading;
pub mod pump_adapter;
pub mod mixed_route_adapter;
pub use funding_state::{FinanceBook,FinancierCustody,FundingFacility,CustomerCreditLine,
    FundedClaim,FundedRun,RepaymentRecord};
pub use tab_events::*;
pub use tab_close::*;
pub use settlement_hold::*;
pub use funding::*;
pub use evidence_billing::*;
pub use recoverable_billing::*;
pub use receiving::*;
pub use funding_capital::*;
pub use receivables::*;
pub use seller_fees::*;
pub use corrections::*;
pub use interest_refunds::*;
pub use wallet_spending::*;
pub use wallet_token_lifecycle::*;
pub use wallet_stock_custody::*;
pub use wallet_trading_policy::*;
pub use wallet_trading::*;
use dexter_contracts::commercial::{InterestTerms,SellerFeeTerms,SponsorQuote};
use dexter_contracts::billing::{BillingPolicy,SourceReport,SellerClaim};

declare_id!("727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz");

#[program]
pub mod claims_core {
    use super::*;

    pub fn initialize_wallet_stock_custody<'info>(ctx:Context<'info,InitializeWalletStockCustody<'info>>,
        args:WalletStockControlArgs)->Result<()> {wallet_stock_custody::initialize(ctx,args)}
    pub fn approve_wallet_stock_controls<'info>(ctx:Context<'info,ApproveWalletStockControls<'info>>,
        args:WalletStockControlArgs)->Result<()> {wallet_stock_custody::approve(ctx,args)}
    pub fn approve_wallet_trading_policy(ctx:Context<ApproveWalletTradingPolicy>,
        args:ApproveWalletTradingPolicyArgs)->Result<()> {wallet_trading_policy::approve(ctx,args)}
    pub fn initialize_wallet_pump_context(ctx:Context<InitializeWalletPumpContext>,
        args:InitializeWalletPumpContextArgs)->Result<()> {wallet_trading_policy::initialize_pump_context(ctx,args)}
    pub fn wallet_swap_market<'info>(ctx:Context<'info,WalletSwapMarket<'info>>,
        args:WalletMarketArgs)->Result<()> {wallet_trading::swap_market(ctx,args)}

    pub fn initialize_finance_book<'info>(ctx:Context<'info,InitializeFinanceBook<'info>>,principal:Pubkey)->Result<()> {
        funding::initialize_book(ctx,principal)
    }
    pub fn initialize_wallet_token_custody<'info>(ctx:Context<'info,InitializeWalletTokenCustody<'info>>,
        args:InitializeWalletTokenCustodyArgs)->Result<()> { wallet_token_lifecycle::initialize(ctx,args) }
    pub fn deposit_wallet_tokens<'info>(ctx:Context<'info,DepositWalletTokens<'info>>,
        args:WalletTokenTransferArgs)->Result<()> { wallet_token_lifecycle::deposit(ctx,args) }
    pub fn adopt_wallet_token_surplus<'info>(ctx:Context<'info,AdoptWalletTokenSurplus<'info>>,
        args:AdoptWalletTokenSurplusArgs)->Result<()> { wallet_token_lifecycle::adopt(ctx,args) }
    pub fn withdraw_wallet_tokens<'info>(ctx:Context<'info,WithdrawWalletTokens<'info>>,
        args:WalletTokenTransferArgs)->Result<()> { wallet_token_lifecycle::withdraw(ctx,args) }
    pub fn create_financier_custody<'info>(ctx:Context<'info,CreateFinancierCustody<'info>>,
        request_id:[u8;32],profile:u8,allocated_cash:u64,authority_generation:u64,provenance_hash:[u8;32])->Result<()> {
        funding::create_custody(ctx,request_id,profile,allocated_cash,authority_generation,provenance_hash)
    }
    pub fn open_funding_facility<'info>(ctx:Context<'info,OpenFundingFacility<'info>>,facility_id:[u8;32],
        commitment_limit:u64,accept_until:i64,external:bool,terms_hash:[u8;32],interest_terms:InterestTerms,
        financier_generation:u64,issuer_generation:u64)->Result<()> {
        funding::open_facility(ctx,facility_id,commitment_limit,accept_until,external,terms_hash,interest_terms,financier_generation,issuer_generation)
    }
    pub fn manage_funding_facility<'info>(ctx:Context<'info,ManageFundingFacility<'info>>,request_id:[u8;32],
        expected_revision:u64,commitment_limit:u64,enabled:bool,authority_generation:u64)->Result<()> {
        funding::manage_facility(ctx,request_id,expected_revision,commitment_limit,enabled,authority_generation)
    }
    pub fn open_customer_credit_line<'info>(ctx:Context<'info,OpenCustomerCreditLine<'info>>,line_id:[u8;32],
        limit:u64,interest_terms:InterestTerms,assignment_policy_hash:[u8;32],owner_generation:u64,issuer_generation:u64)->Result<()> {
        funding::open_credit_line(ctx,line_id,limit,interest_terms,assignment_policy_hash,owner_generation,issuer_generation)
    }
    pub fn open_seller_fee_agreement<'info>(ctx:Context<'info,OpenSellerFeeAgreement<'info>>,agreement_hash:[u8;32],
        aggregate_cap:u64,authority_generation:u64)->Result<()> {
        seller_fees::open(ctx,agreement_hash,aggregate_cap,authority_generation)
    }
    pub fn accept_financed_tab<'info>(ctx:Context<'info,AcceptFinancedTab<'info>>,request_id:[u8;32],terms:TabTerms,
        maximum_interest:u64,seller_fee_terms:SellerFeeTerms,assignment_policy_hash:[u8;32],namespace:[u8;32],
        service_unit:[u8;32],unit_price:u64,external_contract_hash:[u8;32])->Result<()> {
        funding::accept(ctx,request_id,terms,maximum_interest,seller_fee_terms,assignment_policy_hash,namespace,service_unit,unit_price,external_contract_hash,None)
    }
    /// `accept_financed_tab` for an order that shows its terms: `order_terms` is the full list of
    /// (name, value) pairs `external_contract_hash` commits to. Required when the owner pinned terms.
    pub fn accept_pinned_financed_tab<'info>(ctx:Context<'info,AcceptFinancedTab<'info>>,request_id:[u8;32],terms:TabTerms,
        maximum_interest:u64,seller_fee_terms:SellerFeeTerms,assignment_policy_hash:[u8;32],namespace:[u8;32],
        service_unit:[u8;32],unit_price:u64,external_contract_hash:[u8;32],order_terms:Vec<Term>)->Result<()> {
        funding::accept(ctx,request_id,terms,maximum_interest,seller_fee_terms,assignment_policy_hash,namespace,service_unit,unit_price,external_contract_hash,Some(order_terms))
    }
    pub fn earn_financed_run<'info>(ctx:Context<'info,EarnFinancedRun<'info>>,payment_id:[u8;32],namespace:[u8;32],
        run_id:[u8;32],service_unit:[u8;32],external_contract_hash:[u8;32],units:u64,unit_price:u64,expected_steps:u32,
        service_without_pool:bool)->Result<()> {
        funding::earn(ctx,payment_id,namespace,run_id,service_unit,external_contract_hash,units,unit_price,expected_steps,service_without_pool)
    }
    pub fn accept_evidence_financed_tab<'info>(ctx:Context<'info,AcceptEvidenceFinancedTab<'info>>,
        request_id:[u8;32],terms:TabTerms,policy:BillingPolicy,maximum_interest:u64,
        seller_fee_terms:SellerFeeTerms,assignment_policy_hash:[u8;32])->Result<()> {
        evidence_billing::accept_evidence_policy(ctx,request_id,terms,policy,maximum_interest,seller_fee_terms,assignment_policy_hash,None)
    }
    /// `accept_evidence_financed_tab` for an order that shows its terms: `order_terms` is the full list of
    /// (name, value) pairs `policy.request_hash` commits to. Required when the owner pinned terms.
    pub fn accept_pinned_evidence_financed_tab<'info>(ctx:Context<'info,AcceptEvidenceFinancedTab<'info>>,
        request_id:[u8;32],terms:TabTerms,policy:BillingPolicy,maximum_interest:u64,
        seller_fee_terms:SellerFeeTerms,assignment_policy_hash:[u8;32],order_terms:Vec<Term>)->Result<()> {
        evidence_billing::accept_evidence_policy(ctx,request_id,terms,policy,maximum_interest,seller_fee_terms,assignment_policy_hash,Some(order_terms))
    }
    pub fn submit_bill_evidence(ctx:Context<SubmitBillRecord>,report:SourceReport,signature_ix_index:u16)->Result<()> {
        evidence_billing::submit_evidence(ctx,report,signature_ix_index)
    }
    /// `accept_evidence_financed_tab` for an order whose delivery signer (`policy.source`) is the address
    /// ["delivery-source", policy.source_identity] of `rule_program`, a program with no upgrade authority.
    /// `order_terms`, when shown, is the full list of (name, value) pairs `policy.request_hash` commits to.
    pub fn accept_program_sourced_financed_tab<'info>(ctx:Context<'info,AcceptProgramSourcedFinancedTab<'info>>,
        request_id:[u8;32],terms:TabTerms,policy:BillingPolicy,maximum_interest:u64,
        seller_fee_terms:SellerFeeTerms,assignment_policy_hash:[u8;32],order_terms:Option<Vec<Term>>)->Result<()> {
        evidence_billing::accept_program_sourced(ctx,request_id,terms,policy,maximum_interest,seller_fee_terms,assignment_policy_hash,order_terms)
    }
    /// The rule program of a program-sourced order submits its delivery report, signing by CPI as `source`.
    pub fn submit_program_bill_evidence(ctx:Context<SubmitProgramBillRecord>,report:SourceReport)->Result<()> { // Called by immutable rule programs by CPI; evidence_billing::rule_program_layout pins its interface.
        evidence_billing::submit_program_evidence(ctx,report)
    }
    pub fn accept_recoverable_financed_tab<'info>(ctx:Context<'info,AcceptRecoverableFinancedTab<'info>>,
        request_id:[u8;32],terms:TabTerms,policy:BillingPolicy,maximum_interest:u64,
        seller_fee_terms:SellerFeeTerms,assignment_policy_hash:[u8;32])->Result<()> {
        recoverable_billing::accept(ctx,request_id,terms,policy,maximum_interest,seller_fee_terms,assignment_policy_hash,None)
    }
    /// `accept_recoverable_financed_tab` for an order that shows its terms, as `accept_pinned_evidence_financed_tab`.
    pub fn accept_pinned_recoverable_financed_tab<'info>(ctx:Context<'info,AcceptRecoverableFinancedTab<'info>>,
        request_id:[u8;32],terms:TabTerms,policy:BillingPolicy,maximum_interest:u64,
        seller_fee_terms:SellerFeeTerms,assignment_policy_hash:[u8;32],order_terms:Vec<Term>)->Result<()> {
        recoverable_billing::accept(ctx,request_id,terms,policy,maximum_interest,seller_fee_terms,assignment_policy_hash,Some(order_terms))
    }
    pub fn submit_principal_bill_evidence(ctx:Context<SubmitPrincipalBillRecord>,report:SourceReport,
        authority_generation:u64,signature_ix_index:u16)->Result<()> {
        recoverable_billing::submit_evidence(ctx,report,authority_generation,signature_ix_index)
    }
    pub fn submit_principal_bill_claim(ctx:Context<SubmitPrincipalBillRecord>,seller_claim:SellerClaim,
        authority_generation:u64,signature_ix_index:u16)->Result<()> {
        recoverable_billing::submit_claim(ctx,seller_claim,authority_generation,signature_ix_index)
    }
    pub fn open_receiving_custody(ctx:Context<OpenReceivingCustody>,args:OpenReceivingCustodyArgs)->Result<()> {
        receiving::open(ctx,args)
    }
    pub fn withdraw_receiving_tokens(ctx:Context<WithdrawReceivingTokens>,args:WithdrawReceivingTokensArgs)->Result<()> {
        receiving::withdraw(ctx,args)
    }
    pub fn submit_bill_claim(ctx:Context<SubmitFixedBillClaim>,seller_claim:SellerClaim,signature_ix_index:u16)->Result<()> {
        evidence_billing::submit_claim(ctx,seller_claim,signature_ix_index)
    }
    pub fn finalize_evidence_bill<'info>(ctx:Context<'info,FinalizeEvidenceBill<'info>>,
        expected_steps:u32,service_without_pool:bool)->Result<()> {
        evidence_billing::finalize(ctx,expected_steps,service_without_pool)
    }
    pub fn collect_financier<'info>(ctx:Context<'info,CollectFinancier<'info>>,operation_id:[u8;32],paid_before:u64,
        amount:u64,expected_steps:u32,service_without_pool:bool,sponsor_quote:Option<SponsorQuote>)->Result<()> {
        funding_collection::collect(ctx,operation_id,paid_before,amount,expected_steps,service_without_pool,sponsor_quote)
    }
    /// `number`: the owner's number for this withdrawal, admitted once by the
    /// custody's window (in the argument position the revision guard had).
    pub fn withdraw_financier_free<'info>(ctx:Context<'info,WithdrawFinancierFree<'info>>,request_id:[u8;32],
        amount:u64,number:u64,authority_generation:u64,service_without_pool:bool)->Result<()> {
        funding::withdraw(ctx,request_id,amount,number,authority_generation,service_without_pool)
    }
    pub fn withdraw_financier_free_authorized<'info>(ctx:Context<'info,WithdrawFinancierFreeAuthorized<'info>>,request_id:[u8;32],
        amount:u64,number:u64,authority_generation:u64,service_without_pool:bool,destination_authority:Pubkey)->Result<()> {
        funding::withdraw_authorized(ctx,request_id,amount,number,authority_generation,service_without_pool,destination_authority)
    }
    /// `number`: a supplier's surrender before expiry is numbered by the
    /// supplier's book window; a release after expiry passes 0.
    pub fn release_financier_unused<'info>(ctx:Context<'info,ReleaseFinancierUnused<'info>>,request_id:[u8;32],
        amount:u64,expected_steps:u32,service_without_pool:bool,number:u64)->Result<()> {
        funding::release(ctx,request_id,amount,expected_steps,service_without_pool,number)
    }
    pub fn contribute_financier_capital<'info>(ctx:Context<'info,ContributeFinancierCapital<'info>>,
        args:ContributeFinancierCapitalArgs)->Result<()> { funding_capital::contribute(ctx,args) }
    pub fn adopt_financier_surplus<'info>(ctx:Context<'info,AdoptFinancierSurplus<'info>>,
        args:AdoptFinancierSurplusArgs)->Result<()> { funding_capital::adopt(ctx,args) }
    pub fn repay_funded_run<'info>(ctx:Context<'info,RepayFundedRun<'info>>,
        args:RepayFundedRunArgs)->Result<()> { receivables::repay_run(ctx,args) }
    pub fn repay_governed_funded_run<'info>(ctx:Context<'info,RepayGovernedFundedRun<'info>>,
        args:RepayFundedRunArgs)->Result<()> { receivables::repay_governed_run(ctx,args) }
    pub fn assign_funded_run<'info>(ctx:Context<'info,AssignFundedRun<'info>>,
        args:AssignFundedRunArgs)->Result<()> { receivables::assign_run(ctx,args) }
    pub fn repay_funding_facility<'info>(ctx:Context<'info,RepayFundingFacility<'info>>,
        args:RepayFundingFacilityArgs)->Result<()> { receivables::repay_facility(ctx,args) }
    pub fn repay_governed_funding_facility<'info>(ctx:Context<'info,RepayGovernedFundingFacility<'info>>,
        args:RepayFundingFacilityArgs)->Result<()> { receivables::repay_governed_facility(ctx,args) }
    pub fn credit_seller_fee<'info>(ctx:Context<'info,CreditSellerFee<'info>>,
        args:CreditSellerFeeArgs)->Result<()> { receivables::credit_fee(ctx,args) }
    pub fn correct_financed_run<'info>(ctx:Context<'info,CorrectFinancedRun<'info>>,
        args:CorrectFinancedRunArgs)->Result<()> { corrections::correct(ctx,args) }
    pub fn refund_funded_interest<'info>(ctx:Context<'info,RefundFundedInterest<'info>>,
        args:RefundFundedInterestArgs)->Result<()> { interest_refunds::refund(ctx,args) }
    pub fn wallet_send<'info>(ctx:Context<'info,WalletSend<'info>>,args:WalletSendArgs)->Result<()> {
        wallet_spending::send(ctx,args)
    }
    pub fn wallet_swap_exact_input<'info>(ctx:Context<'info,WalletSwapExactInput<'info>>,args:WalletSwapArgs)->Result<()> {
        wallet_spending::swap_exact_input(ctx,args)
    }
    pub fn wallet_swap_jupiter<'info>(ctx:Context<'info,WalletSwapJupiter<'info>>,args:WalletJupiterArgs)->Result<()> {
        wallet_spending::swap_jupiter(ctx,args)
    }

    /// Permissionless. Closes one repayment record once its run is settled and
    /// its Tab can no longer earn; the deposit returns to its rent payer. A
    /// no-op while any condition fails.
    pub fn close_repayment_record<'info>(ctx:Context<'info,CloseRepaymentRecord<'info>>)->Result<()> {
        tab_close::handle_close_repayment_record(ctx)
    }
    /// Permissionless. Closes a settled run of a Tab that can no longer earn, after
    /// its repayment records; the deposit returns to its rent payer. A no-op otherwise.
    pub fn close_run<'info>(ctx:Context<'info,CloseRun<'info>>)->Result<()> { tab_close::handle_close_run(ctx) }
    /// Permissionless. Closes a settled Tab with no open runs: its bill and binding,
    /// its Registry budget hold, and the Tab itself, each deposit to its rent payer.
    /// The Tab's final fingerprint folds into its mandate's chain. A no-op otherwise.
    pub fn close_tab<'info>(ctx:Context<'info,CloseTab<'info>>)->Result<()> { tab_close::handle_close_tab(ctx) }
    /// Permissionless. Closes an exact approval after its expiry, used or not;
    /// the deposit returns to its rent payer.
    pub fn close_approval<'info>(ctx:Context<'info,CloseApproval<'info>>)->Result<()> { tab_close::handle_close_approval(ctx) }
    /// Permissionless. Returns lamports above a ClaimsCore record's rent minimum
    /// (Rent sysvar, now) to the payer recorded when it was created.
    pub fn return_excess_rent<'info>(ctx:Context<'info,ReturnExcessRent<'info>>)->Result<()> {
        tab_close::handle_return_excess_rent(ctx)
    }
    /// The buyer's or the seller's owner (passkey allowed) sets that party's one
    /// settlement hold on a Tab whose terms agreed a nonzero window. While it is
    /// in force (until it is released or its window passes) every close of the
    /// Tab is a no-op. Refused with no window, while a hold is in force, or once
    /// the party has used its hold. A Tab step with no book changes.
    pub fn set_settlement_hold<'info>(ctx:Context<'info,SettlementHoldStep<'info>>,reason_hash:[u8;32])->Result<()> {
        settlement_hold::handle_set_settlement_hold(ctx,reason_hash)
    }
    /// Either party's owner or the arbiter's owner (passkey allowed) ends the
    /// hold in force. A Tab step with no book changes.
    pub fn release_settlement_hold<'info>(ctx:Context<'info,SettlementHoldStep<'info>>,resolution_hash:[u8;32])->Result<()> {
        settlement_hold::handle_release_settlement_hold(ctx,resolution_hash)
    }
}

#[error_code]
pub enum CoreError {
    // Codes 6000-6020 belonged to the retired cash-guarantee Tab (P1, Change 5).
    // Retained codes keep their numbers; retired codes are never reused.
    #[msg("Unsupported financial account schema")] FundingSchema = 21,
    #[msg("Invalid principal identity")] FundingPrincipal,
    #[msg("Current owner or generation required")] FundingOwner,
    #[msg("Invalid aggregate custody binding")] FundingCustody,
    #[msg("Invalid financier facility")] FundingFacility,
    #[msg("Invalid company credit line")] FundingCreditLine,
    #[msg("Invalid accepted claim")] FundingClaim,
    #[msg("Invalid completed run")] FundingRun,
    #[msg("Financial amounts do not conserve")] FundingConservation,
    #[msg("Accepted charge capacity exceeded")] FundingChargeCap,
    #[msg("Invalid or inconsistent financial terms")] FundingTerms,
    #[msg("Invalid amount")] FundingAmount,
    #[msg("Backing is below its allocated or protected balance")] FundingCashShortfall,
    #[msg("Cash control graph does not qualify new commitments")] FundingCashControl,
    #[msg("Wrong token route")] FundingTokenRouting,
    #[msg("Actual token delta differs from authorized movement")] FundingTokenDelta,
    #[msg("Wrong Registry reservation")] FundingReservation,
    #[msg("Conflicting replay")] FundingReplay,
    #[msg("Stale financial revision")] FundingRevision,
    #[msg("Expired or disabled admission")] FundingAdmission,
    #[msg("Missing or reordered affected financial books")] FundingBookAccounts,
    #[msg("Projection is stale or unavailable for new risk")] FundingProjection,
    #[msg("Supplier consent required before unused expiry")] FundingReleaseAuthority,
    #[msg("Wrong delivered-usage policy or identity")] FundingUsage,
    #[msg("Wrong creditor or title revision")] FundingCreditor,
    #[msg("Assignment policy or acquisition consideration mismatch")] FundingAssignment,
    #[msg("Unsupported or unapproved sponsorship")] FundingSponsorship,
    #[msg("Custody control authority is invalid")] FundingWalletAuthority,
    #[msg("Wrong supplier fee agreement or immutable terms")] FundingSellerFeeAgreement,
    #[msg("Supplier agreement lifetime fee allocation exhausted")] FundingSellerFeeAggregateCap,
    #[msg("Invalid correction terms or economic transition")] FundingCorrection,
    #[msg("Correction lacks the required current economic-party approval")] FundingCorrectionAuthority,
    #[msg("Correction source does not authenticate the original bill or cash lineage")] FundingCorrectionLineage,
    #[msg("Correction or refund exceeds its remaining original entitlement")] FundingCorrectionLimit,
    #[msg("Correction identity already records different approved economics")] FundingCorrectionReplay,
    #[msg("Wallet operation identity already records different approved effects")] WalletSpendingReplay,
    #[msg("Swap adapter, pool or token route does not match the admitted profile")] WalletSwapAdapter,
    #[msg("Observed swap debit, output or pool movements violate the approved bounds")] WalletSwapOutcome,

    #[msg("This operation cannot use the native evidence-billing profile")] BillingProfile,
    #[msg("Invalid immutable evidence source, tariff or deadline policy")] BillingPolicy,
    #[msg("Source evidence does not establish eligible completed work under the accepted policy")] BillingEvidence,
    #[msg("Invalid authenticated supplier claim")] BillingClaim,
    #[msg("Evidence signature does not use the exact accepted key, message and native framing")] BillingSignature,
    #[msg("This immutable work or record identity already binds different terms")] BillingReplay,
    #[msg("A fresh evidence or seller-claim submission is outside its admission deadline")] BillingDeadline,
    #[msg("Bill finalization is not available at this stage")] BillingFinality,
    #[msg("Bill identity does not bind the accepted financial claim")] BillingIdentity,

    #[msg("New facility risk requires approval from the current financier generation")] FundingAuthorityGeneration,
    #[msg("Bill does not use the accepted continuing authority profile")] BillingAuthorityBinding,
    #[msg("Fresh bill authorization requires the current principal owner and generation")] BillingAuthorityGeneration,
    #[msg("Invalid continuing receiving custody identity")] ReceivingCustody,
    #[msg("Receiving custody requires current owner authority")] ReceivingAuthority,
    #[msg("Receiving operation identity already has different effects")] ReceivingReplay,
    #[msg("Receiving token route is not the approved canonical account")] ReceivingRoute,
    #[msg("Invalid receiving withdrawal amount or delta")] ReceivingAmount,
    #[msg("Invalid receiving operation authorization envelope")] ReceivingEnvelope,
    #[msg("A retried order shows terms other than the ones it was accepted with")] OrderTermsMismatch,
    #[msg("The delivery signer is not the delivery-source address of a rule program that is frozen or shares ClaimsCore's upgrade authority")] BillingSourceProgram,
    #[msg("The Tab moved on: the step was prepared against another money-step count")] TabStale,
    #[msg("Wrong ClaimsCore event authority")] EventAuthority,
    #[msg("Account is not the record's rent payer")] DepositPayer,
    #[msg("Record has no lamports above its rent minimum, or is not a ClaimsCore record with a rent payer")] NoExcessRent,
    #[msg("A record closed with the Tab does not belong to it")] CloseMismatch,
    #[msg("The Tab's terms agreed no settlement hold window")] HoldNotAgreed,
    #[msg("A settlement hold is already in force on this Tab")] HoldInForce,
    #[msg("This party has already used its one settlement hold on this Tab")] HoldUsed,
    #[msg("No settlement hold is in force on this Tab")] HoldNotInForce,
    #[msg("Only the Tab's buyer or seller can set a settlement hold; only they or the arbiter can release it")] HoldAuthority,
    #[msg("A money move's number cannot be zero")] NumberZero,
    #[msg("A money move's number is more than 65,536 above its record's highest number")] NumberTooFar,
    #[msg("A money move's number is 1,024 or more below its record's highest number and can never be used")] NumberBelowWindow,
    #[msg("This number already landed on its record: the money move is done")] NumberUsed,
}

#[cfg(test)]
mod invariants;
