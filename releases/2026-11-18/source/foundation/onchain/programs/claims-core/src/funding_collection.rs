//! Independent servicing of already-earned supplier rights. No initiating
//! agent, financier owner, issuer backend or hosted subscription authorizes it.
use anchor_lang::prelude::*;
use crate::CoreError;
use anchor_spl::{associated_token, token::{self, Mint, TokenAccount}};
use dexter_contracts::{plus, minus};
use dexter_contracts::commercial::{SellerFeeTerms, SponsorQuote};
use dexter_contracts::financing::{Asset, EarnedCollection, FINANCING_VERSION};
use dexter_contracts::funded::*;
use crate::funding::CollectFinancier;
use crate::funding_state::*;
use crate::funding_journal::{inc, dec, journal, Change};
use crate::funding_sponsor::{apply_sponsorship, SponsorOperation, SponsorshipAccounts};

pub fn collect<'info>(ctx: Context<'info, CollectFinancier<'info>>,
    operation_id: [u8; 32], paid_before: u64, amount: u64, expected_revision: u64,
    service_without_pool: bool, sponsorship: Option<SponsorQuote>) -> Result<()> {
    ctx.accounts.claim.check(ctx.accounts.claim.key())?;
    ctx.accounts.run.check(ctx.accounts.run.key(), ctx.accounts.claim.key(), &ctx.accounts.claim)?;
    let supplier_destination = associated_token::get_associated_token_address(
        &ctx.accounts.claim.terms.supplier, &ctx.accounts.claim.terms.asset);
    let collection = EarnedCollection { version: FINANCING_VERSION, domain: ctx.accounts.claim.terms.domain,
        claim: ctx.accounts.claim.key(), payable_lot: ctx.accounts.run.key(),
        accepted_service_grant_hash: ctx.accounts.claim.acceptance_hash,
        reserve_ledger: ctx.accounts.claim.custody, token_account: ctx.accounts.custody.token_account,
        asset: Asset { mint: ctx.accounts.claim.terms.asset, token_program: token::ID },
        supplier: ctx.accounts.claim.terms.supplier, destination: supplier_destination, paid_before, amount };
    require!(operation_id == math(collection.operation_id())?, CoreError::FundingReplay);
    let digest = financial_digest(b"DEXTER:COLLECT-FINANCIER:1", &collection)?;
    // A recorded collection is the same economic event regardless of caller,
    // nonce, transport, Pool choice or an attempted new sponsorship quote.
    // Historical success does not reimburse a sponsor or transfer cash twice.
    if replay(&ctx.accounts.receipt, &digest)? { return Ok(()); }
    valid_principal(&ctx.accounts.financier_principal)?;
    ctx.accounts.facility.check(ctx.accounts.facility.key())?;
    require!(ctx.accounts.claim.revision == expected_revision, CoreError::FundingRevision);
    require!(ctx.accounts.claim.seller_fee_state.gross_settled == ctx.accounts.claim.paid,
        CoreError::FundingConservation);
    require!(ctx.accounts.claim.custody == ctx.accounts.custody.key()
        && ctx.accounts.claim.facility == ctx.accounts.facility.key()
        && ctx.accounts.facility.custody == ctx.accounts.custody.key()
        && ctx.accounts.facility.issuer == ctx.accounts.claim.issuer
        && ctx.accounts.facility.financier == ctx.accounts.financier_principal.key()
        && ctx.accounts.custody.principal == ctx.accounts.financier_principal.key()
        && ctx.accounts.custody.domain == ctx.accounts.claim.terms.domain
        && ctx.accounts.financier_principal.domain == ctx.accounts.claim.terms.domain
        && ctx.accounts.facility.mint == ctx.accounts.mint.key()
        && ctx.accounts.claim.terms.asset == ctx.accounts.mint.key(), CoreError::FundingCustody);
    custody_token(&ctx.accounts.custody, &ctx.accounts.custody_token, ctx.accounts.mint.key())?;
    validate_custody_authority(&ctx.accounts.custody, &ctx.accounts.wallet_binding,
        &ctx.accounts.wallet_authority)?;
    require!(ctx.accounts.destination.key() == supplier_destination
        && ctx.accounts.destination.owner == ctx.accounts.claim.terms.supplier,
        CoreError::FundingTokenRouting);
    math(collection.validate_unpaid_range(
        minus(minus(ctx.accounts.run.gross_earned, ctx.accounts.run.gross_paid)?,
            ctx.accounts.run.corrections.unpaid_reduced)?, ctx.accounts.run.gross_paid))?;
    require!(amount <= ctx.accounts.claim.payable && amount <= ctx.accounts.custody.payable
        && amount <= ctx.accounts.facility.reserved, CoreError::FundingAmount);

    let terms = &ctx.accounts.claim.seller_fee_terms;
    let no_fee = math(terms.cumulative_fee(terms.gross_cap))? == 0;
    let fee_available = if no_fee {
        require!(ctx.accounts.fee_custody.is_none() && ctx.accounts.fee_destination.is_none(), CoreError::FundingTerms);
        false
    } else {
        let fee_custody = ctx.accounts.fee_custody.as_deref().ok_or_else(|| error!(CoreError::FundingTerms))?;
        let fee_destination = ctx.accounts.fee_destination.as_ref().ok_or_else(|| error!(CoreError::FundingTerms))?;
        require_keys_neq!(fee_custody.key(), ctx.accounts.custody.key(), CoreError::FundingTokenRouting);
        fee_destination_available(fee_custody, &fee_destination.to_account_info(), terms,
            &ctx.accounts.claim.terms.domain)?
    };
    if let Some(sponsor) = ctx.accounts.sponsor_custody.as_deref() {
        // Typed duplicate custody copies cannot be independently serialized.
        // This optional quote profile uses distinct custody accounts while its
        // financial books still coalesce principals that hold several roles.
        require_keys_neq!(sponsor.key(), ctx.accounts.custody.key(), CoreError::FundingSponsorship);
        if let Some(fee) = ctx.accounts.fee_custody.as_deref() {
            require_keys_neq!(sponsor.key(), fee.key(), CoreError::FundingSponsorship);
        }
    }
    let (fee_state, settlement) = math(ctx.accounts.claim.seller_fee_state.settle(amount, terms, fee_available))?;
    let mut changes: Vec<Change> = Vec::with_capacity(20);
    let issuer = ctx.accounts.claim.issuer;
    let financier = ctx.accounts.custody.principal;
    let supplier = ctx.accounts.claim.supplier_principal;
    let company = ctx.accounts.claim.terms.principal;
    // The company journal observes collection while its debt stays unchanged.
    changes.push(inc(company, CUSTOMER_PRINCIPAL, 0));
    changes.push(dec(issuer, UNPAID_SUPPLIER, amount));
    changes.push(dec(financier, FUNDING_RESERVED, amount));
    changes.push(inc(financier, CASH_SENT, amount));
    changes.push(dec(supplier, SUPPLIER_RECEIVABLE, amount));
    changes.push(inc(supplier, CASH_RECEIVED, settlement.supplier_net));
    changes.push(inc(supplier, SELLER_FEES_PAID, settlement.fee));
    // Accepted terms determine the exact account set, even when rounding or
    // destination availability makes this execution's deduction zero.
    if !no_fee {
        changes.push(inc(terms.beneficiary, CASH_RECEIVED, settlement.fee));
        changes.push(inc(terms.beneficiary, SELLER_FEES, settlement.fee));
    }

    // Facility interest is on the existing external issuer debt, before this
    // draw changes principal. Issuer-owned capital creates no debt to itself.
    if ctx.accounts.facility.external {
        let (interest, accrued) = math(ctx.accounts.facility.interest.accrue(
            ctx.accounts.facility.drawn, Clock::get()?.unix_timestamp, &ctx.accounts.facility.interest_terms))?;
        ctx.accounts.facility.interest = interest;
        ctx.accounts.facility.drawn = plus(ctx.accounts.facility.drawn, amount)?;
        changes.push(inc(financier, FACILITY_ASSETS, amount));
        changes.push(inc(issuer, FACILITY_DEBT, amount));
        changes.push(inc(issuer, INTEREST_OWED, accrued));
        changes.push(inc(financier, INTEREST_OWNED, accrued));
    }

    transfer_from_custody(&ctx.accounts.custody, &ctx.accounts.wallet_binding,
        &ctx.accounts.wallet_authority, &ctx.accounts.custody_token.to_account_info(),
        &ctx.accounts.destination.to_account_info(), &ctx.accounts.mint, ctx.accounts.token_program.key(),
        settlement.supplier_net)?;
    if settlement.fee > 0 {
        let fee_destination = ctx.accounts.fee_destination.as_ref().ok_or_else(|| error!(CoreError::FundingTerms))?;
        transfer_from_custody(&ctx.accounts.custody, &ctx.accounts.wallet_binding,
            &ctx.accounts.wallet_authority, &ctx.accounts.custody_token.to_account_info(),
            &fee_destination.to_account_info(), &ctx.accounts.mint, ctx.accounts.token_program.key(), settlement.fee)?;
        let fee_custody = ctx.accounts.fee_custody.as_deref_mut().ok_or_else(|| error!(CoreError::FundingTerms))?;
        let after = read_token(&fee_destination.to_account_info())?;
        fee_custody.credit(settlement.fee)?;
        fee_custody.check(after.amount)?;
    }
    ctx.accounts.custody_token.reload()?;
    ctx.accounts.custody.payable = minus(ctx.accounts.custody.payable, amount)?;
    ctx.accounts.custody.accounted_cash = minus(ctx.accounts.custody.accounted_cash, amount)?;
    ctx.accounts.custody.revision = plus(ctx.accounts.custody.revision, 1)?;
    ctx.accounts.custody.check(ctx.accounts.custody_token.amount)?;
    ctx.accounts.facility.reserved = minus(ctx.accounts.facility.reserved, amount)?;
    ctx.accounts.facility.revision = plus(ctx.accounts.facility.revision, 1)?;
    ctx.accounts.claim.payable = minus(ctx.accounts.claim.payable, amount)?;
    ctx.accounts.claim.paid = plus(ctx.accounts.claim.paid, amount)?;
    ctx.accounts.claim.seller_fee_state = fee_state;
    ctx.accounts.claim.revision = plus(ctx.accounts.claim.revision, 1)?;
    ctx.accounts.run.gross_paid = plus(ctx.accounts.run.gross_paid, amount)?;
    ctx.accounts.claim.check(ctx.accounts.claim.key())?;
    ctx.accounts.run.check(ctx.accounts.run.key(), ctx.accounts.claim.key(), &ctx.accounts.claim)?;
    ctx.accounts.facility.check(ctx.accounts.facility.key())?;

    let mut data = record_base(company, operation_id, digest, FINANCE_COLLECT, ctx.accounts.mint.key(),
        ctx.accounts.claim.key(), ctx.accounts.run.key(), amount);
    data.supplier_net = settlement.supplier_net;
    data.seller_fee = settlement.fee;
    data.seller_fee_waived = settlement.waived;
    data.debit_principal = financier;
    data.debit_authority = ctx.accounts.custody.wallet;
    data.debit_from = ctx.accounts.custody.token_account;
    apply_sponsorship(sponsorship.as_ref(), SponsorshipAccounts {
        payer_principal: ctx.accounts.reimbursement_principal.as_deref(),
        payer_owner: ctx.accounts.reimbursement_authority.as_ref(),
        from: ctx.accounts.reimbursement_from.as_ref().map(|a| a.to_account_info()),
        destination: ctx.accounts.sponsor_destination.as_ref().map(|a| a.to_account_info()),
        sponsor_custody: ctx.accounts.sponsor_custody.as_deref_mut(),
    }, &SponsorOperation { operation_id, domain: ctx.accounts.claim.terms.domain,
        custody_profile: ctx.accounts.custody.profile, supplier_principal: supplier,
        supplier_destination, supplier_net: settlement.supplier_net },
        &ctx.accounts.mint, ctx.accounts.token_program.key(), &mut data, &mut changes)?;
    journal(&mut ctx.accounts.receipt, data, &changes, ctx.remaining_accounts,
        &ctx.accounts.pool_program.to_account_info(), service_without_pool, false)?;
    Ok(())
}

/// Authority and exact SPL transfer for either admitted cash profile. The caller
/// checks the aggregate cash floor before and after the complete operation;
/// checking it between the two gross-to-net legs would reject a valid split.
pub fn transfer_from_custody<'info>(custody: &Account<'info, FinancierCustody>,
    binding: &AccountInfo<'info>, authority: &AccountInfo<'info>, from: &AccountInfo<'info>,
    destination: &AccountInfo<'info>, mint: &Account<'info, Mint>, token_program: Pubkey, amount: u64) -> Result<()> {
    validate_custody_authority(custody, binding, authority)?;
    require!(token_program == token::ID && custody.mint == mint.key()
        && custody.token_account == from.key(), CoreError::FundingTokenRouting);
    let source = read_token(from)?;
    require!(source.owner == custody.wallet && source.mint == mint.key()
        && from.key() == associated_token::get_associated_token_address(&custody.wallet, &mint.key()),
        CoreError::FundingTokenRouting);
    if custody.profile == WALLET_CASH {
        transfer_checked_exact(from, destination, authority, mint, token_program, amount, &[])
    } else {
        let principal = custody.principal; let mint_key = mint.key();
        let (_, bump) = Pubkey::find_program_address(&[b"escrow-authority", principal.as_ref(), mint_key.as_ref()], &crate::ID);
        let bump_bytes = [bump];
        let seeds: &[&[u8]] = &[b"escrow-authority", principal.as_ref(), mint_key.as_ref(), &bump_bytes];
        transfer_checked_exact(from, destination, authority, mint, token_program, amount, &[seeds])
    }
}

pub fn validate_custody_authority(custody: &Account<FinancierCustody>, binding: &AccountInfo,
    authority: &AccountInfo) -> Result<()> {
    custody.check_key(custody.key())?;
    require!(authority.key() == custody.wallet && !authority.executable, CoreError::FundingWalletAuthority);
    match custody.profile {
        WALLET_CASH => {
            let bound = principal_registry::read_funding_wallet_binding(binding)?;
            require!(binding.key() == custody.binding && bound.principal == custody.principal
                && bound.wallet == custody.wallet && authority.is_signer,
                CoreError::FundingWalletAuthority);
        }
        ESCROW_CASH => {
            let expected = Pubkey::find_program_address(&[b"escrow-authority", custody.principal.as_ref(), custody.mint.as_ref()], &crate::ID).0;
            require!(custody.wallet == expected && binding.key() == crate::ID && custody.binding == crate::ID,
                CoreError::FundingWalletAuthority);
        }
        _ => return err!(CoreError::FundingWalletAuthority),
    }
    Ok(())
}

fn read_token(info: &AccountInfo) -> Result<TokenAccount> {
    require_keys_eq!(*info.owner, token::ID, CoreError::FundingTokenRouting);
    TokenAccount::try_deserialize(&mut &info.try_borrow_data()?[..])
}

fn fee_destination_available(custody: &Account<FinancierCustody>, info: &AccountInfo,
    terms: &SellerFeeTerms, domain: &[u8; 32]) -> Result<bool> {
    custody.check_key(custody.key())?;
    require!(custody.domain == *domain && custody.principal == terms.beneficiary
        && custody.wallet == terms.beneficiary_wallet && custody.token_account == terms.destination
        && custody.mint == terms.asset.mint && terms.asset.token_program == token::ID
        && info.key() == terms.destination
        && info.key() == associated_token::get_associated_token_address(&custody.wallet, &custody.mint),
        CoreError::FundingTokenRouting);
    if info.owner == &System::id() && info.data_is_empty() { return Ok(false); }
    let token = read_token(info)?;
    require!(token.mint == terms.asset.mint && token.owner == terms.beneficiary_wallet,
        CoreError::FundingTokenRouting);
    if token.state == token::spl_token::state::AccountState::Frozen { return Ok(false); }
    require!(token.state == token::spl_token::state::AccountState::Initialized,
        CoreError::FundingTokenRouting);
    // A known deficit is not forgiven or relabelled as fee income. The
    // accepted unavailable-destination rule waives this deduction instead.
    if token.amount < custody.accounted_cash { return Ok(false); }
    custody.check(token.amount)?;
    require!(token.delegate.is_none() && token.close_authority.is_none(), CoreError::FundingCashControl);
    Ok(true)
}
