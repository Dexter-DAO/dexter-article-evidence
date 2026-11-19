//! Settlement hold (design section 9, limits in section 15, decided F2).
//!
//! A Tab's terms carry `hold_window_seconds` and an optional `arbiter`, signed
//! by the buyer's credential and consented to by the issuer and the seller like
//! every other term. With a window of zero the Tab is final at settlement and no
//! hold can be set. Otherwise the buyer and the seller may each set one hold
//! before the Tab closes. A hold lasts `hold_window_seconds` from when it is set
//! and is never extended; only one is in force at a time. While a hold is in
//! force every close of the Tab (repayment records, runs, the Tab) is a no-op;
//! corrections, interest refunds and seller-fee credits stay possible, because
//! the records they read still exist. A hold ends when either party or the
//! arbiter releases it, or when its time passes. Setting and releasing are Tab
//! steps with events and no book changes. Owners act directly or by passkey.
//!
//! The Tab keeps only what decisions read, at fixed width: which party set the
//! hold in force and until when, and which parties have used theirs (11 bytes).
//! The setter's principal, the reason and the resolution are in the HoldSet and
//! HoldReleased events, which the Tab's fingerprint commits to. (The full record,
//! 75 bytes, pushed the commit's account-validation frame 64 bytes past SBF's
//! 4,096-byte limit.)
use anchor_lang::prelude::*;
use principal_registry::Principal;
use solana_sha256_hasher::hashv;
use crate::CoreError;
use crate::funding_state::{valid_principal, FundedClaim};
use crate::tab_events::{self, EventBytes, TabStepHeader};

/// The party that set a hold; `Unset` when no hold has been set or the last was released.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq, InitSpace)]
pub enum HoldParty { #[default] Unset, Buyer, Seller }

/// A Tab's settlement hold. In force while set and the clock is before `until`.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq, InitSpace)]
pub struct SettlementHold {
    pub set_by: HoldParty,
    pub until: i64,
}

/// A principal's role on a Tab's hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldRole { Buyer, Seller, Arbiter }

impl FundedClaim {
    /// The buyer, the seller, then the arbiter: the first role `principal` has.
    pub fn hold_role(&self, principal: &Pubkey) -> Option<HoldRole> {
        if *principal == self.terms.principal { Some(HoldRole::Buyer) }
        else if *principal == self.supplier_principal { Some(HoldRole::Seller) }
        else if self.terms.arbiter != Pubkey::default() && *principal == self.terms.arbiter { Some(HoldRole::Arbiter) }
        else { None }
    }

    pub fn hold_in_force(&self, now: i64) -> bool {
        self.hold.set_by != HoldParty::Unset && now < self.hold.until
    }

    /// One hold per party per Tab; none while another is in force; no extension.
    pub fn set_hold(&mut self, principal: Pubkey, now: i64) -> Result<SettlementHold> {
        require!(self.terms.hold_window_seconds > 0, CoreError::HoldNotAgreed);
        let set_by = match self.hold_role(&principal) {
            Some(HoldRole::Buyer) => HoldParty::Buyer,
            Some(HoldRole::Seller) => HoldParty::Seller,
            _ => return err!(CoreError::HoldAuthority),
        };
        require!(!self.hold_in_force(now), CoreError::HoldInForce);
        let until = now.checked_add(i64::from(self.terms.hold_window_seconds))
            .ok_or_else(|| error!(CoreError::FundingSchema))?;
        let used = if set_by == HoldParty::Buyer { &mut self.buyer_hold_used } else { &mut self.seller_hold_used };
        require!(!*used, CoreError::HoldUsed);
        *used = true;
        self.hold = SettlementHold { set_by, until };
        Ok(self.hold)
    }

    /// Either party (the setter or the other one) or the arbiter ends a hold in force.
    pub fn release_hold(&mut self, principal: Pubkey, now: i64) -> Result<SettlementHold> {
        require!(self.hold_in_force(now), CoreError::HoldNotInForce);
        require!(self.hold_role(&principal).is_some(), CoreError::HoldAuthority);
        let hold = self.hold;
        self.hold = SettlementHold::default();
        Ok(hold)
    }

    /// The hold fields agree with the terms: no hold without a window, and the
    /// hold last set belongs to a party that has used its one hold.
    pub fn check_hold(&self) -> Result<()> {
        if self.terms.hold_window_seconds == 0 {
            require!(self.hold == SettlementHold::default() && !self.buyer_hold_used && !self.seller_hold_used,
                CoreError::FundingSchema);
        }
        let used = match self.hold.set_by {
            HoldParty::Unset => self.hold.until == 0,
            HoldParty::Buyer => self.buyer_hold_used,
            HoldParty::Seller => self.seller_hold_used,
        };
        require!(used, CoreError::FundingSchema);
        Ok(())
    }
}

#[event]
pub struct HoldSet {
    pub header: TabStepHeader,
    pub set_by: HoldParty,
    /// The setting party's principal, and the owner authority that acted for it.
    pub principal: Pubkey,
    pub authority: Pubkey,
    pub until: i64,
    pub reason_hash: [u8; 32],
}

#[event]
pub struct HoldReleased {
    pub header: TabStepHeader,
    /// The hold this ends.
    pub set_by: HoldParty,
    pub until: i64,
    /// The principal that released it (either party or the arbiter), and its owner authority.
    pub released_by: Pubkey,
    pub authority: Pubkey,
    pub resolution_hash: [u8; 32],
}

/// The step's operation id: one hold per party per Tab, so (tab, party) names it.
pub fn hold_operation_id(kind: &[u8], tab: &Pubkey, set_by: HoldParty) -> [u8; 32] {
    hashv(&[b"DEXTER:SETTLEMENT-HOLD:1", kind, tab.as_ref(), &[set_by as u8]]).to_bytes()
}

#[derive(Accounts)]
pub struct SettlementHoldStep<'info> {
    /// CHECK: The acting principal's owner: a transaction signer, or a passkey
    /// alias authorized by the Registry instruction before this one.
    pub owner: UncheckedAccount<'info>,
    /// The buyer, the seller or (release only) the arbiter named in the terms.
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut)] pub claim: Box<Account<'info, FundedClaim>>,
    /// CHECK: ClaimsCore's event authority.
    #[account(address = crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority: UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address = crate::ID)] pub core_program: UncheckedAccount<'info>,
}

/// The acting principal and its current owner's authority (passkey allowed).
fn acting<'info>(a: &SettlementHoldStep<'info>, remaining: &[AccountInfo<'info>]) -> Result<Pubkey> {
    a.claim.check(a.claim.key())?;
    valid_principal(&a.principal)?;
    principal_registry::require_owner_authority(&a.principal, &a.owner, remaining)?;
    Ok(a.principal.key())
}

pub fn handle_set_settlement_hold<'info>(ctx: Context<'info, SettlementHoldStep<'info>>, reason_hash: [u8; 32]) -> Result<()> {
    let principal = acting(&ctx.accounts, ctx.remaining_accounts)?;
    let a = ctx.accounts;
    let tab = a.claim.key();
    let hold = a.claim.set_hold(principal, Clock::get()?.unix_timestamp)?;
    let id = tab_events::step_id(&hold_operation_id(b"set", &tab, hold.set_by), &reason_hash);
    let header = TabStepHeader { tab, step: a.claim.next_step()?, step_id: id };
    let event = EventBytes::new::<HoldSet>(TabStepHeader::INIT_SPACE + 1 + 32 + 32 + 8 + 32)
        .put(&header)?.put(&hold.set_by)?.put(&principal)?.put(&a.owner.key())?.put(&hold.until)?.put(&reason_hash)?;
    tab_events::record_tab_step(&mut a.claim, &a.core_event_authority, event, false)
}

pub fn handle_release_settlement_hold<'info>(ctx: Context<'info, SettlementHoldStep<'info>>,
    resolution_hash: [u8; 32]) -> Result<()> {
    let principal = acting(&ctx.accounts, ctx.remaining_accounts)?;
    let a = ctx.accounts;
    let tab = a.claim.key();
    let hold = a.claim.release_hold(principal, Clock::get()?.unix_timestamp)?;
    let id = tab_events::step_id(&hold_operation_id(b"release", &tab, hold.set_by), &resolution_hash);
    let header = TabStepHeader { tab, step: a.claim.next_step()?, step_id: id };
    let event = EventBytes::new::<HoldReleased>(TabStepHeader::INIT_SPACE + 1 + 8 + 32 + 32 + 32)
        .put(&header)?.put(&hold.set_by)?.put(&hold.until)?
        .put(&principal)?.put(&a.owner.key())?.put(&resolution_hash)?;
    tab_events::record_tab_step(&mut a.claim, &a.core_event_authority, event, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::Event;

    fn tab(window: u32, arbiter: Pubkey) -> (FundedClaim, Pubkey, Pubkey) {
        let seller = Pubkey::new_unique();
        let (_, mut claim, _, _) = crate::funding_state::tests::accepted_run(seller, Pubkey::new_unique(), 7);
        claim.terms.hold_window_seconds = window;
        claim.terms.arbiter = arbiter;
        let buyer = claim.terms.principal;
        assert_ne!(buyer, seller);
        claim.check_hold().unwrap();
        (claim, buyer, seller)
    }

    /// The refusal's code; panics on success.
    fn code(result: Result<SettlementHold>) -> u32 {
        match result {
            Err(Error::AnchorError(e)) => e.error_code_number,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
    fn of(error: CoreError) -> u32 { u32::from(error) }

    #[test]
    fn a_zero_window_is_final_at_settlement() {
        let (mut claim, buyer, seller) = tab(0, Pubkey::default());
        assert_eq!(code(claim.set_hold(buyer, 10)), of(CoreError::HoldNotAgreed));
        assert_eq!(code(claim.set_hold(seller, 10)), of(CoreError::HoldNotAgreed));
        assert!(!claim.hold_in_force(10) && claim.hold == SettlementHold::default());
        claim.check_hold().unwrap();
    }

    #[test]
    fn each_party_sets_one_hold_that_lasts_the_window_and_is_never_extended() {
        let (mut claim, buyer, seller) = tab(600, Pubkey::default());
        let hold = claim.set_hold(buyer, 100).unwrap();
        assert_eq!(hold, SettlementHold { set_by: HoldParty::Buyer, until: 700 });
        assert!(claim.hold_in_force(100) && claim.hold_in_force(699) && !claim.hold_in_force(700));
        // No second hold, and no extension, while one is in force.
        assert_eq!(code(claim.set_hold(buyer, 200)), of(CoreError::HoldInForce));
        assert_eq!(code(claim.set_hold(seller, 200)), of(CoreError::HoldInForce));
        assert_eq!(claim.hold.until, 700);
        // The seller releases the buyer's hold; the buyer cannot set another.
        assert_eq!(claim.release_hold(seller, 300).unwrap(), hold);
        assert!(!claim.hold_in_force(300) && claim.hold == SettlementHold::default());
        assert_eq!(code(claim.set_hold(buyer, 300)), of(CoreError::HoldUsed));
        // The seller still has its own one hold.
        assert_eq!(claim.set_hold(seller, 300).unwrap(), SettlementHold { set_by: HoldParty::Seller, until: 900 });
        claim.release_hold(seller, 301).unwrap();
        assert_eq!(code(claim.set_hold(seller, 1_000)), of(CoreError::HoldUsed));
        assert!(claim.buyer_hold_used && claim.seller_hold_used);
        claim.check_hold().unwrap();
    }

    #[test]
    fn the_setter_the_other_party_or_the_arbiter_releases_and_no_one_else() {
        let arbiter = Pubkey::new_unique();
        let stranger = Pubkey::new_unique();
        for releaser in 0..3 {
            let (mut claim, buyer, seller) = tab(600, arbiter);
            claim.set_hold(buyer, 100).unwrap();
            assert_eq!(code(claim.release_hold(stranger, 101)), of(CoreError::HoldAuthority));
            assert_eq!(code(claim.set_hold(stranger, 101)), of(CoreError::HoldAuthority));
            // The arbiter can release but never set.
            assert_eq!(code(claim.set_hold(arbiter, 101)), of(CoreError::HoldAuthority));
            let by = [buyer, seller, arbiter][releaser];
            claim.release_hold(by, 102).unwrap();
            assert_eq!(code(claim.release_hold(by, 103)), of(CoreError::HoldNotInForce));
        }
        // Without an arbiter in the terms, that principal is a stranger.
        let (mut claim, buyer, _) = tab(600, Pubkey::default());
        claim.set_hold(buyer, 100).unwrap();
        assert_eq!(code(claim.release_hold(arbiter, 101)), of(CoreError::HoldAuthority));
    }

    #[test]
    fn a_hold_ends_when_its_time_passes_without_a_call() {
        let (mut claim, buyer, seller) = tab(60, Pubkey::default());
        claim.set_hold(buyer, 1_000).unwrap();
        assert!(claim.hold_in_force(1_059) && !claim.hold_in_force(1_060));
        // Nothing to release once it has ended; the other party may set its own.
        assert_eq!(code(claim.release_hold(seller, 1_060)), of(CoreError::HoldNotInForce));
        let next = claim.set_hold(seller, 1_060).unwrap();
        assert_eq!(next.until, 1_120);
        claim.check_hold().unwrap();
    }

    #[test]
    fn the_largest_window_cannot_overflow_the_clock() {
        let (mut claim, buyer, _) = tab(u32::MAX, Pubkey::default());
        assert_eq!(code(claim.set_hold(buyer, i64::MAX - i64::from(u32::MAX) + 1)), of(CoreError::FundingSchema));
        assert!(claim.hold == SettlementHold::default() && !claim.buyer_hold_used);
        let hold = claim.set_hold(buyer, 0).unwrap();
        assert_eq!(hold.until, i64::from(u32::MAX));
    }

    #[test]
    fn inconsistent_hold_fields_fail_the_tab_check() {
        let (mut claim, _, _) = tab(0, Pubkey::default());
        claim.hold = SettlementHold { set_by: HoldParty::Buyer, until: 5 };
        assert!(claim.check_hold().is_err());
        let (mut claim, _, _) = tab(0, Pubkey::default());
        claim.seller_hold_used = true;
        assert!(claim.check_hold().is_err());
        let (mut claim, _, _) = tab(60, Pubkey::default());
        claim.hold = SettlementHold { set_by: HoldParty::Buyer, until: 5 };
        assert!(claim.check_hold().is_err());
        claim.buyer_hold_used = true;
        claim.check_hold().unwrap();
        claim.hold = SettlementHold { set_by: HoldParty::Seller, until: 5 };
        assert!(claim.check_hold().is_err());
        // An unset hold carries no time.
        claim.hold = SettlementHold { set_by: HoldParty::Unset, until: 5 };
        assert!(claim.check_hold().is_err());
    }

    #[test]
    fn hold_events_are_written_as_their_structs() {
        let header = TabStepHeader { tab: Pubkey::new_unique(), step: 4, step_id: [3; 32] };
        let (principal, authority, releaser) = (Pubkey::new_unique(), Pubkey::new_unique(), Pubkey::new_unique());
        let built = EventBytes::new::<HoldSet>(TabStepHeader::INIT_SPACE + 105)
            .put(&header).unwrap().put(&HoldParty::Seller).unwrap().put(&principal).unwrap().put(&authority).unwrap()
            .put(&77i64).unwrap().put(&[5u8; 32]).unwrap().into_inner();
        let mut expected = anchor_lang::event::EVENT_IX_TAG_LE.to_vec();
        expected.extend_from_slice(&HoldSet { header, set_by: HoldParty::Seller, principal, authority, until: 77,
            reason_hash: [5; 32] }.data());
        assert_eq!(built, expected);
        let built = EventBytes::new::<HoldReleased>(TabStepHeader::INIT_SPACE + 105)
            .put(&header).unwrap().put(&HoldParty::Seller).unwrap().put(&77i64).unwrap()
            .put(&releaser).unwrap().put(&authority).unwrap().put(&[6u8; 32]).unwrap().into_inner();
        let mut expected = anchor_lang::event::EVENT_IX_TAG_LE.to_vec();
        expected.extend_from_slice(&HoldReleased { header, set_by: HoldParty::Seller, until: 77,
            released_by: releaser, authority, resolution_hash: [6; 32] }.data());
        assert_eq!(built, expected);
        // Two parties, two operation ids per Tab: the step ids never collide.
        let tab = Pubkey::new_unique();
        assert_ne!(hold_operation_id(b"set", &tab, HoldParty::Buyer), hold_operation_id(b"set", &tab, HoldParty::Seller));
        assert_ne!(hold_operation_id(b"set", &tab, HoldParty::Buyer), hold_operation_id(b"release", &tab, HoldParty::Buyer));
    }
}
