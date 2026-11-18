//! Complete affected-book authentication and one atomic Pool projection CPI.
use anchor_lang::prelude::*;
use crate::CoreError;
use dexter_contracts::{plus, POOL_ID};
use dexter_contracts::funded::{self, FinanceBookEffect, FinanceReceiptData, MAX_FINANCIAL_BOOKS};
use crate::funding_state::{FinanceBook, FinanceReceipt};

#[derive(Clone, Copy, Debug)]
pub struct Change { pub principal:Pubkey, pub field:u8, pub increase:bool, pub amount:u64 }
pub fn inc(principal:Pubkey,field:u8,amount:u64)->Change { Change{principal,field,increase:true,amount} }
pub fn dec(principal:Pubkey,field:u8,amount:u64)->Change { Change{principal,field,increase:false,amount} }

/// Account order is public and deterministic: distinct principals sorted by
/// address, each with its Core book; projected operations interleave its Pool
/// projection. The offline path requires only Core books, never a Pool account.
/// This set is derived by the handler from authenticated economic roles.
pub fn journal<'info>(receipt:&mut Account<'info,FinanceReceipt>,mut data:FinanceReceiptData,
    changes:&[Change],remaining:&[AccountInfo<'info>],pool_program:&AccountInfo<'info>,
    service_without_pool:bool,new_risk:bool)->Result<()> {
    require!(!new_risk || !service_without_pool,CoreError::FundingProjection);
    require_keys_eq!(pool_program.key(),POOL_ID,CoreError::FundingProjection);
    let mut principals:Vec<Pubkey>=changes.iter().map(|x|x.principal).collect();
    principals.sort_unstable();principals.dedup();
    require!(!principals.is_empty() && principals.len() <= MAX_FINANCIAL_BOOKS,CoreError::FundingBookAccounts);
    let stride=if service_without_pool{1}else{2};
    require!(remaining.len()==principals.len()*stride,CoreError::FundingBookAccounts);
    let mut effects=Vec::with_capacity(principals.len());
    for (i,principal) in principals.iter().enumerate() {
        let info=&remaining[i*stride];
        require!(info.is_writable && info.key()==funded::book_address(principal,&data.mint),CoreError::FundingBookAccounts);
        let mut book=funded::read_finance_book(info)?;
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
        effect.hash=funded::book_effect_hash(&receipt.key(),&data.mint,&effect)?;
        book.sequence=effect.sequence;book.head=effect.hash;book.positions=after;
        FinanceBook{data:book}.try_serialize(&mut &mut info.try_borrow_mut_data()?[..])?;
        effects.push(effect);
    }
    data.effects=effects;
    require!(data.version==funded::FUNDED_VERSION && data.effects.len()<=MAX_FINANCIAL_BOOKS,
        dexter_contracts::ContractError::SchemaVersion);
    data.effects_hash=[0;32];
    receipt.data=data;
    // The receipt already owns a bounded allocation. Serialize its zero-hash
    // record there and hash that exact body, instead of consuming another full
    // receipt-sized heap allocation. Drop the account-data borrow before CPI.
    let effects_hash={
        let info=receipt.to_account_info();
        let mut bytes=info.try_borrow_mut_data()?;
        let allocated=bytes.len();
        let unused={
            let mut output=&mut bytes[..];
            receipt.try_serialize(&mut output)?;
            output.len()
        };
        let consumed=allocated.checked_sub(unused).ok_or_else(||error!(CoreError::FundingSchema))?;
        require!(consumed>=8,CoreError::FundingSchema);
        funded::financial_receipt_hash_from_serialized_body(&bytes[8..consumed])?
    };
    receipt.data.effects_hash=effects_hash;
    // Serialize before CPI. Neither projection nor a later indexer may infer
    // effects from mutable state after another operation has advanced the book.
    receipt.exit(&crate::ID)?;
    if !service_without_pool {
        pool_issuer::cpi::project_financing_batch(CpiContext::new(pool_program.key(),
            pool_issuer::cpi::accounts::ProjectFinancingBatch{receipt:receipt.to_account_info()})
            .with_remaining_accounts(remaining.to_vec()))?;
    }
    Ok(())
}
