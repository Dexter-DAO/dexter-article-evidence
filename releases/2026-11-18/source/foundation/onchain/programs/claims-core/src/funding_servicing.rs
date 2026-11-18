//! Shared accepted-right financial mutations. Each caller first authenticates
//! its billing/release profile; these functions retain the existing accounting,
//! Registry CPI, replay and complete journal rules for both supported profiles.
use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, TokenAccount};
use dexter_contracts::{plus, minus, StageArgs, REGISTRY_CONSUMER_SEED};
use dexter_contracts::commercial::InterestState;
use dexter_contracts::financing::PrincipalAmounts;
use dexter_contracts::funded::*;
use principal_registry::{self, Principal, Budget, Reservation, program::PrincipalRegistry};
use crate::{CoreError, funding_state::*, funding_journal::{journal, inc, dec}};

pub(crate) struct EarnedRunAccounts<'a, 'info> {
    pub payer: &'a Signer<'info>,
    pub principal: &'a Account<'info, Principal>,
    pub budget: &'a mut Account<'info, Budget>,
    pub reservation: &'a mut Account<'info, Reservation>,
    pub registry_operation: &'a UncheckedAccount<'info>,
    pub registry_authority: &'a UncheckedAccount<'info>,
    pub registry_program: &'a Program<'info, PrincipalRegistry>,
    pub line: &'a mut Account<'info, CustomerCreditLine>,
    pub claim: &'a mut Account<'info, FundedClaim>,
    pub custody: &'a mut Account<'info, FinancierCustody>,
    pub custody_token: &'a Account<'info, TokenAccount>,
    pub run: &'a mut Account<'info, FundedRun>,
    pub payment: &'a mut Account<'info, FundedPayment>,
    pub receipt: &'a mut Account<'info, FinanceReceipt>,
    pub mint: &'a Account<'info, Mint>,
    pub pool_program: &'a UncheckedAccount<'info>,
    pub system_program: &'a Program<'info, System>,
}

pub(crate) struct EarnedRunTerms {
    pub payment_id: [u8;32], pub namespace: [u8;32], pub run_id: [u8;32],
    pub service_unit: [u8;32], pub external_contract_hash: [u8;32],
    pub units: u64, pub unit_price: u64, pub expected_revision: u64,
    pub service_without_pool: bool, pub capture_digest: [u8;32],
    /// Trusted reporting expires with the work window. Evidence billing already
    /// authenticated timely records and may finalize after that window.
    pub enforce_deadline: bool,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn establish_run<'info>(
    a: EarnedRunAccounts<'_, 'info>, args: EarnedRunTerms,
    registry_bump: u8, run_bump: u8, payment_bump: u8,
    remaining_accounts: &[AccountInfo<'info>],
) -> Result<()> {
    let EarnedRunTerms { payment_id, namespace, run_id, service_unit, external_contract_hash,
        units, unit_price, expected_revision, service_without_pool, capture_digest, enforce_deadline } = args;
    let claim_key = a.claim.key(); let run_key = a.run.key();
    a.claim.check(claim_key)?;
    require!(namespace == a.claim.usage_namespace && service_unit == a.claim.service_unit
        && unit_price == a.claim.unit_price && external_contract_hash == a.claim.external_contract_hash
        && units > 0, CoreError::FundingUsage);
    let amount = units.checked_mul(unit_price).ok_or_else(|| error!(CoreError::FundingAmount))?;
    let operation_id = funded_run_operation_id(&a.claim.supplier_principal, &namespace, &run_id);
    let payment_digest = financial_digest(b"DEXTER:FINANCED-PAYMENT-ALIAS:1",
        &(a.principal.key(), payment_id, run_key, capture_digest))?;
    // One canonical run earns once. A matching worker payment ID only records
    // an alias after authenticating the original run and financial receipt.
    if a.payment.version != 0 {
        require!(
            a.payment.version == FUNDED_VERSION && a.payment.company == a.principal.key()
                && a.payment.payment_id == payment_id && a.payment.run == run_key
                && a.payment.receipt == a.receipt.key() && a.payment.request_digest == payment_digest,
            CoreError::FundingReplay
        );
    }
    if a.run.version != 0 {
        a.run.check(run_key, claim_key, &a.claim)?;
        require!(
            a.run.capture_digest == capture_digest && a.run.capture_receipt == a.receipt.key()
                && replay(&a.receipt, &capture_digest)?,
            CoreError::FundingReplay
        );
        a.payment.set_inner(FundedPayment {
            version: FUNDED_VERSION, bump: payment_bump, company: a.principal.key(), payment_id,
            run: run_key, receipt: a.receipt.key(), request_digest: payment_digest,
        });
        return Ok(());
    }

    // Fresh earning consumes accepted capacity. Allocate fees cumulatively from
    // gross captures; the final capture receives the accepted rounding remainder.
    require!(a.receipt.data.version == 0 && a.claim.revision == expected_revision, CoreError::FundingRevision);
    require!(
        (!enforce_deadline || Clock::get()?.unix_timestamp <= a.claim.terms.expires_at) && amount <= a.claim.remaining,
        CoreError::FundingAmount
    );
    a.line.check(a.line.key())?;
    reservation_matches(&a.reservation, &a.claim)?;
    require!(
        a.line.key() == a.claim.credit_line && a.principal.key() == a.claim.terms.principal
            && a.custody.key() == a.claim.custody && a.mint.key() == a.claim.terms.asset,
        CoreError::FundingClaim
    );
    custody_token(&a.custody, &a.custody_token, a.mint.key())?;
    let prior_earned = a.claim.captured_total()?;
    let cumulative_fee_cap = if amount == a.claim.remaining {
        minus(a.claim.fee_cap, a.claim.released_unallocated_fee_caps)?
    } else {
        ((plus(prior_earned, amount)? as u128) * (a.claim.fee_cap as u128)
            / (a.claim.terms.cap as u128)) as u64
    };
    let run_fee_cap = minus(cumulative_fee_cap, a.claim.allocated_run_fee_caps)?;
    a.claim.allocated_run_fee_caps = cumulative_fee_cap;
    let mut interest_terms = a.claim.interest_terms.clone();
    interest_terms.maximum_interest = run_fee_cap;

    // Keep fee allocation above the existing Registry CPI. Any later failure
    // rolls it back together with reservation consumption and financial writes.
    let bump = [registry_bump];
    let seeds: &[&[u8]] = &[REGISTRY_CONSUMER_SEED, &bump];
    principal_registry::cpi::consume_completed_run(
        CpiContext::new_with_signer(
            a.registry_program.key(),
            principal_registry::cpi::accounts::ConsumeCompletedRun {
                core_authority: a.registry_authority.to_account_info(),
                payer: a.payer.to_account_info(), principal: a.principal.to_account_info(),
                budget: a.budget.to_account_info(), reservation: a.reservation.to_account_info(),
                operation: a.registry_operation.to_account_info(), system_program: a.system_program.to_account_info(),
            },
            &[seeds],
        ),
        StageArgs { operation_id, amount, expected_revision, service_without_pool },
    )?;

    // Move accepted reserve to earned payable and customer debt. Earning itself
    // transfers no cash; preserve each account's explicit revision and history.
    a.claim.remaining = minus(a.claim.remaining, amount)?;
    a.claim.payable = plus(a.claim.payable, amount)?;
    a.claim.debt = plus(a.claim.debt, amount)?;
    a.claim.revision = plus(a.claim.revision, 1)?;
    a.line.reserved = minus(a.line.reserved, amount)?;
    a.line.principal_outstanding = plus(a.line.principal_outstanding, amount)?;
    a.line.revision = plus(a.line.revision, 1)?;
    a.custody.unused = minus(a.custody.unused, amount)?;
    a.custody.payable = plus(a.custody.payable, amount)?;
    a.custody.revision = plus(a.custody.revision, 1)?;
    a.run.set_inner(FundedRun {
        version: FUNDED_RUN_VERSION, bump: run_bump, operation_id, claim: claim_key, namespace, run_id,
        service_unit, units, unit_price, supplier: a.claim.terms.supplier, supplier_principal: a.claim.supplier_principal,
        debtor: a.claim.terms.principal, mint: a.mint.key(), gross_earned: amount, gross_paid: 0,
        current_creditor: a.claim.issuer, creditor_custody: a.claim.issuer_custody, ownership_revision: 1,
        principal: PrincipalAmounts { originated: amount, outstanding: amount, repaid: 0, extinguished: 0 },
        interest: math(InterestState::new(Clock::get()?.unix_timestamp, &interest_terms))?, interest_terms,
        acquisition_basis: amount, provenance_hash: capture_digest, capture_receipt: a.receipt.key(),
        capture_digest, fee_authority_released: 0, corrections: Default::default(),
    });
    a.payment.set_inner(FundedPayment {
        version: FUNDED_VERSION, bump: payment_bump, company: a.principal.key(), payment_id,
        run: run_key, receipt: a.receipt.key(), request_digest: payment_digest,
    });
    a.claim.check(claim_key)?;
    a.custody.check(a.custody_token.amount)?;

    // The journal owns complete, coalesced book effects and optional projection.
    // The zero financier delta deliberately keeps its book in the observed set.
    let company = a.claim.terms.principal;
    let issuer = a.claim.issuer;
    let supplier = a.claim.supplier_principal;
    let changes = [
        dec(company, CUSTOMER_RESERVED, amount), inc(company, CUSTOMER_PRINCIPAL, amount),
        dec(issuer, UNUSED_SUPPLIER, amount), inc(issuer, UNPAID_SUPPLIER, amount), inc(issuer, OWNED_PRINCIPAL, amount),
        dec(supplier, SUPPLIER_COMMITTED, amount), inc(supplier, SUPPLIER_RECEIVABLE, amount),
        inc(a.custody.principal, FUNDING_RESERVED, 0),
    ];
    let data = record_base(company, operation_id, capture_digest, FINANCE_EARN, a.mint.key(), claim_key, run_key, amount);
    journal(a.receipt, data, &changes, remaining_accounts, &a.pool_program, service_without_pool, false)
}

pub(crate) struct ReleaseAccounts<'a, 'info> {
    pub payer: &'a Signer<'info>,
    pub principal: &'a Account<'info, Principal>,
    pub budget: &'a mut Account<'info, Budget>,
    pub reservation: &'a mut Account<'info, Reservation>,
    pub registry_operation: &'a UncheckedAccount<'info>,
    pub registry_authority: &'a UncheckedAccount<'info>,
    pub registry_program: &'a Program<'info, PrincipalRegistry>,
    pub line: &'a mut Account<'info, CustomerCreditLine>,
    pub claim: &'a mut Account<'info, FundedClaim>,
    pub facility: &'a mut Account<'info, FundingFacility>,
    pub custody: &'a mut Account<'info, FinancierCustody>,
    pub custody_token: &'a Account<'info, TokenAccount>,
    pub receipt: &'a mut Account<'info, FinanceReceipt>,
    pub mint: &'a Account<'info, Mint>,
    pub pool_program: &'a UncheckedAccount<'info>,
    pub system_program: &'a Program<'info, System>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn release_unused<'info>(
    a: ReleaseAccounts<'_, 'info>, id: [u8;32], amount: u64, expected_revision: u64,
    service_without_pool: bool, registry_bump: u8, remaining_accounts: &[AccountInfo<'info>],
    surrender: Option<(&Account<'info, Principal>, Option<&Signer<'info>>)>,
) -> Result<()> {
    let claim_key = a.claim.key();
    let digest=financial_digest(b"DEXTER:RELEASE-FINANCIER-UNUSED:1",&(claim_key,id,amount))?;
    if replay(&a.receipt,&digest)?{return Ok(());}
    a.claim.check(claim_key)?;a.line.check(a.line.key())?;a.facility.check(a.facility.key())?;
    require!(a.claim.revision==expected_revision && amount>0 && amount<=a.claim.remaining,CoreError::FundingRevision);
    require!(a.principal.key()==a.claim.terms.principal
        && a.line.key()==a.claim.credit_line && a.facility.key()==a.claim.facility && a.custody.key()==a.claim.custody
        && a.mint.key()==a.claim.terms.asset,CoreError::FundingClaim);
    reservation_matches(&a.reservation,&a.claim)?;custody_token(&a.custody,&a.custody_token,a.mint.key())?;
    if let Some((supplier_principal, supplier)) = surrender {
        valid_principal(supplier_principal)?;
        require_keys_eq!(supplier_principal.key(), a.claim.supplier_principal, CoreError::FundingClaim);
        if Clock::get()?.unix_timestamp <= a.claim.terms.expires_at {
            let signer = supplier.ok_or_else(|| error!(CoreError::FundingReleaseAuthority))?;
            require_keys_eq!(signer.key(), supplier_principal.owner, CoreError::FundingReleaseAuthority);
        }
    }
    // Run closures consume their own allocated fee caps. This counter concerns
    // only never-earned work, so closing a run cannot subtract capacity twice.
    let total_released=plus(a.claim.released,amount)?;
    let target_unallocated_release=if amount==a.claim.remaining {
        minus(a.claim.fee_cap,a.claim.allocated_run_fee_caps)?
    } else { ((total_released as u128)*(a.claim.fee_cap as u128)/(a.claim.terms.cap as u128)) as u64 };
    let fee_release=minus(target_unallocated_release,a.claim.released_unallocated_fee_caps)?;
    let authority_release=plus(amount,fee_release)?;
    let bump=[registry_bump];let seeds:&[&[u8]]=&[REGISTRY_CONSUMER_SEED,&bump];
    principal_registry::cpi::release(CpiContext::new_with_signer(a.registry_program.key(),principal_registry::cpi::accounts::ServiceReservation{
        core_authority:a.registry_authority.to_account_info(),payer:a.payer.to_account_info(),principal:a.principal.to_account_info(),
        budget:a.budget.to_account_info(),reservation:a.reservation.to_account_info(),operation:a.registry_operation.to_account_info(),
        system_program:a.system_program.to_account_info(),
    },&[seeds]),StageArgs{operation_id:id,amount:authority_release,expected_revision,service_without_pool})?;
    a.claim.remaining=minus(a.claim.remaining,amount)?;a.claim.released=total_released;
    a.claim.released_unallocated_fee_caps=target_unallocated_release;
    a.claim.released_fee_caps=plus(a.claim.released_fee_caps,fee_release)?;
    a.claim.fee_authority_remaining=minus(a.claim.fee_authority_remaining,fee_release)?;
    a.claim.revision=plus(a.claim.revision,1)?;
    a.line.reserved=minus(a.line.reserved,amount)?;a.line.fee_exposure=minus(a.line.fee_exposure,fee_release)?;
    a.line.revision=plus(a.line.revision,1)?;
    a.facility.reserved=minus(a.facility.reserved,amount)?;a.facility.revision=plus(a.facility.revision,1)?;
    a.custody.unused=minus(a.custody.unused,amount)?;a.custody.revision=plus(a.custody.revision,1)?;
    a.claim.check(claim_key)?;a.custody.check(a.custody_token.amount)?;
    let company=a.claim.terms.principal;let issuer=a.claim.issuer;let supplier=a.claim.supplier_principal;
    let changes=[dec(company,CUSTOMER_RESERVED,amount),dec(company,CREDIT_CHARGE_EXPOSURE,fee_release),
        dec(issuer,UNUSED_SUPPLIER,amount),dec(supplier,SUPPLIER_COMMITTED,amount),dec(a.custody.principal,FUNDING_RESERVED,amount)];
    let data=record_base(company,id,digest,FINANCE_RELEASE,a.mint.key(),claim_key,Pubkey::default(),amount);
    journal(a.receipt,data,&changes,remaining_accounts,&a.pool_program,service_without_pool,false)
}
