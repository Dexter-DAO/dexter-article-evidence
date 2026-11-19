//! Change 5: history as events. Every Tab step, wallet trade and custody
//! operation emits one event by self-CPI (Anchor's event-cpi: the data starts
//! with `EVENT_IX_TAG`, and ClaimsCore's event authority PDA signs), so event
//! data never depends on the log buffer. A Tab step's event also extends the
//! Tab's fingerprint: `h_n = sha256(h_{n-1} || event bytes)`, where the event
//! bytes are the instruction data after the 8-byte tag.
//!
//! Event structs define the wire format and the IDL. Large events are written
//! field by field into one pre-sized buffer (`EventBytes`), never built on the
//! 4 KB SBF stack; the unit tests check each writer against its struct.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{instruction::{AccountMeta, Instruction}, program::invoke_signed};
use anchor_lang::Discriminator;
use dexter_contracts::billing::{SellerClaim, SourceReport};
use dexter_contracts::funded::{BookChange, BookStep, FinanceDetail};
use dexter_contracts::trading::WalletTradingDetailsData;
use dexter_contracts::wallet_operation::{WalletRouteQuote, WalletSpendRequest};
use solana_sha256_hasher::hashv;
use crate::funding_state::FundedClaim;

pub const TAB_DOMAIN: &[u8] = b"DEXTER:TAB:1";

/// Step id: sha256("DEXTER:TAB-STEP:1" || operation id || request digest).
/// Request digests exclude staleness guards and fee or rent payers.
pub fn step_id(operation_id: &[u8; 32], request_digest: &[u8; 32]) -> [u8; 32] {
    hashv(&[b"DEXTER:TAB-STEP:1", operation_id, request_digest]).to_bytes()
}

/// h0 = sha256("DEXTER:TAB:1" || tab || commit operation id || commit slot).
pub fn tab_genesis(tab: &Pubkey, operation_id: &[u8; 32], slot: u64) -> [u8; 32] {
    hashv(&[TAB_DOMAIN, tab.as_ref(), operation_id, &slot.to_le_bytes()]).to_bytes()
}

/// Operation id of a custody, facility or receiving-custody operation, bound to
/// its request: what the record's operation ring stores.
pub fn record_operation_id(operation_id: &[u8; 32], request_digest: &[u8; 32]) -> [u8; 32] {
    hashv(&[b"DEXTER:RECORD-OPERATION:1", operation_id, request_digest]).to_bytes()
}

/// The header every Tab event starts with.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub struct TabStepHeader {
    pub tab: Pubkey,
    /// This step's number: the Tab's step count after it.
    pub step: u32,
    pub step_id: [u8; 32],
}

/// What the earn step adds to its detail.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub struct EarnDetail {
    pub run_id: [u8; 32], pub units: u64, pub unit_price: u64, pub capture_digest: [u8; 32],
    pub payment_id: [u8; 32], pub run_fee_cap: u64, pub run_deposit_payer: Pubkey,
}

#[event]
pub struct TabCommitted {
    pub header: TabStepHeader,
    pub slot: u64,
    pub order: u64,
    /// The Tab as committed, before this step's own book entry.
    pub tab_state: FundedClaim,
    /// The order terms hash when the order showed its terms.
    pub order_terms_hash: Option<[u8; 32]>,
    pub detail: FinanceDetail,
    pub books: Vec<BookStep>,
    pub changes: Vec<BookChange>,
}

#[event]
pub struct RunEarned {
    pub header: TabStepHeader,
    pub earn: EarnDetail,
    pub detail: FinanceDetail,
    pub books: Vec<BookStep>,
    pub changes: Vec<BookChange>,
}

#[event]
pub struct ReportAdmitted {
    pub header: TabStepHeader,
    pub bill: Pubkey,
    pub report: SourceReport,
    pub report_digest: [u8; 32],
    pub supported_amount: u128,
    /// The delivery signer's key, or the rule program for a program-sourced order.
    pub signer: Pubkey,
    /// A recoverable order's source authority: its generation and the digest it
    /// authorized (zero for other orders, whose signer signs the digest itself).
    pub authority_generation: u64,
    pub authorization: [u8; 32],
}

#[event]
pub struct ClaimAdmitted {
    pub header: TabStepHeader,
    pub bill: Pubkey,
    pub seller_claim: SellerClaim,
    pub claim_digest: [u8; 32],
    /// The key that signed: the supplier's claimant, or a recoverable order's
    /// current supplier authority with its generation and authorization digest
    /// (zero for other orders).
    pub signer: Pubkey,
    pub authority_generation: u64,
    pub authorization: [u8; 32],
}

/// Evidence finalization: the earned run (when the amount is positive) and the
/// release of the rest, each with its own book entries, in that order.
#[event]
pub struct BillFinalized {
    pub header: TabStepHeader,
    pub bill: Pubkey,
    pub established: u64,
    pub released: u64,
    pub capture_digest: [u8; 32],
    pub earn: EarnDetail,
    pub earned: FinanceDetail,
    pub earned_books: Vec<BookStep>,
    pub earned_changes: Vec<BookChange>,
    pub release: FinanceDetail,
    pub release_books: Vec<BookStep>,
    pub release_changes: Vec<BookChange>,
}

/// One money step whose detail says everything: collection, release,
/// assignment and seller-fee credit.
macro_rules! detail_event {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {$(
        $(#[$doc])*
        #[event]
        pub struct $name {
            pub header: TabStepHeader,
            pub detail: FinanceDetail,
            pub books: Vec<BookStep>,
            pub changes: Vec<BookChange>,
        }
    )*};
}
detail_event!(
    /// Supplier collection (and any approved sponsorship legs).
    Collected,
    /// A run's title assigned to a new creditor.
    RunAssigned,
);

/// A seller fee credited back. `number` was admitted by the beneficiary's book
/// window (the beneficiary pays the credit from owner cash).
#[event]
pub struct FeeCredited {
    pub header: TabStepHeader,
    pub number: u64,
    pub detail: FinanceDetail,
    pub books: Vec<BookStep>,
    pub changes: Vec<BookChange>,
}

/// Unused capacity released: by the supplier before expiry (`surrendering_supplier`),
/// or by anyone after it.
#[event]
pub struct UnusedReleased {
    pub header: TabStepHeader,
    pub surrendering_supplier: bool,
    /// A surrender's number, admitted by the supplier's book window (0 after expiry).
    pub number: u64,
    pub detail: FinanceDetail,
    pub books: Vec<BookStep>,
    pub changes: Vec<BookChange>,
}

/// A step that writes or draws on a repayment record.
macro_rules! record_event {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {$(
        $(#[$doc])*
        #[event]
        pub struct $name {
            pub header: TabStepHeader,
            pub repayment_record: Pubkey,
            /// The standing grant or exact approval of a governed payment
            /// (default for ordinary cash).
            pub permission: Pubkey,
            /// The payment's number: for ordinary cash, admitted by the
            /// repayment record's window; for a governed payment, the grant's
            /// window number or the exact approval's number.
            pub number: u64,
            pub detail: FinanceDetail,
            pub books: Vec<BookStep>,
            pub changes: Vec<BookChange>,
        }
    )*};
}
record_event!(
    /// A run repayment.
    Repaid,
);

/// Who returned interest, from where, and to which original payer.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq, InitSpace)]
pub struct InterestRefundDetail {
    pub reason_hash: [u8; 32],
    pub lender_interest_refund: u64, pub protocol_interest_refund: u64,
    pub lender_principal: Pubkey, pub lender_authority: Pubkey, pub lender_from: Pubkey,
    pub protocol_principal: Pubkey, pub protocol_authority: Pubkey, pub protocol_from: Pubkey,
    pub refund_principal: Pubkey, pub refund_authority: Pubkey, pub refund_destination: Pubkey,
    /// Admitted by the book window of the party paying the refund: the lender
    /// when it refunds any interest, otherwise the protocol beneficiary.
    pub number: u64,
}

/// Interest returned to the original payer of `repayment_record`.
#[event]
pub struct InterestRefunded {
    pub header: TabStepHeader,
    pub repayment_record: Pubkey,
    pub refund: InterestRefundDetail,
    pub detail: FinanceDetail,
    pub books: Vec<BookStep>,
    pub changes: Vec<BookChange>,
}

/// A consensual correction, with its refunds. `repayment_record` is the record
/// a prior-repayment refund drew on (default when none).
#[event]
pub struct RunCorrected {
    pub header: TabStepHeader,
    pub repayment_record: Pubkey,
    pub correction: crate::corrections::CorrectionDetail,
    pub detail: FinanceDetail,
    pub books: Vec<BookStep>,
    pub changes: Vec<BookChange>,
}

#[event]
pub struct RepaymentRecordClosed {
    pub header: TabStepHeader,
    pub record: Pubkey,
    pub payer: Pubkey,
    pub lamports: u64,
}

#[event]
pub struct RunClosed {
    pub header: TabStepHeader,
    pub run: Pubkey,
    pub payer: Pubkey,
    pub lamports: u64,
}

#[event]
pub struct TabClosed {
    pub header: TabStepHeader,
    /// h_n: the fingerprint before this step, folded into the mandate's chain.
    pub prior_fingerprint: [u8; 32],
    /// n: the step count before this step.
    pub steps: u32,
    pub tab_payer: Pubkey,
    pub tab_lamports: u64,
    pub hold: Pubkey,
    pub hold_payer: Pubkey,
    pub hold_lamports: u64,
    pub bill: Pubkey,
    pub bill_payer: Pubkey,
    pub bill_lamports: u64,
    /// A recoverable evidence order's authority binding (default when none).
    pub binding: Pubkey,
    pub binding_payer: Pubkey,
    pub binding_lamports: u64,
}

/// A wallet send, swap or market trade: both legs, each with its own book entries
/// (`origin` is the leg id the book effect hashes bind).
#[event]
pub struct WalletSpent {
    pub operation_id: [u8; 32],
    pub request: WalletSpendRequest,
    pub request_digest: [u8; 32],
    pub executor: Pubkey,
    pub amount_out: u64,
    pub input_fee: u64,
    pub output_fee: u64,
    pub gross_output: u64,
    pub quote: WalletRouteQuote,
    pub quote_digest: [u8; 32],
    pub details: Option<WalletTradingDetailsData>,
    pub input_origin: [u8; 32],
    pub input_detail: FinanceDetail,
    pub input_books: Vec<BookStep>,
    pub input_changes: Vec<BookChange>,
    pub output_origin: [u8; 32],
    pub output_books: Vec<BookStep>,
    pub output_changes: Vec<BookChange>,
}

/// A custody, facility or receiving-custody operation. `number` is the move's
/// number: admitted by the custody's or receiving custody's window, or, for an
/// ordinary facility repayment, by the paying principal's finance book; for a
/// governed facility repayment it is the grant's or approval's number; zero for
/// a record's creation, which happens once.
#[event]
pub struct RecordOperation {
    pub kind: u8,
    pub operation: [u8; 32],
    pub record: Pubkey,
    pub number: u64,
    pub detail: FinanceDetail,
    pub books: Vec<BookStep>,
    pub changes: Vec<BookChange>,
}

/// A facility's limit or admission switch changed (today's `FacilityPolicyReceipt`).
#[event]
pub struct FacilityPolicyChanged {
    pub operation: [u8; 32],
    pub facility: Pubkey,
    pub request_id: [u8; 32],
    pub request_digest: [u8; 32],
    pub owner: Pubkey,
    pub authority_generation: u64,
    pub previous_revision: u64,
    pub resulting_revision: u64,
    pub previous_limit: u64,
    pub resulting_limit: u64,
    pub previously_enabled: bool,
    pub resulting_enabled: bool,
}

/// A token or stock custody lifecycle operation (setup, deposit, adoption,
/// withdrawal): its id, the number its custody's window admitted (zero for the
/// setup), its transfer facts.
#[event]
pub struct TokenOperation {
    pub operation: [u8; 32],
    pub custody: Pubkey,
    pub number: u64,
    pub transfer: crate::wallet_token_lifecycle::WalletTokenTransfer,
    pub detail: FinanceDetail,
    pub books: Vec<BookStep>,
    pub changes: Vec<BookChange>,
}

#[event]
pub struct ApprovalClosed {
    pub approval: Pubkey,
    pub principal: Pubkey,
    pub number: u64,
    pub used: bool,
    pub payer: Pubkey,
    pub lamports: u64,
}

/// One event's instruction data: `EVENT_IX_TAG || discriminator || Borsh body`,
/// written in place into a buffer allocated once. The SBF bump heap never frees,
/// and a Vec that outgrows its capacity strands the old buffer, so callers size
/// the body exactly (`books_space` for book entries), never to a maximum.
pub struct EventBytes { data: Vec<u8>, reserved: usize }

impl EventBytes {
    pub fn new<E: Discriminator>(body: usize) -> Self {
        let tag = anchor_lang::event::EVENT_IX_TAG_LE;
        let reserved = tag.len() + E::DISCRIMINATOR.len() + body;
        let mut data = Vec::with_capacity(reserved);
        data.extend_from_slice(tag);
        data.extend_from_slice(E::DISCRIMINATOR);
        Self { data, reserved }
    }
    pub fn put<T: AnchorSerialize + ?Sized>(mut self, value: &T) -> Result<Self> {
        value.serialize(&mut self.data)?;
        Ok(self)
    }
    pub fn body(&self) -> &[u8] { &self.data[anchor_lang::event::EVENT_IX_TAG_LE.len()..] }
    pub fn into_inner(self) -> Vec<u8> { self.data }
    /// Whether the event fit the buffer reserved for it, so it was never
    /// reallocated (unit tests check every writer at its largest).
    pub fn within_reserved(&self) -> bool { self.data.capacity() == self.reserved && self.data.len() <= self.reserved }
}

/// The exact Borsh size of one step's book entries (two length-prefixed vectors
/// of fixed-size items).
pub fn books_space(books: &[BookStep], changes: &[BookChange]) -> usize {
    8 + books.len() * BookStep::INIT_SPACE + changes.len() * BookChange::INIT_SPACE
}
pub const DETAIL_SPACE: usize = FinanceDetail::INIT_SPACE;

/// Emit one event by self-CPI signed by ClaimsCore's event authority.
#[inline(never)]
pub fn emit_event<'info>(authority: &AccountInfo<'info>, event: EventBytes) -> Result<()> {
    require_keys_eq!(*authority.key, crate::EVENT_AUTHORITY_AND_BUMP.0, crate::CoreError::EventAuthority);
    let instruction = Instruction { program_id: crate::ID,
        accounts: vec![AccountMeta::new_readonly(*authority.key, true)], data: event.into_inner() };
    invoke_signed(&instruction, &[authority.clone()],
        &[&[b"__event_authority", &[crate::EVENT_AUTHORITY_AND_BUMP.1]]])?;
    Ok(())
}

/// Emit a Tab step's event and record the step in the Tab's book: the step
/// count, the fingerprint and, for a money step, the money-step count. The
/// event header must carry `claim.next_step()`.
#[inline(never)]
pub fn record_tab_step<'info>(claim: &mut FundedClaim, authority: &AccountInfo<'info>, event: EventBytes,
    money: bool) -> Result<()> {
    let next = hashv(&[claim.fingerprint.as_ref(), event.body()]).to_bytes();
    emit_event(authority, event)?;
    claim.record_step(next, money)
}

/// Header, then a money step's detail and book entries.
#[inline(never)]
pub fn detail_event<E: Discriminator>(header: &TabStepHeader, detail: &FinanceDetail,
    books: &[BookStep], changes: &[BookChange]) -> Result<EventBytes> {
    EventBytes::new::<E>(TabStepHeader::INIT_SPACE + DETAIL_SPACE + books_space(books, changes))
        .put(header)?.put(detail)?.put(books)?.put(changes)
}

/// Header, a step-specific extra (a struct or tuple whose Borsh bytes are the
/// event's fields between the header and the detail), then detail and books.
#[inline(never)]
pub fn extra_event<E: Discriminator, T: AnchorSerialize + ?Sized>(header: &TabStepHeader, extra: &T,
    extra_space: usize, detail: &FinanceDetail, books: &[BookStep], changes: &[BookChange]) -> Result<EventBytes> {
    EventBytes::new::<E>(TabStepHeader::INIT_SPACE + extra_space + DETAIL_SPACE + books_space(books, changes))
        .put(header)?.put(extra)?.put(detail)?.put(books)?.put(changes)
}

/// Header, the repayment record and permission, then detail and book entries.
#[inline(never)]
pub fn record_event<E: Discriminator>(header: &TabStepHeader, record: &Pubkey, permission: &Pubkey,
    number: u64, detail: &FinanceDetail, books: &[BookStep], changes: &[BookChange]) -> Result<EventBytes> {
    EventBytes::new::<E>(TabStepHeader::INIT_SPACE + 32 + 32 + 8 + DETAIL_SPACE + books_space(books, changes))
        .put(header)?.put(record)?.put(permission)?.put(&number)?.put(detail)?.put(books)?.put(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::Event;

    fn body<E: Event>(event: &E) -> Vec<u8> {
        let mut data = anchor_lang::event::EVENT_IX_TAG_LE.to_vec();
        data.extend_from_slice(&event.data());
        data
    }
    fn header() -> TabStepHeader { TabStepHeader { tab: Pubkey::new_unique(), step: 7, step_id: [3; 32] } }
    fn detail() -> FinanceDetail {
        FinanceDetail { principal: Pubkey::new_unique(), amount: 5, kind: 13,
            sponsor_funding_source: Some(dexter_contracts::commercial::SponsorFundingSource::OwnerCash),
            ..FinanceDetail::default() }
    }
    fn books() -> (Vec<BookStep>, Vec<BookChange>) {
        let p = Pubkey::new_unique();
        (vec![BookStep { principal: p, sequence: 9 }],
            vec![BookChange { principal: p, field: 2, increase: true, amount: 4 }])
    }

    #[test]
    fn writers_match_their_event_structs() {
        let (h, d, (b, c)) = (header(), detail(), books());
        assert!(detail_event::<Collected>(&h, &d, &b, &c).unwrap().within_reserved());
        let built = detail_event::<Collected>(&h, &d, &b, &c).unwrap().into_inner();
        assert_eq!(built, body(&Collected { header: h, detail: d.clone(), books: b.clone(), changes: c.clone() }));
        let record = Pubkey::new_unique(); let permission = Pubkey::new_unique();
        let earn = EarnDetail { run_id: [1; 32], units: 2, unit_price: 3, capture_digest: [4; 32],
            payment_id: [5; 32], run_fee_cap: 6, run_deposit_payer: Pubkey::new_unique() };
        assert!(extra_event::<RunEarned, _>(&h, &earn, EarnDetail::INIT_SPACE, &d, &b, &c).unwrap().within_reserved());
        let built = extra_event::<RunEarned, _>(&h, &earn, EarnDetail::INIT_SPACE, &d, &b, &c).unwrap().into_inner();
        assert_eq!(built, body(&RunEarned { header: h, earn, detail: d.clone(), books: b.clone(), changes: c.clone() }));
        assert!(record_event::<Repaid>(&h, &record, &permission, 4, &d, &b, &c).unwrap().within_reserved());
        let built = record_event::<Repaid>(&h, &record, &permission, 4, &d, &b, &c).unwrap().into_inner();
        assert_eq!(built, body(&Repaid { header: h, repayment_record: record, permission, number: 4,
            detail: d.clone(), books: b.clone(), changes: c.clone() }));
        assert!(extra_event::<FeeCredited, _>(&h, &9u64, 8, &d, &b, &c).unwrap().within_reserved());
        let built = extra_event::<FeeCredited, _>(&h, &9u64, 8, &d, &b, &c).unwrap().into_inner();
        assert_eq!(built, body(&FeeCredited { header: h, number: 9, detail: d, books: b, changes: c }));
    }

    #[test]
    fn steps_extend_the_fingerprint_over_the_bytes_after_the_tag() {
        let h = header(); let (b, c) = books();
        let event = detail_event::<Collected>(&h, &detail(), &b, &c).unwrap();
        let prior = [8u8; 32];
        let expected = hashv(&[&prior, &event.body().to_vec()]).to_bytes();
        let mut data = event.into_inner();
        assert_eq!(&data[..8], anchor_lang::event::EVENT_IX_TAG_LE);
        assert_eq!(expected, hashv(&[&prior, &data.split_off(8)]).to_bytes());
    }
}
