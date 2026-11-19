//! Canonical financial journals for the wallet funding execution profile.
//! These are Core-owned facts. Pool projections consume immutable ordered effects.
use anchor_lang::prelude::*;
use solana_sha256_hasher::hashv;
use crate::{plus, minus, ContractError, CORE_ID, POOL_ID};

pub const FUNDED_VERSION: u16 = 1;
pub const MAX_FINANCIAL_BOOKS: usize = 8;
pub const INITIAL_CAPITAL: u8 = 10;
pub const FINANCE_ACCEPT: u8 = 11;
pub const FINANCE_EARN: u8 = 12;
pub const FINANCE_COLLECT: u8 = 13;
pub const FINANCE_RELEASE: u8 = 14;
pub const FINANCE_WITHDRAW: u8 = 15;
pub const FINANCE_REPAY: u8 = 16;
pub const FINANCE_ASSIGN: u8 = 17;
pub const FINANCE_FACILITY_REPAY: u8 = 18;
pub const FINANCE_REFUND: u8 = 19;
pub const FINANCE_CONTRIBUTE: u8 = 20;
pub const FINANCE_ADOPT_SURPLUS: u8 = 21;
pub const FINANCE_CORRECT: u8 = 22;
pub const FINANCE_INTEREST_REFUND: u8 = 23;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct FinancialPositions {
    pub unused_supplier: u64,
    pub unpaid_supplier: u64,
    pub customer_principal: u64,
    pub owned_principal: u64,
    pub funding_reserved: u64,
    pub facility_assets: u64,
    pub facility_debt: u64,
    pub interest_owed: u64,
    pub interest_owned: u64,
    /// Cumulative authenticated cash flows, not an automatically complete NAV.
    pub cash_received: u64,
    pub cash_sent: u64,
    pub seller_fees: u64,
    pub protocol_interest: u64,
    pub lender_interest: u64,
    pub sponsor_reimbursement: u64,
    pub sponsor_margin: u64,
    pub customer_reserved: u64,
    pub credit_charge_exposure: u64,
    pub seller_fee_refunds: u64,
    /// Explicit own-capital allocation, separate from income and token inflows.
    pub capital_allocated: u64,
    /// Supplier assets, distinct from an issuer's unused/payable liabilities
    /// when the same continuing principal holds more than one economic role.
    pub supplier_committed: u64,
    pub supplier_receivable: u64,
    pub seller_fees_paid: u64,
    pub seller_fee_credits_received: u64,
}
pub const UNUSED_SUPPLIER: u8 = 0;
pub const UNPAID_SUPPLIER: u8 = 1;
pub const CUSTOMER_PRINCIPAL: u8 = 2;
pub const OWNED_PRINCIPAL: u8 = 3;
pub const FUNDING_RESERVED: u8 = 4;
pub const FACILITY_ASSETS: u8 = 5;
pub const FACILITY_DEBT: u8 = 6;
pub const INTEREST_OWED: u8 = 7;
pub const INTEREST_OWNED: u8 = 8;
pub const CASH_RECEIVED: u8 = 9;
pub const CASH_SENT: u8 = 10;
pub const SELLER_FEES: u8 = 11;
pub const PROTOCOL_INTEREST: u8 = 12;
pub const LENDER_INTEREST: u8 = 13;
pub const SPONSOR_REIMBURSEMENT: u8 = 14;
pub const SPONSOR_MARGIN: u8 = 15;
pub const CUSTOMER_RESERVED: u8 = 16;
pub const CREDIT_CHARGE_EXPOSURE: u8 = 17;
pub const SELLER_FEE_REFUNDS: u8 = 18;
pub const CAPITAL_ALLOCATED: u8 = 19;
pub const SUPPLIER_COMMITTED: u8 = 20;
pub const SUPPLIER_RECEIVABLE: u8 = 21;
pub const SELLER_FEES_PAID: u8 = 22;
pub const SELLER_FEE_CREDITS_RECEIVED: u8 = 23;
impl FinancialPositions {
    pub fn apply(&mut self, field: u8, increase: bool, amount: u64) -> Result<()> {
        let slot = match field {
            UNUSED_SUPPLIER => &mut self.unused_supplier, UNPAID_SUPPLIER => &mut self.unpaid_supplier,
            CUSTOMER_PRINCIPAL => &mut self.customer_principal, OWNED_PRINCIPAL => &mut self.owned_principal,
            FUNDING_RESERVED => &mut self.funding_reserved, FACILITY_ASSETS => &mut self.facility_assets,
            FACILITY_DEBT => &mut self.facility_debt, INTEREST_OWED => &mut self.interest_owed,
            INTEREST_OWNED => &mut self.interest_owned, CASH_RECEIVED => &mut self.cash_received,
            CASH_SENT => &mut self.cash_sent, SELLER_FEES => &mut self.seller_fees,
            PROTOCOL_INTEREST => &mut self.protocol_interest, LENDER_INTEREST => &mut self.lender_interest,
            SPONSOR_REIMBURSEMENT => &mut self.sponsor_reimbursement, SPONSOR_MARGIN => &mut self.sponsor_margin,
            CUSTOMER_RESERVED => &mut self.customer_reserved, CREDIT_CHARGE_EXPOSURE => &mut self.credit_charge_exposure,
            SELLER_FEE_REFUNDS => &mut self.seller_fee_refunds,
            CAPITAL_ALLOCATED => &mut self.capital_allocated,
            SUPPLIER_COMMITTED => &mut self.supplier_committed,
            SUPPLIER_RECEIVABLE => &mut self.supplier_receivable,
            SELLER_FEES_PAID => &mut self.seller_fees_paid,
            SELLER_FEE_CREDITS_RECEIVED => &mut self.seller_fee_credits_received,
            _ => return err!(ContractError::UnknownEffect),
        };
        *slot = if increase { plus(*slot, amount)? } else { minus(*slot, amount)? }; Ok(())
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct FinanceBookData {
    pub version: u16, pub bump: u8, pub principal: Pubkey, pub mint: Pubkey,
    pub sequence: u64, pub head: [u8; 32], pub positions: FinancialPositions,
}
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct FinanceBookEffect {
    pub book: Pubkey, pub principal: Pubkey, pub sequence: u64,
    pub previous_hash: [u8; 32], pub before: FinancialPositions, pub after: FinancialPositions,
    pub hash: [u8; 32],
}
/// What one financial step did, beyond its book changes. Each Tab, wallet or
/// custody event carries one; fields a step does not use stay zero. This is
/// today's receipt body without its account links and effect list.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct FinanceDetail {
    pub principal: Pubkey, pub operation_id: [u8; 32],
    pub request_digest: [u8; 32], pub kind: u8, pub mint: Pubkey,
    pub claim: Pubkey, pub run: Pubkey, pub amount: u64,
    pub principal_paid: u64, pub interest_paid: u64,
    /// Collection credit after the accepted Tab fee, before a separate approved
    /// sponsorship debit. The exact sponsorship token legs are recorded below.
    pub supplier_net: u64, pub seller_fee: u64, pub seller_fee_waived: u64,
    pub protocol_interest: u64, pub protocol_waived: u64,
    pub sponsor_reimbursement: u64, pub sponsor_margin: u64,
    pub sponsor_quote_hash: [u8; 32], pub sponsor_payer_principal: Pubkey, pub sponsor_rent_owner: Pubkey,
    pub sponsor_payer_authority: Pubkey, pub sponsor_from: Pubkey, pub sponsor_destination: Pubkey,
    pub sponsor_funding_source: Option<crate::commercial::SponsorFundingSource>,
    pub supplier_proceeds_reimbursement: u64,
    pub seller_fee_credit: u64,
    pub capital_custody: Pubkey, pub capital_allocated: u64,
    pub capital_token_inflow: u64, pub capital_provenance_hash: [u8; 32],
    pub capital_authority: Pubkey, pub capital_from: Pubkey, pub capital_owner_generation: u64,
    /// Actual ordinary-cash funding identity and token debit, independent of
    /// the company whose obligation was serviced or the transaction fee payer.
    pub debit_principal: Pubkey, pub debit_authority: Pubkey, pub debit_from: Pubkey,
    /// Immutable title/destination used by repayment, or the resulting title
    /// after an assignment. Original supplier entitlement remains on the claim.
    pub previous_creditor: Pubkey, pub current_creditor: Pubkey,
    pub ownership_revision: u64, pub creditor_destination: Pubkey,
    pub assignment_consideration: u64, pub assignment_acquisition_cost: u64,
    pub previous_acquisition_basis: u64, pub new_acquisition_basis: u64,
    pub assignment_cost_beneficiary: Pubkey, pub assignment_cost_destination: Pubkey,
}

/// One financial book a step advanced: replaying the step's `changes` for this
/// principal from the book's state at `sequence - 1` gives the state at `sequence`.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub struct BookStep { pub principal: Pubkey, pub sequence: u64 }

/// One change to one position of one principal's book.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub struct BookChange { pub principal: Pubkey, pub field: u8, pub increase: bool, pub amount: u64 }

/// Most changes one step makes; the largest today (a correction with refunds) makes 16.
pub const MAX_BOOK_CHANGES: usize = 24;

pub fn book_address(principal: &Pubkey, mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"financial-book", principal.as_ref(), mint.as_ref()], &CORE_ID).0
}
pub fn projection_address(book: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"financial-projection", book.as_ref()], &POOL_ID).0
}
pub fn financial_digest<T: AnchorSerialize>(domain: &[u8], data: &T) -> Result<[u8; 32]> {
    let bytes = serialize_exact(data)?;
    Ok(hashv(&[domain, CORE_ID.as_ref(), &bytes]).to_bytes())
}

/// Fixed-layout commitments need no heap allocation. A native instruction's
/// bump allocator cannot reclaim dropped Vec buffers, so repeated request and
/// result hashes otherwise consume the route's remaining heap. Keep each
/// bounded buffer in a separate SBF frame and require the exact Borsh length.
#[inline(never)]
pub fn financial_digest_fixed<const N: usize, T: AnchorSerialize>(domain: &[u8], data: &T) -> Result<[u8; 32]> {
    let mut bytes = [0u8; N];
    {
        let mut output = &mut bytes[..];
        data.serialize(&mut output)?;
        require!(output.is_empty(), ContractError::WrongAccountType);
    }
    Ok(hashv(&[domain, CORE_ID.as_ref(), &bytes]).to_bytes())
}

/// The native default allocator is a 32 KiB bump allocator: dropped/reallocated
/// Vec buffers are not reclaimed during an instruction. Count Borsh bytes first
/// so every digest allocates only its final buffer, without changing the wire.
fn serialize_exact<T: AnchorSerialize>(data: &T) -> Result<Vec<u8>> {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.checked_add(bytes.len()).ok_or_else(||
                std::io::Error::new(std::io::ErrorKind::InvalidData, "serialized size overflow"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    let mut size = Count(0); data.serialize(&mut size)?;
    let mut bytes = Vec::with_capacity(size.0); data.serialize(&mut bytes)?;
    require!(bytes.len() == size.0, ContractError::WrongAccountType);
    Ok(bytes)
}
/// This fixed Borsh tuple is 552 bytes in the current schema. Keep it in its own
/// SBF frame instead of consuming the non-reclaiming heap once per book hash.
/// `origin` is the id of the step that made the change: a Tab step id, a wallet
/// trade leg id or a custody operation id. It takes the place the receipt
/// address held before Change 5, so the preimage keeps its size and order.
#[inline(never)]
pub fn book_effect_hash(origin: &[u8; 32], mint: &Pubkey, effect: &FinanceBookEffect) -> Result<[u8; 32]> {
    const PREIMAGE_SIZE: usize = 4 * 32 + 8 + 32 + 2 * FinancialPositions::INIT_SPACE;
    let mut bytes = [0u8; PREIMAGE_SIZE];
    {
        let mut output = &mut bytes[..];
        (origin, mint, &effect.book, &effect.principal, effect.sequence,
            &effect.previous_hash, &effect.before, &effect.after).serialize(&mut output)?;
        require!(output.is_empty(), ContractError::WrongAccountType);
    }
    Ok(hashv(&[b"DEXTER:FINANCE-BOOK-EFFECT:2", CORE_ID.as_ref(), &bytes]).to_bytes())
}
/// A Core `FinanceBook`: its rent payer, then its data.
pub fn read_finance_book_paid(account: &AccountInfo) -> Result<(Pubkey, FinanceBookData)> {
    // The book's account is its payer, its data, then its number window.
    require!(account.data_len() == 8 + 32 + FinanceBookData::INIT_SPACE + crate::window::OrderWindow::INIT_SPACE,
        ContractError::WrongAccountType);
    let (payer, data): (Pubkey, FinanceBookData) = read_core(account, b"account:FinanceBook")?;
    require!(data.version == FUNDED_VERSION, ContractError::SchemaVersion);
    let (expected, bump) = Pubkey::find_program_address(
        &[b"financial-book", data.principal.as_ref(), data.mint.as_ref()], &CORE_ID);
    require_keys_eq!(account.key(), expected, ContractError::WrongAccountType);
    require!(data.bump == bump, ContractError::WrongAccountType);
    Ok((payer, data))
}
pub fn read_finance_book(account: &AccountInfo) -> Result<FinanceBookData> {
    Ok(read_finance_book_paid(account)?.1)
}
fn read_core<T: AnchorDeserialize>(account: &AccountInfo, name: &[u8]) -> Result<T> {
    require_keys_eq!(*account.owner, CORE_ID, ContractError::WrongOwner);
    let bytes = account.try_borrow_data()?;
    let discriminator = hashv(&[name]).to_bytes();
    require!(bytes.len() >= 8 && bytes[..8] == discriminator[..8], ContractError::WrongAccountType);
    T::deserialize(&mut &bytes[8..]).map_err(|_| error!(ContractError::WrongAccountType))
}
