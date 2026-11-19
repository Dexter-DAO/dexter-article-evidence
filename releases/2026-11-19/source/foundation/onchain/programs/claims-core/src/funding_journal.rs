//! Complete affected-book authentication and one atomic Pool projection CPI.
//!
//! Change 5: no receipt. Each affected book's effect hash binds the step's
//! origin id (a Tab step id, a wallet leg id or a custody operation id), and the
//! step's event carries the book entries (`BookStep`s and `BookChange`s), so
//! replaying events in book-sequence order reproduces every book's positions
//! and head. Pool receives the same changes as CPI data and accepts them only
//! if they take its projection from its frontier to the book Core just wrote.
use anchor_lang::prelude::*;
use anchor_lang::Discriminator;
use anchor_lang::solana_program::{instruction::{AccountMeta, Instruction}, program::invoke};
use crate::CoreError;
use dexter_contracts::{plus, POOL_ID};
use dexter_contracts::funded::{self, BookChange, BookStep, FinanceBookEffect, MAX_BOOK_CHANGES, MAX_FINANCIAL_BOOKS};
use crate::funding_state::{admit_number, FinanceBook};

#[derive(Clone, Copy, Debug)]
pub struct Change { pub principal:Pubkey, pub field:u8, pub increase:bool, pub amount:u64 }
pub fn inc(principal:Pubkey,field:u8,amount:u64)->Change { Change{principal,field,increase:true,amount} }
pub fn dec(principal:Pubkey,field:u8,amount:u64)->Change { Change{principal,field,increase:false,amount} }

/// One step's book entries, for its event.
pub struct Booked { pub books: Vec<BookStep>, pub changes: Vec<BookChange> }

/// Account order is public and deterministic: distinct principals sorted by
/// address, each with its Core book; projected operations interleave its Pool
/// projection. The offline path requires only Core books, never a Pool account.
/// This set is derived by the handler from authenticated economic roles.
pub fn journal<'info>(origin:&[u8;32],mint:Pubkey,changes:&[Change],remaining:&[AccountInfo<'info>],
    pool_program:&AccountInfo<'info>,service_without_pool:bool,new_risk:bool)->Result<Booked> {
    journal_numbered(origin,mint,changes,remaining,pool_program,service_without_pool,new_risk,None)
}

/// The journal of a money move its payer pays from owner cash (an interest
/// refund, a seller-fee credit, a correction's cash legs): `numbered` names the
/// payer and its number, which the payer's book window admits once. The payer
/// must be one of the move's books.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub fn journal_numbered<'info>(origin:&[u8;32],mint:Pubkey,changes:&[Change],remaining:&[AccountInfo<'info>],
    pool_program:&AccountInfo<'info>,service_without_pool:bool,new_risk:bool,
    numbered:Option<(Pubkey,u64)>)->Result<Booked> {
    require!(!new_risk || !service_without_pool,CoreError::FundingProjection);
    require_keys_eq!(pool_program.key(),POOL_ID,CoreError::FundingProjection);
    require!(changes.len()<=MAX_BOOK_CHANGES,CoreError::FundingBookAccounts);
    let mut principals:Vec<Pubkey>=changes.iter().map(|x|x.principal).collect();
    principals.sort_unstable();principals.dedup();
    require!(!principals.is_empty() && principals.len() <= MAX_FINANCIAL_BOOKS,CoreError::FundingBookAccounts);
    if let Some((payer,_))=numbered { require!(principals.contains(&payer),CoreError::FundingBookAccounts); }
    let stride=if service_without_pool{1}else{2};
    require!(remaining.len()==principals.len()*stride,CoreError::FundingBookAccounts);
    let mut books=Vec::with_capacity(principals.len());
    for (i,principal) in principals.iter().enumerate() {
        let info=&remaining[i*stride];
        require!(info.is_writable && info.key()==funded::book_address(principal,&mint),CoreError::FundingBookAccounts);
        let (deposit_payer,mut book)=funded::read_finance_book_paid(info)?;
        let mut window=FinanceBook::try_deserialize(&mut &info.try_borrow_data()?[..])?.window;
        if let Some((payer,number))=numbered {
            if payer==*principal { admit_number(&mut window,number)?; }
        }
        if !service_without_pool {
            let projection=&remaining[i*stride+1];
            require!(pool_program.executable && projection.is_writable,CoreError::FundingProjection);
            require_keys_eq!(projection.key(),funded::projection_address(info.key),CoreError::FundingProjection);
            let p=pool_issuer::read_financial_projection(projection)?;
            pool_issuer::require_financing_caught_up(&p,&book)?;
        }
        let before=book.positions;let mut after=before;
        for change in changes.iter().filter(|x|x.principal==*principal) {
            after.apply(change.field,change.increase,change.amount)?;
        }
        let mut effect=FinanceBookEffect{book:info.key(),principal:*principal,
            sequence:plus(book.sequence,1)?,previous_hash:book.head,before,after,hash:[0;32]};
        effect.hash=funded::book_effect_hash(origin,&mint,&effect)?;
        book.sequence=effect.sequence;book.head=effect.hash;book.positions=after;
        FinanceBook{deposit_payer,data:book,window}.try_serialize(&mut &mut info.try_borrow_mut_data()?[..])?;
        books.push(BookStep{principal:*principal,sequence:effect.sequence});
    }
    let changes:Vec<BookChange>=changes.iter().map(|c|BookChange{principal:c.principal,field:c.field,
        increase:c.increase,amount:c.amount}).collect();
    if !service_without_pool {
        project(pool_program,remaining,origin,&mint,&changes)?;
    }
    Ok(Booked{books,changes})
}

/// Instruction data for a CPI: `discriminator || Borsh(args)`, the bytes Anchor's
/// generated CPI sends for the same arguments (`args` is the instruction's
/// argument tuple, borrowed). Allocated once at its final size: the generated
/// CPI grows a 256-byte buffer, and the bump heap keeps every outgrown one.
pub(crate) fn exact_data<T:AnchorSerialize>(discriminator:&[u8],args:&T)->Result<Vec<u8>> {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self,bytes:&[u8])->std::io::Result<usize> { self.0+=bytes.len(); Ok(bytes.len()) }
        fn flush(&mut self)->std::io::Result<()> { Ok(()) }
    }
    let mut size=Count(discriminator.len());args.serialize(&mut size)?;
    let mut data=Vec::with_capacity(size.0);
    data.extend_from_slice(discriminator);args.serialize(&mut data)?;
    Ok(data)
}

/// Pool's `project_financing_batch(origin, mint, changes)` with the book and
/// projection accounts as its account list, as Anchor's generated CPI sends it,
/// with the caller's account infos borrowed instead of copied.
#[inline(never)]
fn project<'info>(pool_program:&AccountInfo<'info>,remaining:&[AccountInfo<'info>],origin:&[u8;32],mint:&Pubkey,
    changes:&[BookChange])->Result<()> {
    let data=exact_data(pool_issuer::instruction::ProjectFinancingBatch::DISCRIMINATOR,&(origin,mint,changes))?;
    let accounts:Vec<AccountMeta>=remaining.iter().map(|x|AccountMeta{pubkey:*x.key,is_signer:x.is_signer,
        is_writable:x.is_writable}).collect();
    invoke(&Instruction{program_id:pool_program.key(),accounts,data},remaining)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::InstructionData;

    #[test]
    fn projection_cpi_data_is_the_generated_instruction() {
        let origin=[7u8;32];let mint=Pubkey::new_unique();
        let changes=vec![BookChange{principal:Pubkey::new_unique(),field:3,increase:true,amount:9},
            BookChange{principal:Pubkey::new_unique(),field:1,increase:false,amount:2}];
        let data=exact_data(pool_issuer::instruction::ProjectFinancingBatch::DISCRIMINATOR,
            &(&origin,&mint,changes.as_slice())).unwrap();
        assert_eq!(data.len(),data.capacity());
        assert_eq!(data,pool_issuer::instruction::ProjectFinancingBatch{origin,mint,changes}.data());
    }

    /// The commit's hold CPI (`funding::reserve`) sends each variant's argument
    /// tuple; it must encode exactly as the generated instruction struct.
    #[test]
    fn hold_cpi_data_is_the_generated_instruction() {
        use dexter_contracts::{AcceptArgs, TabTerms, billing::BillingPolicy};
        use principal_registry::{Term, instruction as registry};
        let mut terms=TabTerms::deserialize(&mut &vec![0u8;TabTerms::INIT_SPACE][..]).unwrap();
        terms.cap=7;terms.order=9;terms.principal=Pubkey::new_unique();terms.expires_at=-3;
        let accepted=AcceptArgs{operation_id:[5;32],terms};
        let mut policy=BillingPolicy::deserialize(&mut &vec![0u8;BillingPolicy::INIT_SPACE][..]).unwrap();
        policy.work_id=[6;32];policy.source=Pubkey::new_unique();policy.work_until=11;
        let shown=vec![Term{name:[1;32],value:[2;32],number:Some(-4)},Term{name:[3;32],value:[4;32],number:None}];
        let (namespace,unit,price,bill,claim)=([8u8;32],[9u8;32],13u64,Pubkey::new_unique(),Pubkey::new_unique());
        assert_eq!(exact_data(registry::Reserve::DISCRIMINATOR,&accepted).unwrap(),
            registry::Reserve{args:accepted.clone()}.data());
        assert_eq!(exact_data(registry::ReservePinned::DISCRIMINATOR,&(&accepted,&namespace,&unit,&price,&shown)).unwrap(),
            registry::ReservePinned{args:accepted.clone(),namespace,unit,unit_price:price,order_terms:shown.clone()}.data());
        assert_eq!(exact_data(registry::ReservePinnedBilling::DISCRIMINATOR,&(&accepted,&bill,&claim,&policy,&shown)).unwrap(),
            registry::ReservePinnedBilling{args:accepted,bill,claim,policy,order_terms:shown}.data());
    }
}
