//! Supplier-approved lifetime fee capacity shared by separately accepted Tabs.
//! Allocation authorizes a child's maximum realizable fee; actual fee earning
//! and collection remain governed by that Tab's immutable gross-to-net terms.
//! Collection, credits, waivers and unused release never replenish this parent.
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint};
use dexter_contracts::{CORE_ID, commercial::{SellerFeeCapUsage, SellerFeeTerms},
    financing::Asset, funded::FUNDED_VERSION};
use principal_registry::Principal;
use crate::{CoreError, funding_state::{math, owner}};

#[account]
#[derive(InitSpace)]
pub struct SellerFeeAgreement {
    pub version: u16,
    pub bump: u8,
    pub domain: [u8; 32],
    pub supplier_principal: Pubkey,
    pub asset: Asset,
    pub agreement_hash: [u8; 32],
    /// Immutable lifetime ceiling, in this asset's base units.
    pub aggregate_cap: u64,
    pub usage: SellerFeeCapUsage,
    /// Opening provenance, not the authority required for later reconciliation.
    pub approved_by: Pubkey,
    pub authority_generation: u64,
}

pub fn seller_fee_agreement_address(supplier: &Pubkey, mint: &Pubkey, agreement_hash: &[u8; 32]) -> Pubkey {
    Pubkey::find_program_address(&[b"seller-fee-agreement", supplier.as_ref(), mint.as_ref(), agreement_hash], &CORE_ID).0
}

impl SellerFeeAgreement {
    pub fn check_key(&self, key: Pubkey) -> Result<()> {
        let (expected, bump) = Pubkey::find_program_address(&[b"seller-fee-agreement",
            self.supplier_principal.as_ref(), self.asset.mint.as_ref(), &self.agreement_hash], &CORE_ID);
        require!(self.version == FUNDED_VERSION && key == expected && self.bump == bump
            && self.supplier_principal != Pubkey::default() && self.asset.mint != Pubkey::default()
            && self.asset.token_program == token::ID && self.agreement_hash != [0; 32]
            && self.approved_by != Pubkey::default() && self.approved_by.is_on_curve() && self.authority_generation > 0,
            CoreError::FundingSellerFeeAgreement);
        require!(self.usage.allocated <= self.aggregate_cap, CoreError::FundingSellerFeeAggregateCap);
        Ok(())
    }

    pub fn check_identity(&self, key: Pubkey, domain: &[u8; 32], supplier: Pubkey,
        asset: &Asset, agreement_hash: &[u8; 32]) -> Result<()> {
        self.check_key(key)?;
        require!(self.domain == *domain && self.supplier_principal == supplier
            && self.asset == *asset && self.agreement_hash == *agreement_hash,
            CoreError::FundingSellerFeeAgreement);
        Ok(())
    }

    pub fn reconcile(&self, key: Pubkey, domain: &[u8; 32], supplier: Pubkey,
        asset: &Asset, agreement_hash: &[u8; 32], aggregate_cap: u64) -> Result<()> {
        self.check_identity(key, domain, supplier, asset, agreement_hash)?;
        require!(self.aggregate_cap == aggregate_cap, CoreError::FundingSellerFeeAgreement);
        Ok(())
    }

    /// Called once on fresh Tab acceptance, in the same transaction as its
    /// Registry reservation, custody commitment and complete financial journal.
    pub fn allocate(&mut self, amount: u64) -> Result<()> {
        require!(amount > 0, CoreError::FundingSellerFeeAggregateCap);
        let next = self.usage.allocate(amount, self.aggregate_cap)
            .map_err(|_| error!(CoreError::FundingSellerFeeAggregateCap))?;
        self.usage = next;
        Ok(())
    }
}

/// Strict reader for inspection and cross-program consumers. Anchor's typed
/// accounts provide the corresponding owner/type check in the handlers.
pub fn read_seller_fee_agreement(info: &AccountInfo) -> Result<SellerFeeAgreement> {
    require!(*info.owner == CORE_ID && !info.executable
        && info.data_len() == 8 + SellerFeeAgreement::INIT_SPACE, CoreError::FundingSellerFeeAgreement);
    let data = info.try_borrow_data()?;
    let agreement = SellerFeeAgreement::try_deserialize(&mut &data[..])
        .map_err(|_| error!(CoreError::FundingSellerFeeAgreement))?;
    agreement.check_key(info.key())?;
    Ok(agreement)
}

/// The accepted child carries its immutable parent identity and allocation.
/// Servicing checks this local fact without loading or consulting the parent.
pub fn check_claim_allocation(supplier: Pubkey, terms: &SellerFeeTerms,
    parent: Pubkey, allocation: u64) -> Result<()> {
    let maximum = math(terms.cumulative_fee(terms.gross_cap))?;
    require!(allocation == maximum, CoreError::FundingSellerFeeAggregateCap);
    let expected = if maximum == 0 { Pubkey::default() } else {
        seller_fee_agreement_address(&supplier, &terms.asset.mint, &terms.agreement_hash)
    };
    require_keys_eq!(parent, expected, CoreError::FundingSellerFeeAgreement);
    Ok(())
}

#[derive(Accounts)]
#[instruction(agreement_hash: [u8; 32])]
pub struct OpenSellerFeeAgreement<'info> {
    pub owner: Signer<'info>,
    #[account(mut)] pub payer: Signer<'info>,
    pub supplier_principal: Box<Account<'info, Principal>>,
    pub mint: Box<Account<'info, Mint>>,
    #[account(init_if_needed, payer = payer, space = 8 + SellerFeeAgreement::INIT_SPACE,
        seeds = [b"seller-fee-agreement", supplier_principal.key().as_ref(), mint.key().as_ref(), &agreement_hash], bump)]
    pub agreement: Box<Account<'info, SellerFeeAgreement>>,
    pub system_program: Program<'info, System>,
}

pub fn open<'info>(ctx: Context<'info, OpenSellerFeeAgreement<'info>>, agreement_hash: [u8; 32],
    aggregate_cap: u64, authority_generation: u64) -> Result<()> {
    let a = ctx.accounts;
    owner(&a.supplier_principal, a.owner.key(), authority_generation)?;
    require!(a.owner.key().is_on_curve() && authority_generation > 0 && agreement_hash != [0; 32],
        CoreError::FundingSellerFeeAgreement);
    let asset = Asset { mint: a.mint.key(), token_program: token::ID };
    if a.agreement.version != 0 {
        // Current-owner reconciliation reports the same agreement and current
        // usage. Neither rotation nor reopening rewrites original provenance.
        return a.agreement.reconcile(a.agreement.key(), &a.supplier_principal.domain,
            a.supplier_principal.key(), &asset, &agreement_hash, aggregate_cap);
    }
    a.agreement.set_inner(SellerFeeAgreement { version: FUNDED_VERSION, bump: ctx.bumps.agreement,
        domain: a.supplier_principal.domain, supplier_principal: a.supplier_principal.key(), asset,
        agreement_hash, aggregate_cap, usage: SellerFeeCapUsage::default(), approved_by: a.owner.key(),
        authority_generation });
    a.agreement.check_key(a.agreement.key())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dexter_contracts::commercial::{COMMERCIAL_VERSION, FeeFailurePolicy, SellerFeeState};

    fn agreement() -> (Pubkey, SellerFeeAgreement) {
        let supplier = Pubkey::new_unique(); let mint = Pubkey::new_unique(); let hash = [4; 32];
        let (key, bump) = Pubkey::find_program_address(
            &[b"seller-fee-agreement", supplier.as_ref(), mint.as_ref(), &hash], &CORE_ID);
        // RFC 8032 public test key. Arbitrary Pubkey::new_unique values need not
        // lie on the curve and therefore do not model a native approver.
        let approved_by = Pubkey::new_from_array([0xd7,0x5a,0x98,0x01,0x82,0xb1,0x0a,0xb7,
            0xd5,0x4b,0xfe,0xd3,0xc9,0x64,0x07,0x3a,0x0e,0xe1,0x72,0xf3,0xda,0xa6,0x23,0x25,
            0xaf,0x02,0x1a,0x68,0xf7,0x07,0x51,0x1a]);
        (key, SellerFeeAgreement { version: FUNDED_VERSION, bump, domain: [3; 32],
            supplier_principal: supplier, asset: Asset { mint, token_program: token::ID },
            agreement_hash: hash, aggregate_cap: 2_100, usage: SellerFeeCapUsage::default(),
            approved_by, authority_generation: 1 })
    }

    fn terms(parent: &SellerFeeAgreement, gross: u64) -> SellerFeeTerms {
        SellerFeeTerms { version: COMMERCIAL_VERSION, agreement_hash: parent.agreement_hash,
            supplier: parent.approved_by, asset: parent.asset.clone(), gross_cap: gross,
            rate_bps: 137, fixed_fee: 7, aggregate_fee_cap: 5_000, maximum_deduction_bps: 300,
            beneficiary: Pubkey::new_unique(), beneficiary_wallet: Pubkey::new_unique(),
            destination: Pubkey::new_unique(), failure_policy: FeeFailurePolicy::WaiveUnavailable }
    }

    #[test]
    fn two_tabs_allocate_realizable_fees_and_excess_cannot_consume_capacity() {
        let (key, mut parent) = agreement(); parent.check_key(key).unwrap();
        let first = terms(&parent, 100_000); let second = terms(&parent, 50_000);
        let first_max = first.cumulative_fee(first.gross_cap).unwrap();
        let second_max = second.cumulative_fee(second.gross_cap).unwrap();
        assert_eq!((first_max, second_max), (1_377, 692));
        parent.allocate(first_max).unwrap(); parent.allocate(second_max).unwrap();
        assert_eq!(parent.usage.allocated, 2_069);
        assert!(parent.allocate(second_max).is_err());
        assert_eq!(parent.usage.allocated, 2_069);
        parent.check_key(key).unwrap();
    }

    #[test]
    fn reconciliation_preserves_usage_and_refuses_another_cap_or_identity() {
        let (key, mut parent) = agreement(); parent.allocate(1_377).unwrap();
        parent.reconcile(key, &parent.domain, parent.supplier_principal, &parent.asset,
            &parent.agreement_hash, 2_100).unwrap();
        assert_eq!(parent.usage.allocated, 1_377);
        assert!(parent.reconcile(key, &parent.domain, parent.supplier_principal, &parent.asset,
            &parent.agreement_hash, 2_101).is_err());
        assert!(parent.check_identity(key, &[9; 32], parent.supplier_principal, &parent.asset,
            &parent.agreement_hash).is_err());
        assert!(parent.check_identity(key, &parent.domain, Pubkey::new_unique(), &parent.asset,
            &parent.agreement_hash).is_err());
        let other_asset = Asset { mint: Pubkey::new_unique(), token_program: token::ID };
        assert!(parent.check_identity(key, &parent.domain, parent.supplier_principal, &other_asset,
            &parent.agreement_hash).is_err());
        assert!(parent.check_identity(key, &parent.domain, parent.supplier_principal, &parent.asset,
            &[8; 32]).is_err());
        assert_eq!(parent.usage.allocated, 1_377);
    }

    #[test]
    fn claim_has_an_exact_parent_and_slice_without_a_parent_servicing_dependency() {
        let (key, parent) = agreement(); let paid = terms(&parent, 100_000);
        check_claim_allocation(parent.supplier_principal, &paid, key, 1_377).unwrap();
        assert!(check_claim_allocation(parent.supplier_principal, &paid, key, 1_378).is_err());
        assert!(check_claim_allocation(parent.supplier_principal, &paid, Pubkey::default(), 1_377).is_err());
        let mut free = paid; free.aggregate_fee_cap = 0;
        check_claim_allocation(parent.supplier_principal, &free, Pubkey::default(), 0).unwrap();
        assert!(check_claim_allocation(parent.supplier_principal, &free, key, 0).is_err());
    }

    #[test]
    fn credits_refunds_and_waivers_do_not_reopen_lifetime_fee_capacity() {
        let (_, mut parent) = agreement(); let child = terms(&parent, 100_000);
        parent.aggregate_cap = 1_377; parent.allocate(1_377).unwrap();
        let (paid, _) = SellerFeeState::default().settle(100_000, &child, true).unwrap();
        let returned = paid.credit(700, &child).unwrap().refund(677, &child).unwrap();
        returned.validate(&child).unwrap();
        let (waived, settlement) = SellerFeeState::default().settle(100_000, &child, false).unwrap();
        assert_eq!((waived.waived, settlement.supplier_net), (1_377, 100_000));
        assert!(parent.allocate(1).is_err());
        assert_eq!(parent.usage.allocated, 1_377);
    }

    #[test]
    fn strict_reader_checks_owner_type_size_version_pda_and_bump() {
        let (key, mut parent) = agreement();
        let read = |value: &SellerFeeAgreement, address: Pubkey, account_owner: Pubkey,
            corrupt_type: bool, extra: bool| {
            let mut data = Vec::new(); value.try_serialize(&mut data).unwrap();
            if corrupt_type { data[0] ^= 1; } if extra { data.push(0); }
            let mut lamports = 0;
            let info = AccountInfo::new(&address, false, false, &mut lamports, &mut data, &account_owner, false);
            read_seller_fee_agreement(&info).is_ok()
        };
        assert!(read(&parent, key, CORE_ID, false, false));
        assert!(!read(&parent, key, System::id(), false, false));
        assert!(!read(&parent, key, CORE_ID, true, false));
        assert!(!read(&parent, key, CORE_ID, false, true));
        assert!(!read(&parent, Pubkey::new_unique(), CORE_ID, false, false));
        parent.bump ^= 1; assert!(!read(&parent, key, CORE_ID, false, false)); parent.bump ^= 1;
        parent.version += 1; assert!(!read(&parent, key, CORE_ID, false, false)); parent.version -= 1;
        parent.usage.allocated = 2_101; assert!(!read(&parent, key, CORE_ID, false, false));
    }
}
