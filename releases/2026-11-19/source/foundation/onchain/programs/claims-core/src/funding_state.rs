//! Authoritative cash funding, customer obligations and delivery-linked title.
//! Reconstructed from the accepted contracts; all execution evidence is new.
use anchor_lang::prelude::*;
use crate::CoreError;
use anchor_spl::{associated_token, token::{self, Mint, TokenAccount}};
use dexter_contracts::{self as contracts, plus, minus, TabTerms, CORE_ID, REGISTRY_ID};
use dexter_contracts::commercial::{InterestTerms, InterestState, SellerFeeTerms, SellerFeeState};
use dexter_contracts::financing::{CustodyAmounts, FinanceResult, PrincipalAmounts};
use dexter_contracts::corrections::{CorrectionAmounts, RepaymentRefundUsage};
use dexter_contracts::funded::{FinanceBookData, FinanceDetail, FUNDED_VERSION};
use dexter_contracts::window::{OrderWindow, WindowRefusal};
use principal_registry::{Principal, Reservation};
use crate::settlement_hold::SettlementHold;

pub const WALLET_CASH: u8 = 1;
pub const ESCROW_CASH: u8 = 2;

/// Approval for new risk, separate from the continuing facility's obligations.
/// Recovery invalidates this approval without moving or releasing any backing.
#[account]
#[derive(InitSpace)]
pub struct FacilityAuthority {
    pub deposit_payer: Pubkey,
    pub version: u16, pub bump: u8, pub facility: Pubkey,
    pub financier: Pubkey, pub authority_generation: u64,
    pub approval_digest: [u8;32],
}
impl FacilityAuthority {
    pub fn check(&self, key: Pubkey, facility: Pubkey, financier: &Account<Principal>) -> Result<()> {
        let (expected, bump) = Pubkey::find_program_address(&[b"facility-authority", facility.as_ref()], &CORE_ID);
        require!(self.version == 1 && self.bump == bump && key == expected
            && self.facility == facility && self.financier == financier.key()
            && self.authority_generation == financier.authority_generation,
            CoreError::FundingAuthorityGeneration);
        valid_principal(financier)
    }
}

/// Change 5 layouts: the Tab carries its receipt book, runs live under it.
pub const FUNDED_CLAIM_VERSION: u16 = 3;
pub const FUNDED_RUN_VERSION: u16 = 3;
pub const REPAYMENT_RECORD_VERSION: u16 = 1;

/// Admit a chosen-amount money move's number into the window of the record it
/// writes (design note section 3.13): once ever; refused at zero, below the
/// window or past the jump cap. The record's window has one allocator, the
/// party who initiates these moves, and its SDK persists the number with the
/// intent before signing and reuses it for any rebuild.
pub fn admit_number(window: &mut OrderWindow, number: u64) -> Result<()> {
    window.admit(number).map_err(|refusal| match refusal {
        WindowRefusal::Zero => error!(CoreError::NumberZero),
        WindowRefusal::TooFar => error!(CoreError::NumberTooFar),
        WindowRefusal::BelowWindow => error!(CoreError::NumberBelowWindow),
        WindowRefusal::Used => error!(CoreError::NumberUsed),
    })
}

/// The operation id of an earned run. Delivered-work identity follows the
/// continuing supplier Principal; under Change 5 a run is unique within its Tab.
pub fn funded_run_operation_id(supplier_principal: &Pubkey, namespace: &[u8; 32], run_id: &[u8; 32]) -> [u8; 32] {
    contracts::completed_run_operation_id(supplier_principal, namespace, run_id)
}

/// A run lives under its Tab: `["run", tab, run_id]`. A closed run's address is
/// never reused, because a run closes only after its Tab can no longer earn.
pub fn funded_run_address(tab: &Pubkey, run_id: &[u8; 32]) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"run", tab.as_ref(), run_id], &CORE_ID)
}

/// A Tab lives at `["tab", mandate, order]`; the mandate's window admits each order once.
pub fn tab_address(mandate: &Pubkey, order: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"tab", mandate.as_ref(), &order.to_le_bytes()], &CORE_ID)
}

pub fn repayment_record_address(run: &Pubkey, payer_source: &Pubkey, creditor: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"repayment", run.as_ref(), payer_source.as_ref(), creditor.as_ref()], &CORE_ID)
}

/// A principal's book for one mint. Its window numbers the money moves the
/// principal pays from owner cash (interest refunds, seller-fee credits,
/// correction cash legs): the book is the one record of the payer each of them
/// writes. Books never close, so a number can never be admitted twice.
#[account]
#[derive(InitSpace)]
pub struct FinanceBook { pub deposit_payer: Pubkey, pub data: FinanceBookData, pub window: OrderWindow }

#[account]
#[derive(InitSpace)]
pub struct FinancierCustody {
    pub deposit_payer: Pubkey,
    pub version: u16, pub bump: u8, pub domain: [u8; 32], pub principal: Pubkey,
    pub binding: Pubkey, pub wallet: Pubkey, pub token_account: Pubkey, pub mint: Pubkey,
    pub profile: u8,
    /// Explicit allocation only. An unsolicited transfer does not become income.
    pub accounted_cash: u64, pub unused: u64, pub payable: u64,
    pub contributed_capital: u64,
    /// Cumulative authorized uncommitted debits: withdrawals, sends and swap input.
    /// Includes eligible earned cash; it is not a valuation or income measure.
    pub withdrawn_cash: u64,
    pub revision: u64,
    /// Paid the rent of the custody's token account (escrow ATAs), which records no payer itself.
    pub token_deposit_payer: Pubkey,
    /// Numbers of the custody's withdrawals, contributions and token moves,
    /// allocated by its owner. Custody never closes.
    pub window: OrderWindow,
}
impl FinancierCustody {
    pub fn amounts(&self) -> CustodyAmounts { CustodyAmounts { unused: self.unused, earned_unpaid: self.payable } }
    pub fn protected(&self) -> Result<u64> { math(self.amounts().protected()) }
    pub fn check(&self, actual: u64) -> Result<()> {
        require!(self.version == FUNDED_VERSION && self.revision > 0, CoreError::FundingSchema);
        require!(actual >= self.accounted_cash && self.accounted_cash >= self.protected()?, CoreError::FundingCashShortfall);
        Ok(())
    }
    pub fn check_key(&self, key: Pubkey) -> Result<()> {
        require!(self.version == FUNDED_VERSION && self.revision > 0, CoreError::FundingSchema);
        let (expected, bump) = Pubkey::find_program_address(&[b"custody-reserve", self.token_account.as_ref()], &CORE_ID);
        require!(key == expected && self.bump == bump, CoreError::FundingCustody);
        Ok(())
    }
    pub fn credit(&mut self, amount: u64) -> Result<()> {
        self.accounted_cash = plus(self.accounted_cash, amount)?;
        self.revision = plus(self.revision, 1)?; Ok(())
    }
    /// The shared withdrawal/spending floor is expressed in this mint's actual
    /// units. It neither adopts unsolicited surplus nor spends another facility's
    /// accepted backing. The caller separately authenticates capital ownership.
    pub fn debit_uncommitted(&mut self, actual: u64, amount: u64) -> Result<()> {
        self.check(actual)?;
        require!(amount>0 && amount<=minus(self.accounted_cash,self.protected()?)?,
            CoreError::FundingCashShortfall);
        self.accounted_cash=minus(self.accounted_cash,amount)?;
        self.withdrawn_cash=plus(self.withdrawn_cash,amount)?;
        self.revision=plus(self.revision,1)?;
        self.check(minus(actual,amount)?)
    }
}

#[account]
#[derive(InitSpace)]
pub struct FundingFacility {
    pub deposit_payer: Pubkey,
    pub version: u16, pub bump: u8, pub facility_id: [u8; 32],
    pub financier: Pubkey, pub issuer: Pubkey, pub custody: Pubkey, pub mint: Pubkey,
    pub external: bool, pub commitment_limit: u64, pub reserved: u64,
    pub drawn: u64, pub repaid: u64, pub accepts_new: bool,
    pub accept_until: i64, pub revision: u64, pub terms_hash: [u8; 32], pub opening_hash:[u8;32],
    pub interest_terms: InterestTerms, pub interest: InterestState,
}
impl FundingFacility {
    pub fn check(&self, key: Pubkey) -> Result<()> {
        let (p, bump) = Pubkey::find_program_address(&[b"funding-facility", self.financier.as_ref(), &self.facility_id], &CORE_ID);
        require!(self.version == FUNDED_VERSION && self.revision > 0 && key == p && self.bump == bump,
            CoreError::FundingFacility);
        require!(self.external == (self.financier != self.issuer), CoreError::FundingFacility);
        plus(self.reserved, self.drawn)?; math(self.interest.validate(&self.interest_terms))?;
        Ok(())
    }
}

#[account]
#[derive(InitSpace)]
pub struct CustomerCreditLine {
    pub deposit_payer: Pubkey,
    pub version: u16, pub bump: u8, pub line_id: [u8; 32],
    pub company: Pubkey, pub issuer: Pubkey, pub issuer_custody: Pubkey,
    pub facility: Pubkey, pub mint: Pubkey, pub limit: u64,
    pub reserved: u64, pub principal_outstanding: u64, pub fee_exposure: u64,
    /// Lifetime cap allocation is distinct from outstanding risk usage.
    pub allocated_fee_caps: u64,
    pub interest_terms: InterestTerms, pub enabled: bool, pub revision: u64,
    pub assignment_policy_hash: [u8; 32],
}
impl CustomerCreditLine {
    pub fn check(&self, key: Pubkey) -> Result<()> {
        let (p, bump) = Pubkey::find_program_address(&[b"credit-line", self.company.as_ref(), &self.line_id], &CORE_ID);
        require!(self.version == FUNDED_VERSION && self.revision > 0 && key == p && self.bump == bump,
            CoreError::FundingCreditLine);
        plus(plus(self.reserved, self.principal_outstanding)?, self.fee_exposure)?;
        require!(self.allocated_fee_caps <= self.interest_terms.maximum_interest, CoreError::FundingChargeCap);
        Ok(())
    }
}

/// The Tab: an accepted order, its money state and its receipt book.
#[account]
#[derive(InitSpace)]
pub struct FundedClaim {
    pub deposit_payer: Pubkey,
    pub version: u16, pub bump: u8, pub terms: TabTerms,
    /// Accepted completed-run price preimage, available directly to readers.
    pub usage_namespace: [u8;32], pub service_unit: [u8;32], pub unit_price: u64,
    pub external_contract_hash: [u8;32],
    pub acceptance_hash: [u8; 32], pub credit_line: Pubkey,
    pub assignment_policy_hash: [u8; 32], pub facility: Pubkey, pub custody: Pubkey,
    pub issuer: Pubkey, pub issuer_custody: Pubkey, pub supplier_principal: Pubkey,
    /// Never increases (invariant I1): only `lower_remaining` writes it after the commit.
    pub remaining: u64, pub payable: u64, pub debt: u64, pub paid: u64,
    pub repaid: u64, pub released: u64,
    pub fee_cap: u64, pub fee_authority_remaining: u64, pub allocated_run_fee_caps: u64,
    pub released_fee_caps: u64, pub released_unallocated_fee_caps: u64,
    pub interest_outstanding: u64, pub interest_terms: InterestTerms,
    /// Immutable parent allocation. Accepted servicing never consults the parent.
    pub seller_fee_agreement: Pubkey, pub seller_fee_allocation: u64,
    pub seller_fee_terms: SellerFeeTerms, pub seller_fee_state: SellerFeeState,
    /// Captured principal and cash history never rewind.
    pub principal_extinguished: u64,
    pub corrections: CorrectionAmounts,
    /// Receipt book: every step (events, fingerprint).
    pub steps: u32,
    /// Money steps (the commit and every step that changes money or obligations):
    /// the staleness guard. Closes, admissions and holds do not count. A landed
    /// step is never applied again: a retry is refused by this guard or by the
    /// window of its number.
    pub money_steps: u32,
    /// h_steps.
    pub fingerprint: [u8; 32],
    /// Runs not yet closed.
    pub open_runs: u32,
    /// Repayment records not yet closed. A record closes only after its run, and
    /// the Tab only after every record.
    pub open_records: u32,
    /// The settlement hold last set (design section 9): in force while it is set
    /// and the clock is before its `until`. A release unsets it; expiry needs no call.
    pub hold: SettlementHold,
    /// Each party sets at most one hold per Tab.
    pub buyer_hold_used: bool,
    pub seller_hold_used: bool,
}
impl FundedClaim {
    pub fn check(&self, key: Pubkey) -> Result<()> {
        let (p,bump)=tab_address(&self.terms.mandate,self.terms.order);
        require!(self.version == FUNDED_CLAIM_VERSION && p == key && bump == self.bump,
            CoreError::FundingClaim);
        let captured = self.captured_total()?;
        require!(plus(plus(self.remaining, self.released)?, captured)? == self.terms.cap,
            CoreError::FundingConservation);
        require!(plus(plus(self.payable, self.paid)?, self.corrections.unpaid_reduced)? == captured,
            CoreError::FundingConservation);
        math(self.corrections.validate(captured, self.paid, &PrincipalAmounts {
            originated: captured, outstanding: self.debt, repaid: self.repaid,
            extinguished: self.principal_extinguished,
        }))?;
        require!(self.allocated_run_fee_caps <= self.fee_cap && self.released_fee_caps <= self.fee_cap
            && self.released_unallocated_fee_caps <= self.released_fee_caps, CoreError::FundingChargeCap);
        math(self.seller_fee_state.validate(&self.seller_fee_terms))?;
        crate::seller_fees::check_claim_allocation(self.supplier_principal,&self.seller_fee_terms,
            self.seller_fee_agreement,self.seller_fee_allocation)?;
        self.check_hold()?;
        Ok(())
    }
    pub fn captured_total(&self) -> Result<u64> {
        plus(plus(self.debt, self.repaid)?, self.principal_extinguished)
    }
    pub fn authority_terms(&self) -> Result<TabTerms> {
        let mut terms=self.terms.clone(); terms.cap=plus(terms.cap,self.fee_cap)?; Ok(terms)
    }
    /// The only writer of `remaining` after the commit: it can only go down.
    pub fn lower_remaining(&mut self, amount: u64) -> Result<()> {
        self.remaining = minus(self.remaining, amount)?; Ok(())
    }
    /// The number the next step's event header carries.
    pub fn next_step(&self) -> Result<u32> {
        self.steps.checked_add(1).ok_or_else(|| error!(CoreError::FundingSchema))
    }
    /// The staleness guard of a money step.
    pub fn guard(&self, expected_steps: u32) -> Result<()> {
        require!(expected_steps == self.money_steps, CoreError::TabStale); Ok(())
    }
    pub fn record_step(&mut self, fingerprint: [u8; 32], money: bool) -> Result<()> {
        self.steps = self.next_step()?;
        self.fingerprint = fingerprint;
        if money {
            self.money_steps = self.money_steps.checked_add(1).ok_or_else(|| error!(CoreError::FundingSchema))?;
        }
        Ok(())
    }
    /// Nothing is owed or can be earned: every condition `close_tab` checks
    /// except the runs, the bill and a hold.
    pub fn settled(&self) -> bool {
        self.remaining == 0 && self.payable == 0 && self.debt == 0 && self.interest_outstanding == 0
            && self.fee_authority_remaining == 0
    }
}

/// A delivered run, under its Tab. Tab-wide facts (namespace, unit, price,
/// supplier, debtor, mint) are read from the Tab.
#[account]
#[derive(InitSpace)]
pub struct FundedRun {
    pub deposit_payer: Pubkey,
    pub version: u16, pub bump: u8, pub claim: Pubkey, pub run_id: [u8; 32],
    /// A second worker billing the same run with the same capture is a no-op.
    pub capture_digest: [u8; 32],
    pub gross_earned: u64, pub gross_paid: u64,
    pub current_creditor: Pubkey, pub creditor_custody: Pubkey, pub ownership_revision: u64,
    pub principal: PrincipalAmounts, pub interest_terms: InterestTerms, pub interest: InterestState,
    pub acquisition_basis: u64, pub provenance_hash: [u8; 32],
    pub fee_authority_released: u64,
    /// Original delivery, gross collection and principal repayments remain historical.
    pub corrections: CorrectionAmounts,
    /// Seller fees collected on this run and credited back: credits are bounded by collections.
    pub seller_fee_collected: u64, pub seller_fee_credited: u64,
}
impl FundedRun {
    pub fn check(&self, key: Pubkey, claim: Pubkey, _c: &FundedClaim) -> Result<()> {
        let (p,bump)=funded_run_address(&claim,&self.run_id);
        require!(self.version == FUNDED_RUN_VERSION && key == p && self.bump == bump && self.claim == claim,
            CoreError::FundingRun);
        math(self.principal.validate())?; math(self.interest.validate(&self.interest_terms))?;
        require!(self.principal.originated == self.gross_earned && self.gross_paid <= self.gross_earned,
            CoreError::FundingConservation);
        math(self.corrections.validate(self.gross_earned, self.gross_paid, &self.principal))?;
        require!(self.seller_fee_credited <= self.seller_fee_collected, CoreError::FundingConservation);
        Ok(())
    }
    /// Nothing more is owed on this run, to its creditor or its supplier.
    pub fn settled(&self) -> Result<bool> {
        Ok(self.principal.outstanding == 0 && math(self.interest.outstanding())? == 0
            && self.gross_earned == plus(self.gross_paid, self.corrections.unpaid_reduced)?)
    }
}

/// What one payer route paid one run's creditor: per (run, payer source,
/// creditor). Refunds and interest returns draw on it, so a refund follows the
/// original payer. Its window numbers the route's ordinary cash repayments,
/// allocated by the payer. Its address chains to the Tab's order number
/// (record -> run -> Tab -> mandate order), so it can never recur.
#[account]
#[derive(InitSpace)]
pub struct RepaymentRecord {
    pub deposit_payer: Pubkey,
    pub version: u16, pub bump: u8,
    /// The Tab of its run: the record closes as a step of this Tab, after its run.
    pub claim: Pubkey,
    pub run: Pubkey,
    pub payer_principal: Pubkey, pub payer_authority: Pubkey, pub payer_source: Pubkey,
    pub creditor: Pubkey,
    pub principal_paid: u64,
    /// All interest paid through this route, protocol share included.
    pub interest_paid: u64,
    pub protocol_interest_paid: u64,
    pub usage: RepaymentRefundUsage,
    pub window: OrderWindow,
}
impl RepaymentRecord {
    pub fn check(&self, key: Pubkey) -> Result<()> {
        let (p,bump)=repayment_record_address(&self.run,&self.payer_source,&self.creditor);
        require!(self.version == REPAYMENT_RECORD_VERSION && p == key && bump == self.bump, CoreError::FundingRun);
        math(self.usage.validate(self.principal_paid, self.interest_paid, self.protocol_interest_paid))
    }
    pub fn record(&mut self, principal: u64, interest: u64, protocol: u64) -> Result<()> {
        self.principal_paid = plus(self.principal_paid, principal)?;
        self.interest_paid = plus(self.interest_paid, interest)?;
        self.protocol_interest_paid = plus(self.protocol_interest_paid, protocol)?;
        Ok(())
    }
}

pub fn math<T>(v: FinanceResult<T>) -> Result<T> { v.map_err(|_| error!(CoreError::FundingTerms)) }
pub fn valid_principal(p: &Account<Principal>) -> Result<()> {
    let (key,bump)=Pubkey::find_program_address(&[b"principal",p.seed_owner.as_ref(),&p.principal_id],&REGISTRY_ID);
    require!(p.version == contracts::SCHEMA_VERSION && p.key() == key && p.bump == bump, CoreError::FundingPrincipal);
    Ok(())
}
pub fn owner(p: &Account<Principal>, signer: Pubkey, generation: u64) -> Result<()> {
    valid_principal(p)?;
    require!(p.owner == signer && p.authority_generation == generation, CoreError::FundingOwner); Ok(())
}
/// Fresh owner approval remains distinct from the fee/rent payer and from an
/// actual SPL token-source signer. Native owners sign the transaction; passkey
/// owners supply Registry's exact preceding top-level authorization proof.
pub fn owner_authority(p: &Account<Principal>, authority: &AccountInfo, generation: u64,
    remaining: &[AccountInfo]) -> Result<()> {
    owner(p, authority.key(), generation)?;
    principal_registry::require_owner_authority(p, authority, remaining)
}

/// Passkey owner proofs need the canonical Instructions sysvar. It may be
/// appended once, read-only, after the complete financial book/projection set.
/// Remove only that exact suffix before the journal's strict set validation;
/// never filter arbitrary authority accounts or conceal missing books.
pub fn financial_remaining_accounts<'a, 'info>(remaining: &'a [AccountInfo<'info>])
    -> Result<&'a [AccountInfo<'info>]> {
    let instructions = anchor_lang::compat::solana_instructions_sysvar::ID;
    let end = if let Some(last) = remaining.last() {
        if last.key() == instructions {
            require!(!last.is_writable && !last.is_signer && !last.executable,
                CoreError::FundingBookAccounts);
            remaining.len() - 1
        } else { remaining.len() }
    } else { 0 };
    require!(!remaining[..end].iter().any(|account| account.key() == instructions),
        CoreError::FundingBookAccounts);
    Ok(&remaining[..end])
}
pub fn custody_token(c: &Account<FinancierCustody>, token: &Account<TokenAccount>, mint: Pubkey) -> Result<()> {
    c.check_key(c.key())?;
    require!(c.mint == mint && c.token_account == token.key() && token.mint == mint && token.owner == c.wallet,
        CoreError::FundingTokenRouting);
    require_keys_eq!(token.key(), associated_token::get_associated_token_address(&c.wallet,&mint), CoreError::FundingTokenRouting);
    c.check(token.amount)
}
pub fn admit_cash(token: &Account<TokenAccount>) -> Result<()> {
    require!(token.state == token::spl_token::state::AccountState::Initialized
        && token.delegate.is_none() && token.close_authority.is_none(), CoreError::FundingCashControl); Ok(())
}
pub fn reservation_matches(r: &Account<Reservation>, c: &FundedClaim) -> Result<()> {
    require!(r.holds_terms(&c.authority_terms()?)?, CoreError::FundingReservation);
    r.check()
}
pub fn record_base(principal:Pubkey,id:[u8;32],digest:[u8;32],kind:u8,mint:Pubkey,
    claim:Pubkey,run:Pubkey,amount:u64)->FinanceDetail {
    FinanceDetail{principal,operation_id:id,request_digest:digest,
        kind,mint,claim,run,amount,..FinanceDetail::default()}
}

pub fn transfer_checked_exact<'info>(from: &AccountInfo<'info>, destination:&AccountInfo<'info>,
    authority:&AccountInfo<'info>, mint:&Account<'info,Mint>, token_program:Pubkey,
    amount:u64, seeds:&[&[&[u8]]])->Result<()> {
    if amount==0 { return Ok(()); }
    require_keys_neq!(from.key(),destination.key(),CoreError::FundingTokenRouting);
    let read=|info:&AccountInfo|->Result<TokenAccount>{
        require_keys_eq!(*info.owner,token::ID,CoreError::FundingTokenRouting);
        TokenAccount::try_deserialize(&mut &info.try_borrow_data()?[..])
    };
    let before=read(from)?; let dest_before=read(destination)?;
    require!(before.mint == mint.key() && dest_before.mint == mint.key(),CoreError::FundingTokenRouting);
    token::transfer_checked(CpiContext::new_with_signer(token_program,token::TransferChecked{
        from:from.clone(),mint:mint.to_account_info(),to:destination.clone(),authority:authority.clone(),
    },seeds),amount,mint.decimals)?;
    let after=read(from)?;let dest_after=read(destination)?;
    require!(after.amount == minus(before.amount,amount)? && dest_after.amount == plus(dest_before.amount,amount)?,CoreError::FundingTokenDelta);
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use dexter_contracts::commercial::{COMMERCIAL_VERSION, FeeFailurePolicy, RepaymentPriority};
    use dexter_contracts::financing::Asset;

    #[test]
    fn passkey_sysvar_suffix_cannot_hide_financial_accounts() {
        let book_key=Pubkey::new_unique();
        let instructions_key=anchor_lang::compat::solana_instructions_sysvar::ID;
        let unrelated_key=Pubkey::new_unique();
        let owner=Pubkey::new_unique();
        let mut book_lamports=0;let mut book_data=[];
        let mut instructions_lamports=0;let mut instructions_data=[];
        let mut unrelated_lamports=0;let mut unrelated_data=[];
        let book=AccountInfo::new(&book_key,false,true,&mut book_lamports,&mut book_data,&owner,false);
        let instructions=AccountInfo::new(&instructions_key,false,false,
            &mut instructions_lamports,&mut instructions_data,&owner,false);
        let unrelated=AccountInfo::new(&unrelated_key,false,false,
            &mut unrelated_lamports,&mut unrelated_data,&owner,false);

        let native=[book.clone()];
        assert_eq!(financial_remaining_accounts(&native).unwrap().len(),1);
        let passkey=[book.clone(),instructions.clone()];
        let retained=financial_remaining_accounts(&passkey).unwrap();
        assert_eq!(retained.len(),1);assert_eq!(retained[0].key(),book_key);
        // An arbitrary tail is retained for the journal to authenticate; it is
        // never discarded as if it were an authorization proof account.
        let arbitrary=[book.clone(),unrelated];
        assert_eq!(financial_remaining_accounts(&arbitrary).unwrap().len(),2);
        assert!(financial_remaining_accounts(&[instructions.clone(),book.clone()]).is_err());
        assert!(financial_remaining_accounts(&[instructions.clone(),instructions.clone()]).is_err());
        let mut writable=instructions.clone();writable.is_writable=true;
        assert!(financial_remaining_accounts(&[book.clone(),writable]).is_err());
        let mut signer=instructions.clone();signer.is_signer=true;
        assert!(financial_remaining_accounts(&[book.clone(),signer]).is_err());
        let mut executable=instructions;executable.executable=true;
        assert!(financial_remaining_accounts(&[book,executable]).is_err());
    }

    pub(crate) fn accepted_run(supplier_principal: Pubkey, payment_wallet: Pubkey, order: u64)
        -> (Pubkey, FundedClaim, Pubkey, FundedRun) {
        let company=Pubkey::new_from_array([20;32]);
        let mint=Pubkey::new_from_array([21;32]);
        let issuer=Pubkey::new_from_array([22;32]);
        let issuer_custody=Pubkey::new_from_array([23;32]);
        let mandate=Pubkey::new_from_array([26;32]);
        let namespace=[24;32];let run_id=[25;32];
        let (claim_key,claim_bump)=tab_address(&mandate,order);
        let interest_terms=InterestTerms{version:COMMERCIAL_VERSION,annual_rate_bps:0,seconds_per_year:1,
            maximum_interest:0,accrual_cutoff:i64::MAX,protocol_share_bps:0,protocol_beneficiary:Pubkey::default(),
            protocol_destination:Pubkey::default(),repayment_priority:RepaymentPriority::InterestFirst,stop_on_shortfall:false};
        let terms=TabTerms{domain:[1;32],order,principal:company,actor:Pubkey::new_unique(),mandate,
            asset:mint,supplier:payment_wallet,source:Pubkey::new_unique(),cap:10,expires_at:100,
            credential_generation:1,mandate_revision:1,external_terms_hash:[2;32],
            hold_window_seconds:0,arbiter:Pubkey::default()};
        let seller_fee_terms=SellerFeeTerms{version:COMMERCIAL_VERSION,agreement_hash:[3;32],supplier:payment_wallet,
            asset:Asset{mint,token_program:token::ID},gross_cap:10,rate_bps:0,fixed_fee:0,aggregate_fee_cap:0,
            maximum_deduction_bps:0,beneficiary:Pubkey::default(),beneficiary_wallet:Pubkey::default(),
            destination:Pubkey::default(),failure_policy:FeeFailurePolicy::WaiveUnavailable};
        let claim=FundedClaim{deposit_payer:Pubkey::new_unique(),version:FUNDED_CLAIM_VERSION,bump:claim_bump,terms,
            usage_namespace:namespace,
            service_unit:[4;32],unit_price:1,external_contract_hash:[5;32],acceptance_hash:[6;32],
            credit_line:Pubkey::new_unique(),assignment_policy_hash:[0;32],facility:Pubkey::new_unique(),
            custody:Pubkey::new_unique(),issuer,issuer_custody,supplier_principal,remaining:0,payable:10,debt:10,
            paid:0,repaid:0,released:0,fee_cap:0,fee_authority_remaining:0,allocated_run_fee_caps:0,
            released_fee_caps:0,released_unallocated_fee_caps:0,interest_outstanding:0,interest_terms:interest_terms.clone(),
            seller_fee_agreement:Pubkey::default(),seller_fee_allocation:0,seller_fee_terms,seller_fee_state:SellerFeeState::default(),
            principal_extinguished:0,corrections:CorrectionAmounts::default(),steps:2,money_steps:2,
            fingerprint:[9;32],open_runs:1,open_records:0,
            hold:SettlementHold::default(),buyer_hold_used:false,seller_hold_used:false};
        let (run_key,bump)=funded_run_address(&claim_key,&run_id);
        let run=FundedRun{deposit_payer:Pubkey::new_unique(),version:FUNDED_RUN_VERSION,bump,claim:claim_key,run_id,
            capture_digest:[8;32],gross_earned:10,gross_paid:0,current_creditor:issuer,creditor_custody:issuer_custody,
            ownership_revision:1,principal:PrincipalAmounts{originated:10,outstanding:10,repaid:0,extinguished:0},
            interest:InterestState::new(1,&interest_terms).unwrap(),interest_terms,acquisition_basis:10,
            provenance_hash:[7;32],fee_authority_released:0,corrections:CorrectionAmounts::default(),
            seller_fee_collected:0,seller_fee_credited:0};
        claim.check(claim_key).unwrap();run.check(run_key,claim_key,&claim).unwrap();
        (claim_key,claim,run_key,run)
    }

    #[test]
    fn a_run_belongs_to_one_tab_and_its_address_follows_the_tab() {
        let supplier=Pubkey::new_unique();let first_owner=Pubkey::new_unique();let next_owner=Pubkey::new_unique();
        let (first_claim_key,_first_claim,first_key,first_run)=accepted_run(supplier,first_owner,31);
        let (next_claim_key,next_claim,next_key,_next_run)=accepted_run(supplier,next_owner,32);
        // Change 5: one run id bills once per Tab. Two Tabs give two run addresses.
        assert_ne!(first_key,next_key);
        assert_ne!(first_claim_key,next_claim_key);
        // A run cannot be presented as a run of another Tab.
        assert!(first_run.check(first_key,next_claim_key,&next_claim).is_err());
        assert!(first_run.check(next_key,next_claim_key,&next_claim).is_err());
    }

    #[test]
    fn tab_book_guard_and_remaining() {
        let (claim_key,mut claim,_,_)=accepted_run(Pubkey::new_unique(),Pubkey::new_unique(),40);
        claim.remaining=5;
        claim.lower_remaining(2).unwrap();assert_eq!(claim.remaining,3);
        assert!(claim.lower_remaining(4).is_err());assert_eq!(claim.remaining,3);
        claim.guard(2).unwrap();assert!(claim.guard(1).is_err());assert!(claim.guard(3).is_err());
        for i in 0..10u8 { claim.record_step([i;32],true).unwrap(); }
        assert_eq!((claim.steps,claim.money_steps),(12,12));
        // A step prepared before any of those ten is stale.
        assert!(claim.guard(2).is_err());claim.guard(12).unwrap();
        // Closes, admissions and holds advance the steps, not the money steps.
        for _ in 0..20 { claim.record_step([0;32],false).unwrap(); }
        assert_eq!((claim.steps,claim.money_steps),(32,12));
        claim.guard(12).unwrap();
        let _ = claim_key;
    }

    #[test]
    fn numbered_moves_are_admitted_once_ever() {
        let mut window=OrderWindow::default();
        assert!(admit_number(&mut window,0).is_err());
        admit_number(&mut window,1).unwrap();
        assert!(admit_number(&mut window,1).is_err());
        admit_number(&mut window,3).unwrap();admit_number(&mut window,2).unwrap();
        for n in [1,2,3] { assert!(admit_number(&mut window,n).is_err()); }
        // Far ahead is refused; 1,024 below the highest can never be admitted.
        assert!(admit_number(&mut window,3+65_537).is_err());
        admit_number(&mut window,2_000).unwrap();
        assert!(admit_number(&mut window,4).is_err());
        let run=Pubkey::new_unique();let source=Pubkey::new_unique();let creditor=Pubkey::new_unique();
        let (key,bump)=repayment_record_address(&run,&source,&creditor);
        let mut record=RepaymentRecord{deposit_payer:Pubkey::new_unique(),version:REPAYMENT_RECORD_VERSION,bump,
            claim:Pubkey::new_unique(),run,
            payer_principal:Pubkey::new_unique(),payer_authority:Pubkey::new_unique(),payer_source:source,creditor,
            principal_paid:0,interest_paid:0,protocol_interest_paid:0,usage:RepaymentRefundUsage::default(),
            window:OrderWindow::default()};
        record.check(key).unwrap();
        for n in 1..=5u64 { admit_number(&mut record.window,n).unwrap(); record.record(10,2,1).unwrap(); }
        assert_eq!((record.principal_paid,record.interest_paid,record.protocol_interest_paid),(50,10,5));
        for n in 1..=5u64 { assert!(admit_number(&mut record.window,n).is_err()); }
        record.check(key).unwrap();
        assert!(record.check(Pubkey::new_unique()).is_err());
    }
}
