use anchor_lang::prelude::*;
use anchor_spl::{associated_token::{self, AssociatedToken}, token::{self, Mint, Token, TokenAccount, TransferChecked}};
use dexter_contracts::{self as contracts, AcceptArgs, StageArgs, TabTerms, Balances, SourceData, EffectData, OperationData,
    SCHEMA_VERSION, METERED_CLAIM_VERSION, RUN_OPERATION_SEED, RUN_RECEIPT_VERSION, ACCEPT, CAPTURE, PAYOUT, REPAY, RELEASE, REGISTRY_CONSUMER_SEED, POOL_DISPATCH_SEED};
use principal_registry::{self, program::PrincipalRegistry, Principal, Actor, Mandate, Budget, Reservation};
use pool_issuer::{self, program::PoolIssuer, PoolBook};

pub mod funding_state;
pub mod funding_journal;
pub mod funding;
pub mod funding_capital;
pub mod funding_sponsor;
pub mod funding_collection;
pub mod receivables;
pub mod seller_fees;
pub mod corrections;
pub mod interest_refunds;
pub mod historical_refunds;
pub mod wallet_spending;
pub use funding_state::{FinanceBook,FinanceReceipt,FinancierCustody,FundingFacility,CustomerCreditLine,
    FundedClaim,FundedRun,FundedPayment,SellerFeeCredits,FacilityPolicyReceipt};
pub use funding::*;
pub use funding_capital::*;
pub use receivables::*;
pub use seller_fees::*;
pub use corrections::*;
pub use interest_refunds::*;
pub use wallet_spending::*;
use dexter_contracts::commercial::{InterestTerms,SellerFeeTerms,SponsorQuote};

declare_id!("727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz");

#[program]
pub mod claims_core {
    use super::*;

    pub fn initialize_finance_book<'info>(ctx:Context<'info,InitializeFinanceBook<'info>>,principal:Pubkey)->Result<()> {
        funding::initialize_book(ctx,principal)
    }
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
        funding::accept(ctx,request_id,terms,maximum_interest,seller_fee_terms,assignment_policy_hash,namespace,service_unit,unit_price,external_contract_hash)
    }
    pub fn earn_financed_run<'info>(ctx:Context<'info,EarnFinancedRun<'info>>,payment_id:[u8;32],namespace:[u8;32],
        run_id:[u8;32],service_unit:[u8;32],external_contract_hash:[u8;32],units:u64,unit_price:u64,expected_revision:u64,
        service_without_pool:bool)->Result<()> {
        funding::earn(ctx,payment_id,namespace,run_id,service_unit,external_contract_hash,units,unit_price,expected_revision,service_without_pool)
    }
    pub fn collect_financier<'info>(ctx:Context<'info,CollectFinancier<'info>>,operation_id:[u8;32],paid_before:u64,
        amount:u64,expected_revision:u64,service_without_pool:bool,sponsor_quote:Option<SponsorQuote>)->Result<()> {
        funding_collection::collect(ctx,operation_id,paid_before,amount,expected_revision,service_without_pool,sponsor_quote)
    }
    pub fn withdraw_financier_free<'info>(ctx:Context<'info,WithdrawFinancierFree<'info>>,request_id:[u8;32],
        amount:u64,expected_revision:u64,authority_generation:u64,service_without_pool:bool)->Result<()> {
        funding::withdraw(ctx,request_id,amount,expected_revision,authority_generation,service_without_pool)
    }
    pub fn release_financier_unused<'info>(ctx:Context<'info,ReleaseFinancierUnused<'info>>,request_id:[u8;32],
        amount:u64,expected_revision:u64,service_without_pool:bool)->Result<()> {
        funding::release(ctx,request_id,amount,expected_revision,service_without_pool)
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

    pub fn initialize_source(ctx: Context<InitializeSource>) -> Result<()> {
        if ctx.accounts.source.data.version != 0 {
            let data = &ctx.accounts.source.data;
            require!(data.version == SCHEMA_VERSION && data.pool == ctx.accounts.pool.key()
                && data.mint == ctx.accounts.mint.key() && data.escrow == ctx.accounts.escrow.key()
                && data.treasury == ctx.accounts.treasury.key(), CoreError::SourceMismatch);
            return Ok(());
        }
        require_keys_eq!(ctx.accounts.pool.source, ctx.accounts.source.key(), CoreError::SourceMismatch);
        ctx.accounts.source.data = SourceData {
            version: SCHEMA_VERSION, bump: ctx.bumps.source, pool: ctx.accounts.pool.key(),
            mint: ctx.accounts.mint.key(), escrow: ctx.accounts.escrow.key(), treasury: ctx.accounts.treasury.key(),
            balances: Balances::default(), sequence: 0, head: [0; 32],
        };
        Ok(())
    }

    pub fn accept_tab(ctx: Context<AcceptTab>, args: AcceptArgs) -> Result<()> {
        accept_with_profile(ctx, args, SCHEMA_VERSION)
    }

    /// Buyer credential and issuer sign the full quantity × unit-price policy through accepted Tab terms.
    pub fn accept_metered_tab(ctx: Context<AcceptTab>, args: AcceptArgs, namespace: [u8; 32],
        service_unit: [u8; 32], unit_price: u64, external_contract_hash: [u8; 32]) -> Result<()> {
        validate_usage_policy(&args.terms, &namespace, &service_unit, unit_price, &external_contract_hash)?;
        accept_with_profile(ctx, args, METERED_CLAIM_VERSION)
    }

    pub fn capture(ctx: Context<Capture>, args: StageArgs) -> Result<()> {
        let mut args = args;
        args.operation_id = contracts::signed_request_id(ctx.accounts.supplier.key(), &args.operation_id);
        require!(ctx.accounts.claim.version == SCHEMA_VERSION, CoreError::CaptureProfile);
        let digest = contracts::operation_digest(&ctx.accounts.claim.terms, CAPTURE,
            &args.operation_id, args.amount, args.service_without_pool)?;
        if replay(&ctx.accounts.operation, &digest, SCHEMA_VERSION)? { return Ok(()); }
        CaptureExecution::from(ctx.accounts).apply_capture(args, digest, ctx.bumps.registry_authority, ctx.bumps.dispatcher)
    }

    /// The accepted supplier attests one completed run and its measured quantity.
    pub fn capture_completed_run(ctx: Context<CaptureCompletedRun>, args: StageArgs,
        namespace: [u8; 32], run_id: [u8; 32], service_unit: [u8; 32], external_contract_hash: [u8; 32],
        payment_id: [u8; 32], units: u64, unit_price: u64) -> Result<()> {
        apply_completed_run(ctx, args, namespace, run_id, service_unit, external_contract_hash, payment_id, units, unit_price)
    }

    pub fn payout(ctx: Context<Payout>, args: StageArgs) -> Result<()> {
        let mut args = args;
        args.operation_id = contracts::signed_request_id(ctx.accounts.payer.key(), &args.operation_id);
        let digest = contracts::operation_digest(&ctx.accounts.claim.terms, PAYOUT, &args.operation_id, args.amount, args.service_without_pool)?;
        if replay(&ctx.accounts.operation, &digest, SCHEMA_VERSION)? { return Ok(()); }
        validate_claim(&ctx.accounts.claim, &ctx.accounts.source, ctx.accounts.mint.key(), args.expected_revision)?;
        require!(args.amount > 0 && args.amount <= ctx.accounts.claim.payable, CoreError::Amount);
        require_keys_eq!(ctx.accounts.escrow.key(), ctx.accounts.source.data.escrow, CoreError::TokenRouting);
        let destination = associated_token::get_associated_token_address(&ctx.accounts.claim.terms.supplier, &ctx.accounts.mint.key());
        require_keys_eq!(ctx.accounts.destination.key(), destination, CoreError::TokenRouting);
        require_keys_eq!(ctx.accounts.destination.owner, ctx.accounts.claim.terms.supplier, CoreError::TokenRouting);
        let before = ctx.accounts.escrow.amount;
        let destination_before = ctx.accounts.destination.amount;
        require!(before >= ctx.accounts.source.data.balances.protected_cash, CoreError::CashDeficit);
        let pool = ctx.accounts.source.data.pool;
        let mint = ctx.accounts.source.data.mint;
        let bump = [ctx.accounts.source.data.bump];
        let seeds: &[&[u8]] = &[b"source", pool.as_ref(), mint.as_ref(), &bump];
        token::transfer_checked(CpiContext::new_with_signer(ctx.accounts.token_program.key(), TransferChecked {
            from: ctx.accounts.escrow.to_account_info(), mint: ctx.accounts.mint.to_account_info(),
            to: ctx.accounts.destination.to_account_info(), authority: ctx.accounts.source.to_account_info(),
        }, &[seeds]), args.amount, ctx.accounts.mint.decimals)?;
        ctx.accounts.escrow.reload()?;
        ctx.accounts.destination.reload()?;
        require!(ctx.accounts.escrow.amount == contracts::minus(before, args.amount)?
            && ctx.accounts.destination.amount == contracts::plus(destination_before, args.amount)?, CoreError::TokenDelta);
        ctx.accounts.claim.payable = contracts::minus(ctx.accounts.claim.payable, args.amount)?;
        ctx.accounts.claim.paid = contracts::plus(ctx.accounts.claim.paid, args.amount)?;
        ctx.accounts.claim.revision = contracts::plus(ctx.accounts.claim.revision, 1)?;
        let beneficiary = ctx.accounts.claim.terms.supplier;
        record(&mut ctx.accounts.source, &ctx.accounts.claim, &mut ctx.accounts.operation,
            args.operation_id, digest, PAYOUT, args.amount, beneficiary)?;
        require!(ctx.accounts.escrow.amount >= ctx.accounts.source.data.balances.protected_cash, CoreError::CashDeficit);
        project_if_requested(&mut ctx.accounts.source, &mut ctx.accounts.operation, &ctx.accounts.pool,
            &ctx.accounts.pool_program, &ctx.accounts.dispatcher, ctx.bumps.dispatcher, args.service_without_pool)?;
        completed(&ctx.accounts.operation);
        Ok(())
    }

    /// Repayment reduces buyer debt and transfers to the recorded creditor
    /// treasury. It never calls Registry to replenish expenditure permission.
    pub fn repay(ctx: Context<Repay>, args: StageArgs) -> Result<()> {
        let mut args = args;
        args.operation_id = contracts::signed_request_id(ctx.accounts.repayment_authority.key(), &args.operation_id);
        let digest = contracts::operation_digest(&ctx.accounts.claim.terms, REPAY, &args.operation_id, args.amount, args.service_without_pool)?;
        let digest = contracts::debit_digest(&digest, ctx.accounts.repayment_authority.key(), ctx.accounts.from.key());
        if replay(&ctx.accounts.operation, &digest, SCHEMA_VERSION)? { return Ok(()); }
        validate_claim(&ctx.accounts.claim, &ctx.accounts.source, ctx.accounts.mint.key(), args.expected_revision)?;
        require!(args.amount > 0 && args.amount <= ctx.accounts.claim.debt, CoreError::Amount);
        require_keys_eq!(ctx.accounts.treasury.key(), ctx.accounts.source.data.treasury, CoreError::TokenRouting);
        require_keys_eq!(ctx.accounts.treasury.owner, ctx.accounts.source.data.pool, CoreError::TokenRouting);
        require_keys_neq!(ctx.accounts.from.key(), ctx.accounts.treasury.key(), CoreError::TokenRouting);
        let from_before = ctx.accounts.from.amount;
        let before = ctx.accounts.treasury.amount;
        token::transfer_checked(CpiContext::new(ctx.accounts.token_program.key(), TransferChecked {
            from: ctx.accounts.from.to_account_info(), mint: ctx.accounts.mint.to_account_info(),
            to: ctx.accounts.treasury.to_account_info(), authority: ctx.accounts.repayment_authority.to_account_info(),
        }), args.amount, ctx.accounts.mint.decimals)?;
        ctx.accounts.from.reload()?;
        ctx.accounts.treasury.reload()?;
        require!(ctx.accounts.from.amount == contracts::minus(from_before, args.amount)?
            && ctx.accounts.treasury.amount == contracts::plus(before, args.amount)?, CoreError::TokenDelta);
        ctx.accounts.claim.debt = contracts::minus(ctx.accounts.claim.debt, args.amount)?;
        ctx.accounts.claim.repaid = contracts::plus(ctx.accounts.claim.repaid, args.amount)?;
        ctx.accounts.claim.revision = contracts::plus(ctx.accounts.claim.revision, 1)?;
        let beneficiary = ctx.accounts.source.data.treasury;
        record(&mut ctx.accounts.source, &ctx.accounts.claim, &mut ctx.accounts.operation,
            args.operation_id, digest, REPAY, args.amount, beneficiary)?;
        project_if_requested(&mut ctx.accounts.source, &mut ctx.accounts.operation, &ctx.accounts.pool,
            &ctx.accounts.pool_program, &ctx.accounts.dispatcher, ctx.bumps.dispatcher, args.service_without_pool)?;
        emit!(RepaymentDebited { receipt: ctx.accounts.operation.key(), authority: ctx.accounts.repayment_authority.key(),
            from: ctx.accounts.from.key(), amount: args.amount });
        completed(&ctx.accounts.operation);
        Ok(())
    }

    /// Only the supplier can surrender an accepted unused promise before its
    /// capture deadline. After expiry anyone can release the unused remainder;
    /// earned supplier payable and debt are never canceled by this operation.
    pub fn release_unused(ctx: Context<ReleaseUnused>, args: StageArgs) -> Result<()> {
        let mut args = args;
        args.operation_id = contracts::signed_request_id(ctx.accounts.payer.key(), &args.operation_id);
        let digest = contracts::operation_digest(&ctx.accounts.claim.terms, RELEASE, &args.operation_id, args.amount, args.service_without_pool)?;
        if replay(&ctx.accounts.operation, &digest, SCHEMA_VERSION)? { return Ok(()); }
        validate_claim(&ctx.accounts.claim, &ctx.accounts.source, ctx.accounts.mint.key(), args.expected_revision)?;
        require_keys_eq!(ctx.accounts.principal.key(), ctx.accounts.claim.terms.principal, CoreError::TermsMismatch);
        validate_reservation(&ctx.accounts.reservation, &ctx.accounts.claim)?;
        if Clock::get()?.unix_timestamp <= ctx.accounts.claim.terms.expires_at {
            let supplier = ctx.accounts.supplier.as_ref().ok_or_else(|| error!(CoreError::ReleaseAuthority))?;
            require_keys_eq!(supplier.key(), ctx.accounts.claim.terms.supplier, CoreError::ReleaseAuthority);
        }
        require!(args.amount > 0 && args.amount <= ctx.accounts.claim.remaining, CoreError::Amount);
        require_keys_eq!(ctx.accounts.escrow.key(), ctx.accounts.source.data.escrow, CoreError::TokenRouting);
        require_keys_eq!(ctx.accounts.treasury.key(), ctx.accounts.source.data.treasury, CoreError::TokenRouting);
        require_keys_eq!(ctx.accounts.treasury.owner, ctx.accounts.source.data.pool, CoreError::TokenRouting);
        let before = ctx.accounts.escrow.amount;
        let treasury_before = ctx.accounts.treasury.amount;
        require!(before >= ctx.accounts.source.data.balances.protected_cash, CoreError::CashDeficit);
        let registry_bump = [ctx.bumps.registry_authority];
        let registry_seeds: &[&[u8]] = &[REGISTRY_CONSUMER_SEED, &registry_bump];
        principal_registry::cpi::release(CpiContext::new_with_signer(ctx.accounts.registry_program.key(), principal_registry::cpi::accounts::ServiceReservation {
            core_authority: ctx.accounts.registry_authority.to_account_info(), payer: ctx.accounts.payer.to_account_info(),
            principal: ctx.accounts.principal.to_account_info(), budget: ctx.accounts.budget.to_account_info(),
            reservation: ctx.accounts.reservation.to_account_info(), operation: ctx.accounts.registry_operation.to_account_info(),
            system_program: ctx.accounts.system_program.to_account_info(),
        }, &[registry_seeds]), args.clone())?;
        let pool = ctx.accounts.source.data.pool;
        let mint = ctx.accounts.source.data.mint;
        let bump = [ctx.accounts.source.data.bump];
        let seeds: &[&[u8]] = &[b"source", pool.as_ref(), mint.as_ref(), &bump];
        token::transfer_checked(CpiContext::new_with_signer(ctx.accounts.token_program.key(), TransferChecked {
            from: ctx.accounts.escrow.to_account_info(), mint: ctx.accounts.mint.to_account_info(),
            to: ctx.accounts.treasury.to_account_info(), authority: ctx.accounts.source.to_account_info(),
        }, &[seeds]), args.amount, ctx.accounts.mint.decimals)?;
        ctx.accounts.escrow.reload()?;
        ctx.accounts.treasury.reload()?;
        require!(ctx.accounts.escrow.amount == contracts::minus(before, args.amount)?
            && ctx.accounts.treasury.amount == contracts::plus(treasury_before, args.amount)?, CoreError::TokenDelta);
        ctx.accounts.claim.remaining = contracts::minus(ctx.accounts.claim.remaining, args.amount)?;
        ctx.accounts.claim.released = contracts::plus(ctx.accounts.claim.released, args.amount)?;
        ctx.accounts.claim.revision = contracts::plus(ctx.accounts.claim.revision, 1)?;
        let beneficiary = ctx.accounts.source.data.treasury;
        record(&mut ctx.accounts.source, &ctx.accounts.claim, &mut ctx.accounts.operation,
            args.operation_id, digest, RELEASE, args.amount, beneficiary)?;
        require!(ctx.accounts.escrow.amount >= ctx.accounts.source.data.balances.protected_cash, CoreError::CashDeficit);
        project_if_requested(&mut ctx.accounts.source, &mut ctx.accounts.operation, &ctx.accounts.pool,
            &ctx.accounts.pool_program, &ctx.accounts.dispatcher, ctx.bumps.dispatcher, args.service_without_pool)?;
        completed(&ctx.accounts.operation);
        Ok(())
    }
}

fn accept_with_profile(ctx: Context<AcceptTab>, args: AcceptArgs, profile: u16) -> Result<()> {
    let mut args = args;
    args.operation_id = contracts::signed_request_id(ctx.accounts.credential.key(), &args.operation_id);
    let digest = contracts::operation_digest(&args.terms, ACCEPT, &args.operation_id, args.terms.cap, false)?;
    if replay(&ctx.accounts.operation, &digest, SCHEMA_VERSION)? {
        require!(ctx.accounts.claim.version == profile, CoreError::CaptureProfile);
        return Ok(());
    }
    require!(ctx.accounts.claim.version == 0, CoreError::DuplicateClaim);
    require!(args.terms.cap > 0 && args.terms.expires_at > Clock::get()?.unix_timestamp, CoreError::Expired);
    require_keys_eq!(args.terms.principal, ctx.accounts.principal.key(), CoreError::TermsMismatch);
    require_keys_eq!(args.terms.actor, ctx.accounts.actor.key(), CoreError::TermsMismatch);
    require_keys_eq!(args.terms.mandate, ctx.accounts.mandate.key(), CoreError::TermsMismatch);
    require_keys_eq!(args.terms.asset, ctx.accounts.mint.key(), CoreError::TermsMismatch);
    require_keys_eq!(args.terms.source, ctx.accounts.source.key(), CoreError::TermsMismatch);
    validate_source(&ctx.accounts.source, ctx.accounts.mint.key())?;
    require_keys_eq!(ctx.accounts.source.data.pool, ctx.accounts.pool.key(), CoreError::SourceMismatch);
    require_keys_eq!(ctx.accounts.source.data.treasury, ctx.accounts.treasury.key(), CoreError::SourceMismatch);
    require_keys_eq!(ctx.accounts.source.data.escrow, ctx.accounts.escrow.key(), CoreError::SourceMismatch);
    require_keys_eq!(ctx.accounts.pool.manager, ctx.accounts.manager.key(), CoreError::IssuerAuthority);
    let before = ctx.accounts.source.data.balances;
    require!(ctx.accounts.escrow.amount >= before.protected_cash, CoreError::CashDeficit);
    let escrow_before = ctx.accounts.escrow.amount;
    let registry_bump = [ctx.bumps.registry_authority];
    let registry_seeds: &[&[u8]] = &[REGISTRY_CONSUMER_SEED, &registry_bump];
    principal_registry::cpi::reserve(CpiContext::new_with_signer(ctx.accounts.registry_program.key(), principal_registry::cpi::accounts::Reserve {
        core_authority: ctx.accounts.registry_authority.to_account_info(), payer: ctx.accounts.payer.to_account_info(),
        principal: ctx.accounts.principal.to_account_info(), actor: ctx.accounts.actor.to_account_info(),
        credential: ctx.accounts.credential.to_account_info(), mandate: ctx.accounts.mandate.to_account_info(),
        budget: ctx.accounts.budget.to_account_info(), reservation: ctx.accounts.reservation.to_account_info(),
        operation: ctx.accounts.registry_operation.to_account_info(), system_program: ctx.accounts.system_program.to_account_info(),
    }, &[registry_seeds]), args.clone())?;
    ctx.accounts.claim.set_inner(Claim {
        version: profile, bump: ctx.bumps.claim, terms: args.terms.clone(),
        remaining: args.terms.cap, payable: 0, debt: 0, paid: 0, repaid: 0, released: 0, revision: 1,
    });
    record(&mut ctx.accounts.source, &ctx.accounts.claim, &mut ctx.accounts.operation,
        args.operation_id, digest, ACCEPT, args.terms.cap, args.terms.supplier)?;
    // Anchor normally writes at handler exit. Pool must see the canonical
    // updated source and receipt during this CPI, not their previous bytes.
    ctx.accounts.source.exit(&crate::ID)?;
    ctx.accounts.operation.exit(&crate::ID)?;
    let dispatch_bump = [ctx.bumps.dispatcher];
    let dispatch_seeds: &[&[u8]] = &[POOL_DISPATCH_SEED, &dispatch_bump];
    pool_issuer::cpi::reserve(CpiContext::new_with_signer(ctx.accounts.pool_program.key(), pool_issuer::cpi::accounts::Reserve {
        dispatcher: ctx.accounts.dispatcher.to_account_info(), pool: ctx.accounts.pool.to_account_info(),
        source: ctx.accounts.source.to_account_info(), operation: ctx.accounts.operation.to_account_info(),
        treasury: ctx.accounts.treasury.to_account_info(), escrow: ctx.accounts.escrow.to_account_info(),
        mint: ctx.accounts.mint.to_account_info(), token_program: ctx.accounts.token_program.to_account_info(),
    }, &[dispatch_seeds]))?;
    ctx.accounts.escrow.reload()?;
    require!(ctx.accounts.escrow.amount == contracts::plus(escrow_before, args.terms.cap)?
        && ctx.accounts.escrow.amount >= ctx.accounts.source.data.balances.protected_cash, CoreError::TokenDelta);
    completed(&ctx.accounts.operation);
    Ok(())
}

fn validate_usage_policy(terms: &TabTerms, namespace: &[u8; 32], service_unit: &[u8; 32],
    unit_price: u64, external_contract_hash: &[u8; 32]) -> Result<()> {
    require!(unit_price > 0 && terms.external_terms_hash ==
        contracts::completed_run_policy(namespace, service_unit, unit_price, external_contract_hash), CoreError::UsagePolicy);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_completed_run(ctx: Context<CaptureCompletedRun>, args: StageArgs,
    namespace: [u8; 32], run_id: [u8; 32], service_unit: [u8; 32], external_contract_hash: [u8; 32],
    payment_id: [u8; 32], units: u64, unit_price: u64) -> Result<()> {
    let a = ctx.accounts;
    require!(a.claim.version == METERED_CLAIM_VERSION, CoreError::CaptureProfile);
    let terms = &a.claim.terms;
    let supplier = a.supplier.key();
    require_keys_eq!(supplier, terms.supplier, CoreError::CaptureAuthority);
    require_keys_eq!(a.principal.key(), terms.principal, CoreError::TermsMismatch);
    require_keys_eq!(a.mint.key(), terms.asset, CoreError::TermsMismatch);
    require_keys_eq!(a.source.key(), terms.source, CoreError::TermsMismatch);
    validate_usage_policy(terms, &namespace, &service_unit, unit_price, &external_contract_hash)?;
    require!(units > 0 && units.checked_mul(unit_price) == Some(args.amount), CoreError::UsagePolicy);
    require!(args.operation_id == contracts::completed_run_operation_id(&supplier, &namespace, &run_id), CoreError::UsageIdentity);
    let terms_hash = contracts::terms_digest(terms)?;
    let digest = contracts::operation_digest(terms, CAPTURE, &args.operation_id, args.amount, args.service_without_pool)?;

    // Within the worker-payment domain one ID can only identify one billed run.
    // Reject an attempt to move an already bound payment to another run before any CPI.
    if a.payment.version != 0 {
        require!(a.payment.version == SCHEMA_VERSION && a.payment.bump == ctx.bumps.payment
            && a.payment.payment_id == payment_id && a.payment.principal == a.principal.key()
            && a.payment.usage_origin == a.usage_origin.key() && a.payment.operation == a.operation.key()
            && a.payment.request_digest == digest && a.usage_origin.version == SCHEMA_VERSION
            && a.payment.effect_hash == a.usage_origin.effect_hash, CoreError::PaymentConflict);
    }

    let origin = &a.usage_origin;
    if origin.version == 0 {
        // Never adopt a pre-existing receipt for another transition. Both fresh records,
        // both economic mutations and the Pool projection commit or roll back together.
        require!(a.operation.data.version == 0, CoreError::UsageConflict);
        CaptureExecution::from(&mut *a).apply_capture(args.clone(), digest, ctx.bumps.registry_authority, ctx.bumps.dispatcher)?;
        a.usage_origin.set_inner(CompletedRunOrigin {
            version: SCHEMA_VERSION, bump: ctx.bumps.usage_origin, supplier, namespace, run_id, service_unit,
            principal: a.principal.key(), claim: a.claim.key(), asset: a.mint.key(), units, unit_price,
            amount: args.amount, terms_digest: terms_hash, first_payment_id: payment_id, operation: a.operation.key(),
            request_digest: digest, effect_hash: a.operation.data.effect_hash,
        });
    } else {
        require!(origin.version == SCHEMA_VERSION && origin.bump == ctx.bumps.usage_origin
            && origin.supplier == supplier && origin.namespace == namespace && origin.run_id == run_id
            && origin.service_unit == service_unit && origin.principal == a.principal.key()
            && origin.claim == a.claim.key() && origin.asset == a.mint.key() && origin.units == units
            && origin.unit_price == unit_price && origin.amount == args.amount && origin.terms_digest == terms_hash
            && origin.operation == a.operation.key() && origin.request_digest == digest
            && origin.effect_hash == a.operation.data.effect_hash, CoreError::UsageConflict);
        require!(replay(&a.operation, &digest, RUN_RECEIPT_VERSION)?, CoreError::UsageConflict);
    }

    if a.payment.version == 0 {
        a.payment.set_inner(CompletedRunPayment {
            version: SCHEMA_VERSION, bump: ctx.bumps.payment, principal: a.principal.key(), payment_id,
            usage_origin: a.usage_origin.key(), operation: a.operation.key(),
            request_digest: digest, effect_hash: a.operation.data.effect_hash,
        });
    }
    Ok(())
}

fn validate_source(source: &Account<Source>, mint: Pubkey) -> Result<()> {
    let data = &source.data;
    require!(data.version == SCHEMA_VERSION, CoreError::Version);
    require_keys_eq!(data.mint, mint, CoreError::SourceMismatch);
    let expected = Pubkey::find_program_address(&[b"source", data.pool.as_ref(), mint.as_ref()], &crate::ID).0;
    require_keys_eq!(source.key(), expected, CoreError::SourceMismatch);
    require!(data.balances.protected_cash == contracts::plus(data.balances.reserved, data.balances.payable)?, CoreError::CashDeficit);
    Ok(())
}

fn validate_claim(claim: &Account<Claim>, source: &Account<Source>, mint: Pubkey, revision: u64) -> Result<()> {
    validate_source(source, mint)?;
    require!(matches!(claim.version, SCHEMA_VERSION | METERED_CLAIM_VERSION), CoreError::Version);
    require!(claim.revision == revision, CoreError::Revision);
    require_keys_eq!(claim.terms.source, source.key(), CoreError::SourceMismatch);
    require_keys_eq!(claim.terms.asset, mint, CoreError::SourceMismatch);
    let expected = Pubkey::find_program_address(&[b"claim", claim.terms.principal.as_ref(), &claim.terms.tab_id], &crate::ID).0;
    require_keys_eq!(claim.key(), expected, CoreError::TermsMismatch);
    Ok(())
}

fn validate_reservation(reservation: &Reservation, claim: &Claim) -> Result<()> {
    require!(reservation.terms == claim.terms && reservation.remaining == claim.remaining
        && reservation.consumed == contracts::plus(claim.payable, claim.paid)?
        && reservation.released == claim.released, CoreError::ReservationMismatch);
    Ok(())
}

fn replay(operation: &OperationRecord, digest: &[u8; 32], receipt_version: u16) -> Result<bool> {
    if operation.data.version == 0 { return Ok(false); }
    require!(operation.data.version == receipt_version && operation.data.request_digest == *digest, CoreError::OperationConflict);
    require!(operation.data.effect_hash == contracts::effect_digest(&operation.data.effect)?, CoreError::OperationConflict);
    completed(operation);
    Ok(true)
}

fn record(source: &mut Account<Source>, claim: &Account<Claim>, operation: &mut Account<OperationRecord>,
    operation_id: [u8; 32], request_digest: [u8; 32], kind: u8, amount: u64, beneficiary: Pubkey) -> Result<()> {
    let before = source.data.balances;
    let after = before.transition(kind, amount)?;
    let sequence = contracts::plus(source.data.sequence, 1)?;
    let effect = EffectData {
        version: SCHEMA_VERSION, domain: claim.terms.domain, principal: claim.terms.principal,
        operation_id, request_digest, claim: claim.key(), source: source.key(), book: source.data.pool,
        mint: source.data.mint, kind, amount, beneficiary, sequence, previous_hash: source.data.head, before, after,
    };
    let effect_hash = contracts::effect_digest(&effect)?;
    let version = if kind == CAPTURE && claim.version == METERED_CLAIM_VERSION {
        RUN_RECEIPT_VERSION
    } else { SCHEMA_VERSION };
    operation.data = OperationData { version, request_digest, effect, effect_hash };
    source.data.balances = after;
    source.data.sequence = sequence;
    source.data.head = effect_hash;
    Ok(())
}

fn project_if_requested<'info>(source: &mut Account<'info, Source>, operation: &mut Account<'info, OperationRecord>,
    pool: &Option<UncheckedAccount<'info>>, program: &Option<Program<'info, PoolIssuer>>,
    dispatcher: &UncheckedAccount<'info>, bump: u8, without_pool: bool) -> Result<()> {
    if without_pool { return Ok(()); }
    let pool = pool.as_ref().ok_or_else(|| error!(CoreError::PoolRequired))?;
    let program = program.as_ref().ok_or_else(|| error!(CoreError::PoolRequired))?;
    require_keys_eq!(pool.key(), source.data.pool, CoreError::SourceMismatch);
    source.exit(&crate::ID)?;
    operation.exit(&crate::ID)?;
    let bump_bytes = [bump];
    let seeds: &[&[u8]] = &[POOL_DISPATCH_SEED, &bump_bytes];
    pool_issuer::cpi::project(CpiContext::new_with_signer(program.key(), pool_issuer::cpi::accounts::Project {
        dispatcher: dispatcher.to_account_info(), pool: pool.to_account_info(),
        source: source.to_account_info(), operation: operation.to_account_info(),
    }, &[seeds]))
}

fn completed(operation: &OperationRecord) {
    if operation.data.version == RUN_RECEIPT_VERSION {
        let effect = &operation.data.effect;
        emit!(CompletedRunBilled { principal: effect.principal, operation_id: effect.operation_id,
            amount: effect.amount, sequence: effect.sequence, effect_hash: operation.data.effect_hash });
        return;
    }
    emit!(OperationCompleted { principal: operation.data.effect.principal,
        operation_id: operation.data.effect.operation_id, kind: operation.data.effect.kind,
        amount: operation.data.effect.amount, sequence: operation.data.effect.sequence,
        effect_hash: operation.data.effect_hash });
}

#[derive(Accounts)]
pub struct InitializeSource<'info> {
    #[account(mut)] pub manager: Signer<'info>,
    #[account(has_one = manager, has_one = mint, has_one = treasury)] pub pool: Box<Account<'info, PoolBook>>,
    pub mint: Box<Account<'info, Mint>>,
    #[account(token::mint = mint, token::authority = pool)] pub treasury: Box<Account<'info, TokenAccount>>,
    #[account(init_if_needed, payer = manager, space = 8 + Source::INIT_SPACE,
        seeds = [b"source", pool.key().as_ref(), mint.key().as_ref()], bump)]
    pub source: Box<Account<'info, Source>>,
    #[account(init_if_needed, payer = manager, associated_token::mint = mint, associated_token::authority = source)]
    pub escrow: Box<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(args: AcceptArgs)]
pub struct AcceptTab<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    pub manager: Signer<'info>,
    pub credential: Signer<'info>,
    pub principal: Box<Account<'info, Principal>>,
    pub actor: Box<Account<'info, Actor>>,
    pub mandate: Box<Account<'info, Mandate>>,
    #[account(mut)] pub budget: Box<Account<'info, Budget>>,
    /// CHECK: Registry validates and initializes its canonical reservation PDA.
    #[account(mut)] pub reservation: UncheckedAccount<'info>,
    /// CHECK: Registry validates and initializes its canonical operation PDA.
    #[account(mut)] pub registry_operation: UncheckedAccount<'info>,
    /// CHECK: Canonical Core signer elevated only by Core's CPI seeds.
    #[account(seeds = [REGISTRY_CONSUMER_SEED], bump)] pub registry_authority: UncheckedAccount<'info>,
    pub registry_program: Program<'info, PrincipalRegistry>,
    #[account(mut)] pub source: Box<Account<'info, Source>>,
    #[account(init_if_needed, payer = payer, space = 8 + Claim::INIT_SPACE,
        seeds = [b"claim", principal.key().as_ref(), &args.terms.tab_id], bump)]
    pub claim: Box<Account<'info, Claim>>,
    #[account(init_if_needed, payer = payer, space = 8 + OperationRecord::INIT_SPACE,
        seeds = [b"operation", principal.key().as_ref(), &contracts::signed_request_id(credential.key(), &args.operation_id)], bump)]
    pub operation: Box<Account<'info, OperationRecord>>,
    #[account(mut, token::mint = mint, token::authority = source)] pub escrow: Box<Account<'info, TokenAccount>>,
    pub mint: Box<Account<'info, Mint>>,
    #[account(mut, has_one = manager, has_one = mint, has_one = treasury, has_one = source)]
    pub pool: Box<Account<'info, PoolBook>>,
    #[account(mut, token::mint = mint, token::authority = pool)] pub treasury: Box<Account<'info, TokenAccount>>,
    /// CHECK: Canonical Core dispatcher signer elevated only by Core.
    #[account(seeds = [POOL_DISPATCH_SEED], bump)] pub dispatcher: UncheckedAccount<'info>,
    pub pool_program: Program<'info, PoolIssuer>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(args: StageArgs)]
pub struct Capture<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    pub supplier: Signer<'info>,
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut)] pub budget: Box<Account<'info, Budget>>,
    #[account(mut)] pub reservation: Box<Account<'info, Reservation>>,
    /// CHECK: Registry validates and initializes its canonical operation PDA.
    #[account(mut)] pub registry_operation: UncheckedAccount<'info>,
    /// CHECK: Canonical Core signer elevated only by Core.
    #[account(seeds = [REGISTRY_CONSUMER_SEED], bump)] pub registry_authority: UncheckedAccount<'info>,
    pub registry_program: Program<'info, PrincipalRegistry>,
    #[account(mut)] pub source: Box<Account<'info, Source>>,
    #[account(mut)] pub claim: Box<Account<'info, Claim>>,
    #[account(init_if_needed, payer = payer, space = 8 + OperationRecord::INIT_SPACE,
        seeds = [b"operation", claim.terms.principal.as_ref(), &contracts::signed_request_id(supplier.key(), &args.operation_id)], bump)]
    pub operation: Box<Account<'info, OperationRecord>>,
    #[account(token::mint = mint, token::authority = source)] pub escrow: Box<Account<'info, TokenAccount>>,
    pub mint: Box<Account<'info, Mint>>,
    /// CHECK: Canonical Core dispatcher signer elevated only by Core.
    #[account(seeds = [POOL_DISPATCH_SEED], bump)] pub dispatcher: UncheckedAccount<'info>,
    /// CHECK: Optional accepted-right escape omits Pool. Normal CPI validates ownership and source binding.
    #[account(mut)] pub pool: Option<UncheckedAccount<'info>>,
    pub pool_program: Option<Program<'info, PoolIssuer>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(args: StageArgs, namespace: [u8; 32], run_id: [u8; 32],
    service_unit: [u8; 32], external_contract_hash: [u8; 32], payment_id: [u8; 32])]
pub struct CaptureCompletedRun<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    pub supplier: Signer<'info>,
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut)] pub budget: Box<Account<'info, Budget>>,
    #[account(mut)] pub reservation: Box<Account<'info, Reservation>>,
    /// CHECK: Registry validates and initializes its canonical operation PDA.
    #[account(mut)] pub registry_operation: UncheckedAccount<'info>,
    /// CHECK: Canonical Core signer elevated only by Core.
    #[account(seeds = [REGISTRY_CONSUMER_SEED], bump)] pub registry_authority: UncheckedAccount<'info>,
    pub registry_program: Program<'info, PrincipalRegistry>,
    #[account(mut)] pub source: Box<Account<'info, Source>>,
    #[account(mut)] pub claim: Box<Account<'info, Claim>>,
    #[account(init_if_needed, payer = payer, space = 8 + OperationRecord::INIT_SPACE,
        seeds = [RUN_OPERATION_SEED, claim.terms.principal.as_ref(), &args.operation_id], bump)]
    pub operation: Box<Account<'info, OperationRecord>>,
    #[account(token::mint = mint, token::authority = source)] pub escrow: Box<Account<'info, TokenAccount>>,
    pub mint: Box<Account<'info, Mint>>,
    /// CHECK: Canonical Core dispatcher signer elevated only by Core.
    #[account(seeds = [POOL_DISPATCH_SEED], bump)] pub dispatcher: UncheckedAccount<'info>,
    /// CHECK: Optional accepted-right escape omits Pool. Normal CPI validates ownership and source binding.
    #[account(mut)] pub pool: Option<UncheckedAccount<'info>>,
    pub pool_program: Option<Program<'info, PoolIssuer>>,
    pub system_program: Program<'info, System>,
    #[account(init_if_needed, payer = payer, space = 8 + CompletedRunOrigin::INIT_SPACE,
        seeds = [b"usage", supplier.key().as_ref(), namespace.as_ref(), run_id.as_ref()], bump)]
    pub usage_origin: Box<Account<'info, CompletedRunOrigin>>,
    #[account(init_if_needed, payer = payer, space = 8 + CompletedRunPayment::INIT_SPACE,
        seeds = [b"run-payment", principal.key().as_ref(), payment_id.as_ref()], bump)]
    pub payment: Box<Account<'info, CompletedRunPayment>>,
}

// Both on-chain entry points use the same financial transition, after Anchor has
// validated their explicitly declared account lists.
struct CaptureExecution<'a, 'info> {
    payer: &'a Signer<'info>,
    supplier: &'a Signer<'info>,
    principal: &'a Account<'info, Principal>,
    budget: &'a Account<'info, Budget>,
    reservation: &'a Account<'info, Reservation>,
    registry_operation: &'a UncheckedAccount<'info>,
    registry_authority: &'a UncheckedAccount<'info>,
    registry_program: &'a Program<'info, PrincipalRegistry>,
    source: &'a mut Account<'info, Source>,
    claim: &'a mut Account<'info, Claim>,
    operation: &'a mut Account<'info, OperationRecord>,
    escrow: &'a Account<'info, TokenAccount>,
    mint: &'a Account<'info, Mint>,
    dispatcher: &'a UncheckedAccount<'info>,
    pool: &'a Option<UncheckedAccount<'info>>,
    pool_program: &'a Option<Program<'info, PoolIssuer>>,
    system_program: &'a Program<'info, System>,
}

// This conversion only borrows already-validated accounts. Keeping it out of the
// Anchor derives makes each instruction's account order and constraints visible in its IDL.
macro_rules! impl_capture_execution {
    ($accounts:ident) => {
        impl<'a, 'info> From<&'a mut $accounts<'info>> for CaptureExecution<'a, 'info> {
            fn from(a: &'a mut $accounts<'info>) -> Self {
                Self {
                    payer: &a.payer, supplier: &a.supplier, principal: &a.principal,
                    budget: &a.budget, reservation: &a.reservation, registry_operation: &a.registry_operation,
                    registry_authority: &a.registry_authority, registry_program: &a.registry_program,
                    source: &mut a.source, claim: &mut a.claim, operation: &mut a.operation,
                    escrow: &a.escrow, mint: &a.mint, dispatcher: &a.dispatcher,
                    pool: &a.pool, pool_program: &a.pool_program, system_program: &a.system_program,
                }
            }
        }
    };
}
impl_capture_execution!(Capture);
impl_capture_execution!(CaptureCompletedRun);

impl<'info> CaptureExecution<'_, 'info> {
    fn apply_capture(&mut self, args: StageArgs, digest: [u8; 32],
        registry_bump: u8, dispatcher_bump: u8) -> Result<()> {
        validate_claim(self.claim, self.source, self.mint.key(), args.expected_revision)?;
        require_keys_eq!(self.supplier.key(), self.claim.terms.supplier, CoreError::CaptureAuthority);
        require_keys_eq!(self.principal.key(), self.claim.terms.principal, CoreError::TermsMismatch);
        validate_reservation(self.reservation, self.claim)?;
        require!(Clock::get()?.unix_timestamp <= self.claim.terms.expires_at, CoreError::Expired);
        require!(args.amount > 0 && args.amount <= self.claim.remaining, CoreError::Amount);
        require_keys_eq!(self.escrow.key(), self.source.data.escrow, CoreError::TokenRouting);
        require!(self.escrow.amount >= self.source.data.balances.protected_cash, CoreError::CashDeficit);
        let bump = [registry_bump];
        let seeds: &[&[u8]] = &[REGISTRY_CONSUMER_SEED, &bump];
        if self.claim.version == METERED_CLAIM_VERSION {
            principal_registry::cpi::consume_completed_run(CpiContext::new_with_signer(self.registry_program.key(), principal_registry::cpi::accounts::ConsumeCompletedRun {
                core_authority: self.registry_authority.to_account_info(), payer: self.payer.to_account_info(),
                principal: self.principal.to_account_info(), budget: self.budget.to_account_info(),
                reservation: self.reservation.to_account_info(), operation: self.registry_operation.to_account_info(),
                system_program: self.system_program.to_account_info(),
            }, &[seeds]), args.clone())?;
        } else {
            principal_registry::cpi::consume(CpiContext::new_with_signer(self.registry_program.key(), principal_registry::cpi::accounts::ServiceReservation {
                core_authority: self.registry_authority.to_account_info(), payer: self.payer.to_account_info(),
                principal: self.principal.to_account_info(), budget: self.budget.to_account_info(),
                reservation: self.reservation.to_account_info(), operation: self.registry_operation.to_account_info(),
                system_program: self.system_program.to_account_info(),
            }, &[seeds]), args.clone())?;
        }
        self.claim.remaining = contracts::minus(self.claim.remaining, args.amount)?;
        self.claim.payable = contracts::plus(self.claim.payable, args.amount)?;
        self.claim.debt = contracts::plus(self.claim.debt, args.amount)?;
        self.claim.revision = contracts::plus(self.claim.revision, 1)?;
        let beneficiary = self.claim.terms.supplier;
        record(self.source, self.claim, self.operation,
            args.operation_id, digest, CAPTURE, args.amount, beneficiary)?;
        project_if_requested(self.source, self.operation, self.pool,
            self.pool_program, self.dispatcher, dispatcher_bump, args.service_without_pool)?;
        completed(self.operation);
        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(args: StageArgs)]
pub struct ReleaseUnused<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    pub supplier: Option<Signer<'info>>,
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut)] pub budget: Box<Account<'info, Budget>>,
    #[account(mut)] pub reservation: Box<Account<'info, Reservation>>,
    /// CHECK: Registry validates and initializes its canonical operation PDA.
    #[account(mut)] pub registry_operation: UncheckedAccount<'info>,
    /// CHECK: Canonical Core signer elevated only by Core.
    #[account(seeds = [REGISTRY_CONSUMER_SEED], bump)] pub registry_authority: UncheckedAccount<'info>,
    pub registry_program: Program<'info, PrincipalRegistry>,
    #[account(mut)] pub source: Box<Account<'info, Source>>,
    #[account(mut)] pub claim: Box<Account<'info, Claim>>,
    #[account(init_if_needed, payer = payer, space = 8 + OperationRecord::INIT_SPACE,
        seeds = [b"operation", claim.terms.principal.as_ref(), &contracts::signed_request_id(payer.key(), &args.operation_id)], bump)]
    pub operation: Box<Account<'info, OperationRecord>>,
    #[account(mut, token::mint = mint, token::authority = source)] pub escrow: Box<Account<'info, TokenAccount>>,
    #[account(mut, token::mint = mint)] pub treasury: Box<Account<'info, TokenAccount>>,
    pub mint: Box<Account<'info, Mint>>,
    /// CHECK: Canonical Core dispatcher signer elevated only by Core.
    #[account(seeds = [POOL_DISPATCH_SEED], bump)] pub dispatcher: UncheckedAccount<'info>,
    /// CHECK: Optional accepted-right escape omits Pool. Normal CPI validates ownership and source binding.
    #[account(mut)] pub pool: Option<UncheckedAccount<'info>>,
    pub pool_program: Option<Program<'info, PoolIssuer>>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(args: StageArgs)]
pub struct Payout<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    #[account(mut)] pub source: Box<Account<'info, Source>>,
    #[account(mut)] pub claim: Box<Account<'info, Claim>>,
    #[account(init_if_needed, payer = payer, space = 8 + OperationRecord::INIT_SPACE,
        seeds = [b"operation", claim.terms.principal.as_ref(), &contracts::signed_request_id(payer.key(), &args.operation_id)], bump)]
    pub operation: Box<Account<'info, OperationRecord>>,
    #[account(mut, token::mint = mint, token::authority = source)] pub escrow: Box<Account<'info, TokenAccount>>,
    #[account(mut, token::mint = mint)] pub destination: Box<Account<'info, TokenAccount>>,
    pub mint: Box<Account<'info, Mint>>,
    /// CHECK: Canonical Core dispatcher signer elevated only by Core.
    #[account(seeds = [POOL_DISPATCH_SEED], bump)] pub dispatcher: UncheckedAccount<'info>,
    /// CHECK: Optional accepted-right escape omits Pool. Normal CPI validates ownership and source binding.
    #[account(mut)] pub pool: Option<UncheckedAccount<'info>>,
    pub pool_program: Option<Program<'info, PoolIssuer>>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(args: StageArgs)]
pub struct Repay<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    /// The token owner authorizes repayment independently of the rent/fee sponsor.
    /// A governed wallet may provide this signature through its native CPI.
    pub repayment_authority: Signer<'info>,
    #[account(mut)] pub source: Box<Account<'info, Source>>,
    #[account(mut)] pub claim: Box<Account<'info, Claim>>,
    #[account(init_if_needed, payer = payer, space = 8 + OperationRecord::INIT_SPACE,
        seeds = [b"operation", claim.terms.principal.as_ref(), &contracts::signed_request_id(repayment_authority.key(), &args.operation_id)], bump)]
    pub operation: Box<Account<'info, OperationRecord>>,
    #[account(mut, token::mint = mint, token::authority = repayment_authority)] pub from: Box<Account<'info, TokenAccount>>,
    #[account(mut, token::mint = mint)] pub treasury: Box<Account<'info, TokenAccount>>,
    pub mint: Box<Account<'info, Mint>>,
    /// CHECK: Canonical Core dispatcher signer elevated only by Core.
    #[account(seeds = [POOL_DISPATCH_SEED], bump)] pub dispatcher: UncheckedAccount<'info>,
    /// CHECK: Optional accepted-right escape omits Pool. Normal CPI validates ownership and source binding.
    #[account(mut)] pub pool: Option<UncheckedAccount<'info>>,
    pub pool_program: Option<Program<'info, PoolIssuer>>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

// These two bodies are shared with independently validating Pool/readers.
// Do not append fields outside the shared body or silently change discriminators.
#[account]
#[derive(InitSpace)]
pub struct Source { pub data: SourceData }

#[account]
#[derive(InitSpace)]
pub struct OperationRecord { pub data: OperationData }

#[account]
#[derive(InitSpace)]
pub struct Claim {
    pub version: u16,
    pub bump: u8,
    pub terms: TabTerms,
    pub remaining: u64,
    pub payable: u64,
    pub debt: u64,
    pub paid: u64,
    pub repaid: u64,
    pub released: u64,
    pub revision: u64,
}

/// One immutable, supplier-attested bill per supplier, billing namespace and run.
#[account]
#[derive(InitSpace)]
pub struct CompletedRunOrigin {
    pub version: u16,
    pub bump: u8,
    pub supplier: Pubkey,
    pub namespace: [u8; 32],
    pub run_id: [u8; 32],
    pub service_unit: [u8; 32],
    pub principal: Pubkey,
    pub claim: Pubkey,
    pub asset: Pubkey,
    pub units: u64,
    pub unit_price: u64,
    pub amount: u64,
    pub terms_digest: [u8; 32],
    pub first_payment_id: [u8; 32],
    pub operation: Pubkey,
    pub request_digest: [u8; 32],
    pub effect_hash: [u8; 32],
}

/// Resolves the submitted financial ID to the original bill and its sole financial effect.
#[account]
#[derive(InitSpace)]
pub struct CompletedRunPayment {
    pub version: u16,
    pub bump: u8,
    pub principal: Pubkey,
    pub payment_id: [u8; 32],
    pub usage_origin: Pubkey,
    pub operation: Pubkey,
    pub request_digest: [u8; 32],
    pub effect_hash: [u8; 32],
}

#[event]
pub struct RepaymentDebited {
    pub receipt: Pubkey,
    pub authority: Pubkey,
    pub from: Pubkey,
    pub amount: u64,
}

#[event]
pub struct CompletedRunBilled {
    pub principal: Pubkey,
    pub operation_id: [u8; 32],
    pub amount: u64,
    pub sequence: u64,
    pub effect_hash: [u8; 32],
}

#[event]
pub struct OperationCompleted {
    pub principal: Pubkey,
    pub operation_id: [u8; 32],
    pub kind: u8,
    pub amount: u64,
    pub sequence: u64,
    pub effect_hash: [u8; 32],
}

#[error_code]
pub enum CoreError {
    #[msg("Operation identity has already committed different economic terms")] OperationConflict,
    #[msg("Tab identity already has an accepted financial claim")] DuplicateClaim,
    #[msg("Financial terms differ from the bound account, actor, source or asset")] TermsMismatch,
    #[msg("Finite acceptance or capture deadline has expired")] Expired,
    #[msg("Issuer manager did not authorize this exact guarantee")] IssuerAuthority,
    #[msg("Capture requires the accepted supplier's signature")] CaptureAuthority,
    #[msg("Unused rights require supplier surrender before the capture deadline")] ReleaseAuthority,
    #[msg("Registry reservation does not match this claim's accepted terms and residual rights")] ReservationMismatch,
    #[msg("Source, book, escrow or creditor route does not match")] SourceMismatch,
    #[msg("Unsupported financial schema version")] Version,
    #[msg("Claim changed since this operation was prepared")] Revision,
    #[msg("Amount must be positive and within the residual accepted right")] Amount,
    #[msg("Actual protected custody is deficient")] CashDeficit,
    #[msg("Token account is not on the accepted service route")] TokenRouting,
    #[msg("Token movement differs from the exact authorized amount")] TokenDelta,
    #[msg("Normal financial operation requires the issuer Pool projection")] PoolRequired,
    #[msg("This capture mode does not match the accepted Tab profile")] CaptureProfile,
    #[msg("The reported usage does not match the accepted tariff")] UsagePolicy,
    #[msg("Invalid canonical completed-run or payment identity")] UsageIdentity,
    #[msg("This completed run already binds another claim or bill")] UsageConflict,
    #[msg("This payment identifier already binds another completed run")] PaymentConflict,

    // Financing errors share the program IDL and follow the existing stable codes.

    #[msg("Unsupported financial account schema")] FundingSchema,
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

}
